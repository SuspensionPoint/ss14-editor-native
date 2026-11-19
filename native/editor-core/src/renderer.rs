//! Draws a document's draw list on the GPU, with every rectangle of the map in one draw call.
//!
//! There is one instance per rectangle in the engine's drawing order, textures sit in an array
//! atlas, and sampling is nearest. Panning and zooming change one small uniform and nothing else.
//! Switching a layer off is just as cheap, because every rectangle carries its layer and the GPU
//! skips the hidden ones.
//!
//! Colours are handled as the game's Clyde handles them. The atlas and the backdrop are sRGB
//! textures (TextureLoadParameters.Default has Srgb set, Clyde.Textures.cs uploads them as
//! Srgb8Alpha8), the map is drawn through an sRGB view of the target (Clyde renders into an
//! Rgba8Srgb target with GL_FRAMEBUFFER_SRGB on), and a sprite's tint is turned to linear in the
//! vertex shader. So blending happens in linear light, as in the game. The editor's own drawing
//! (the air overlay, what is about to be placed, grid lines, the selection border) goes through
//! the target's plain view instead, so its colours blend as the sRGB values they were chosen as.
//!
//! On request it also draws two things of its own around the map ([`MapRenderer::set_surround`]):
//! the game's backdrop behind it, and the grid lines between cells over it.
//!
//! The renderer doesn't own a window or a frame. It draws into a texture it is handed, so the
//! same code works in a bare window and inside an interface toolkit's canvas.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::backdrop::{Backdrop, Placing};
use crate::drawlist::{DrawLists, Item, FLOATS_PER_QUAD, KIND_DECALS, KIND_DISPOSALS, KIND_MARKER, KIND_PLUMBING, KIND_SUBFLOOR, KIND_TILES, WHITE_TEXTURE};
use crate::textures::{BakedTextures, Sheet};

const LAYER_SIZE: u32 = 2048;
const PADDING: u32 = 1;

/// What the view looks at.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Centre of the view, in world tiles.
    pub x: f32,
    pub y: f32,
    /// Device pixels per world pixel; a tile is 32 world pixels.
    pub zoom: f32,
}

impl Camera {
    /// The world position under a point of a view, both in device pixels from its top left.
    pub fn world_at(&self, point: (f32, f32), view: (u32, u32)) -> (f32, f32) {
        let scale = 32.0 * self.zoom;
        (self.x + (point.0 - view.0 as f32 / 2.0) / scale, self.y - (point.1 - view.1 as f32 / 2.0) / scale)
    }

    /// Where a world position is in a view, in device pixels from its top left.
    pub fn view_at(&self, world: (f32, f32), view: (u32, u32)) -> (f32, f32) {
        let scale = 32.0 * self.zoom;
        (view.0 as f32 / 2.0 + (world.0 - self.x) * scale, view.1 as f32 / 2.0 - (world.1 - self.y) * scale)
    }
}

/// The layers a rectangle can belong to, as the web editor's Layers panel divides them. The
/// numbers are bits of the mask given to [`MapRenderer::set_view`].
pub mod group {
    pub const TILES: u32 = 0;
    pub const DECALS: u32 = 1;
    pub const MARKERS: u32 = 2;
    pub const SUBFLOOR: u32 = 3;
    pub const FLOOR_OBJECTS: u32 = 4;
    pub const STRUCTURES: u32 = 5;
    pub const OBJECTS: u32 = 6;
    pub const DOORS: u32 = 7;
    /// The editor's own drawing (what is about to be placed); never hidden.
    pub const OVERLAY: u32 = 8;
    /// Starlight's plumbing, apart from the rest of what lies under the floor so that it can be seen alone.
    pub const PLUMBING: u32 = 9;
    /// Disposal tubes, apart from the rest of what lies under the floor in the same way.
    pub const DISPOSALS: u32 = 10;
}

/// Which layer an item is in. Entities are divided by the game's draw depth, at the web
/// editor's boundaries (its `hiddenDepthRanges`), except what
/// the engine says lives under the floor: the game draws that above doors while it is being
/// shown through the floor, and it is the SubFloor layer all the same. What the engine says is
/// plumbing is the Plumbing layer, and disposal tubes the Disposals layer, whatever their depth.
pub fn group_of(kind: u8, depth: i32) -> u32 {
    match kind {
        KIND_TILES => group::TILES,
        KIND_DECALS => group::DECALS,
        KIND_MARKER => group::MARKERS,
        KIND_SUBFLOOR => group::SUBFLOOR,
        KIND_PLUMBING => group::PLUMBING,
        KIND_DISPOSALS => group::DISPOSALS,
        _ => match depth {
            i32::MIN..=-13 => group::SUBFLOOR,
            -12..=-5 => group::FLOOR_OBJECTS,
            -4..=-1 => group::STRUCTURES,
            8..=10 => group::DOORS,
            _ => group::OBJECTS,
        },
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    /// World to clip space: clip = scale * (world - centre).
    scale: [f32; 2],
    centre: [f32; 2],
    atlas_size: f32,
    /// One bit per layer; a set bit hides it.
    hidden: u32,
    /// The target's size in device pixels, for the map's corners to be put on its pixels.
    pixels: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OutlineUniform {
    /// Premultiplied.
    colour: [f32; 4],
    texel: [f32; 2],
    width: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    centre: [f32; 2],
    half: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    /// Premultiplied, in linear light.
    colour: [f32; 4],
    /// 0 repeated, 1 shown once, 2 a flat colour.
    mode: u32,
    /// 1 to take the picture's own pixels, nearest, as the game does. 0 to let the GPU take
    /// the smaller copies.
    sharp: u32,
    _padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridUniform {
    centre: [f32; 2],
    half: [f32; 2],
    origin: [f32; 2],
    turn: [f32; 2],
    /// Premultiplied, both.
    line: [f32; 4],
    axis: [f32; 4],
    pixels: f32,
    step: f32,
    _padding: [f32; 2],
}

/// The lines between the cells of a grid: where its origin is in the world, and how it is
/// turned, in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLines {
    pub origin: (f32, f32),
    pub rotation: f32,
}

/// What is drawn around the map: the backdrop behind it (the one last given to
/// [`MapRenderer::set_backdrop`], as it looks `seconds` after the editor started), and the
/// lines between cells over it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Surround {
    pub backdrop: bool,
    pub seconds: f64,
    pub grid: Option<GridLines>,
}

/// A layer of the backdrop on the GPU.
struct ShownLayer {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The picture's own size, which is what it is placed by, whatever size is on the GPU.
    pixels: (u32, u32),
    placing: Placing,
}

/// The backdrop and the grid lines: each a pass over the whole view.
struct Around {
    backdrop_pipeline: wgpu::RenderPipeline,
    backdrop_layout: wgpu::BindGroupLayout,
    backdrop_sampler: wgpu::Sampler,
    /// A flat black under the layers, as the game clears its view to. It is drawn whenever the
    /// backdrop is on, layers or not.
    floor: wgpu::BindGroup,
    layers: Vec<ShownLayer>,
    backdrop_id: u64,
    backdrop_shown: bool,
    grid_pipeline: wgpu::RenderPipeline,
    grid_uniform: wgpu::Buffer,
    grid_bind_group: wgpu::BindGroup,
    grid_shown: bool,
}

/// The border around what is selected: the selected things drawn on their own into a picture
/// of the view, and a pass that colours the pixels just outside that picture's shapes.
struct Selection {
    /// The selected items' rectangles.
    overlay: Overlay,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    /// The picture of the selected things, as large as the view, and what binds it.
    mask: Option<(wgpu::TextureView, wgpu::BindGroup, (u32, u32))>,
    /// Whether the mask holds the current selection, so that there is a border to draw.
    drawn: bool,
}

#[derive(Clone, Copy)]
struct AtlasRect {
    layer: u32,
    x: u32,
    y: u32,
}

/// Sprite frames packed into the layers of one array texture, placed as they are first needed.
struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    layers: u32,
    rects: HashMap<u32, AtlasRect>,
    missing: std::collections::HashSet<u32>,
    cursor: (u32, u32, u32, u32),
}

impl Atlas {
    fn new(device: &wgpu::Device, layers: u32) -> Atlas {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: wgpu::Extent3d { width: LAYER_SIZE, height: LAYER_SIZE, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB with straight alpha, as the baked file holds them (textures.rs).
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Atlas { texture, view, layers, rects: HashMap::new(), missing: Default::default(), cursor: (0, 0, 0, 0) }
    }

    /// Finds room for a frame. None means the atlas is full and has to grow.
    fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasRect> {
        let (mut layer, mut x, mut y, mut shelf) = self.cursor;
        if x + width + PADDING > LAYER_SIZE {
            x = 0;
            y += shelf + PADDING;
            shelf = 0;
        }
        if y + height + PADDING > LAYER_SIZE {
            layer += 1;
            x = 0;
            y = 0;
            shelf = 0;
        }
        if layer >= self.layers {
            return None;
        }
        self.cursor = (layer, x + width + PADDING, y, shelf.max(height));
        Some(AtlasRect { layer, x, y })
    }
}

/// What preparing a frame's data cost.
#[derive(Clone, Copy, Debug, Default)]
pub struct Prepared {
    /// Rectangles in the buffer, and those left out because their image is not in the baked file.
    pub drawn: u32,
    pub missing: u32,
    /// Sprite frames copied into the atlas by this call.
    pub textures_placed: u32,
    pub atlas_layers: u32,
    pub rebuilt: bool,
}

/// A few rectangles of the editor's own, drawn with the map: what is about to be placed.
#[derive(Default)]
struct Overlay {
    /// As given: rectangles in the draw list's layout, with texture ids.
    quads: Vec<f32>,
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
    count: u32,
}

pub struct MapRenderer {
    /// The map, through the target's sRGB view.
    pipeline: wgpu::RenderPipeline,
    /// The editor's own rectangles, through the target's plain view.
    overlay_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: u64,
    instance_count: u32,
    /// How many of the instances are tiles: they come first, and the overlay that stands in
    /// for tiles is drawn right after them, under everything else.
    tiles_end: u32,
    atlas: Atlas,
    built: Option<(i32, u64)>,
    /// The instance buffer's contents, kept so that a change can be patched in.
    scratch: Vec<f32>,
    /// Where each item's rectangles are in the buffer: offset and length, in floats.
    ranges: HashMap<i32, (usize, usize)>,
    under: Overlay,
    over: Overlay,
    overlays_stale: bool,
    /// The air overlay: drawn over the tiles and under all else, and put on the GPU again only
    /// when its number changes ([`Self::set_air`]).
    air: Overlay,
    air_version: Option<u64>,
    air_stale: bool,
    /// The sRGB form of the target's format, which the map is drawn through.
    srgb: wgpu::TextureFormat,
    selection: Selection,
    around: Around,
}

impl MapRenderer {
    /// A renderer for targets of a plain 8-bit format, such as `Rgba8Unorm`. Each target must
    /// also allow views in the sRGB form of it ([`Self::view_formats`]).
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> MapRenderer {
        let srgb = target.add_srgb_suffix();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // One instance: four corners, a tint, the atlas layer with the map layer, and the
        // source rectangle.
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2,
            4 => Float32x4, 5 => Float32, 6 => Float32x4
        ];
        let make = |vertex: &str, fragment: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("map"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: (FLOATS_PER_QUAD * 4) as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make("vertex", "fragment", srgb);
        let overlay_pipeline = make("vertex_overlay", "fragment_overlay", target);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instance_capacity = 4096 * (FLOATS_PER_QUAD * 4) as u64;
        let instances = Self::instance_buffer(device, instance_capacity);
        let selection = Self::selection(device, target);
        let around = Self::around(device, srgb, target);
        let atlas = Atlas::new(device, 4);
        let bind_group = Self::bind(device, &bind_group_layout, &uniform, &atlas, &sampler);

        MapRenderer {
            pipeline,
            overlay_pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            uniform,
            instances,
            instance_capacity,
            instance_count: 0,
            tiles_end: 0,
            atlas,
            built: None,
            scratch: Vec::new(),
            ranges: HashMap::new(),
            under: Overlay::default(),
            over: Overlay::default(),
            overlays_stale: false,
            air: Overlay::default(),
            air_version: None,
            air_stale: false,
            srgb,
            selection,
            around,
        }
    }

    /// A pipeline that covers the view with one triangle and blends its fragments over what
    /// is there.
    fn covering(device: &wgpu::Device, target: wgpu::TextureFormat, label: &str, source: &str, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vertex"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The formats a target's texture must allow views in, besides its own.
    pub fn view_formats(target: wgpu::TextureFormat) -> [wgpu::TextureFormat; 1] {
        [target.add_srgb_suffix()]
    }

    /// The backdrop is the game's and is drawn through the sRGB view. The grid lines are the
    /// editor's and are drawn through the plain one.
    fn around(device: &wgpu::Device, srgb: wgpu::TextureFormat, plain: wgpu::TextureFormat) -> Around {
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let backdrop_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                uniform_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let backdrop_pipeline = Self::covering(device, srgb, "backdrop", include_str!("backdrop.wgsl"), &backdrop_layout);
        // The game samples its textures nearest, with no smaller copies (TextureSampleParameters
        // .Default has no filter, Clyde.Textures.cs sets GL_NEAREST). While the view is within
        // the game's zoom the shader asks for the picture's own pixels (`sharp`), which the GPU
        // samples with the magnifying filter, nearest, however small they are drawn. Zoomed out
        // further than the game goes, it takes the smaller copies, smoothed, so that a picture
        // shrunk far does not shimmer.
        let backdrop_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("backdrop"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("backdrop floor"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let floor_uniform = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("backdrop floor"),
                contents: bytemuck::bytes_of(&LayerUniform {
                    centre: [0.0; 2],
                    half: [1.0; 2],
                    origin: [0.0; 2],
                    size: [1.0; 2],
                    colour: [0.0, 0.0, 0.0, 1.0],
                    mode: 2,
                    sharp: 1,
                    _padding: [0; 2],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let floor = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop floor"),
            layout: &backdrop_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: floor_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&blank.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&backdrop_sampler) },
            ],
        });

        let grid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("grid lines"), entries: &[uniform_entry] });
        let grid_pipeline = Self::covering(device, plain, "grid lines", include_str!("grid.wgsl"), &grid_layout);
        let grid_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid lines"),
            size: std::mem::size_of::<GridUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid lines"),
            layout: &grid_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: grid_uniform.as_entire_binding() }],
        });

        Around {
            backdrop_pipeline,
            backdrop_layout,
            backdrop_sampler,
            floor,
            layers: Vec::new(),
            backdrop_id: 0,
            backdrop_shown: false,
            grid_pipeline,
            grid_uniform,
            grid_bind_group,
            grid_shown: false,
        }
    }

    /// Puts a backdrop's pictures on the GPU, in place of the one that was there. Does nothing
    /// if it is the one that is there already. Whether it is drawn is up to
    /// [`Self::set_surround`].
    pub fn set_backdrop(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, backdrop: &Backdrop) {
        if self.around.backdrop_id == backdrop.id {
            return;
        }
        self.around.backdrop_id = backdrop.id;
        self.around.layers.clear();
        let limit = device.limits().max_texture_dimension_2d;
        for layer in &backdrop.layers {
            let picture = &layer.picture;
            // A picture larger than the GPU takes is shown from its first smaller copy that fits.
            let Some(first) = (0..picture.levels.len()).find(|&level| {
                let (width, height) = picture.level_size(level);
                width <= limit && height <= limit
            }) else {
                continue;
            };
            let (width, height) = picture.level_size(first);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("backdrop layer"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: (picture.levels.len() - first) as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (level, pixels) in picture.levels.iter().enumerate().skip(first) {
                let (width, height) = picture.level_size(level);
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: (level - first) as u32,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    pixels,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );
            }
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop layer"),
                size: std::mem::size_of::<LayerUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backdrop layer"),
                layout: &self.around.backdrop_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.around.backdrop_sampler) },
                ],
            });
            self.around.layers.push(ShownLayer { uniform, bind_group, pixels: (picture.width, picture.height), placing: layer.placing });
        }
    }

    /// What is drawn around the map in the next frames, as the camera sees it: see
    /// [`Surround`]. Without this call, nothing is.
    pub fn set_surround(&mut self, queue: &wgpu::Queue, camera: Camera, width: u32, height: u32, surround: Surround) {
        let pixels = 32.0 * camera.zoom;
        let centre = [camera.x, camera.y];
        let half = [width.max(1) as f32 / 2.0 / pixels, height.max(1) as f32 / 2.0 / pixels];

        // The game clears its view to black before anything is drawn (Clyde.Viewport.cs), so
        // with the backdrop on there is black behind the map even when the map has no layers,
        // or none of them could be read.
        self.around.backdrop_shown = surround.backdrop;
        if self.around.backdrop_shown {
            for layer in &self.around.layers {
                let (origin, size) = layer.placing.place(layer.pixels, (camera.x, camera.y), surround.seconds);
                // Zoomed in, a layer grows with the map, as in the game. Zoomed out further
                // than the game shows a map, it is the camera rising: a layer that follows
                // the camera entirely is infinitely far and stays the size it has at 100%, a
                // layer fixed to the world shrinks with the map, and the rest lie between.
                // (Shrunk with the map, a station seen whole would sit on a wallpaper of
                // thumbnail skies.)
                let far = layer.placing.slowness.clamp(0.0, 1.0);
                let zoom = camera.zoom + (camera.zoom.max(1.0) - camera.zoom) * far;
                let half = [half[0] * camera.zoom / zoom, half[1] * camera.zoom / zoom];
                let uniform = LayerUniform {
                    centre,
                    half,
                    origin: origin.into(),
                    size: size.into(),
                    colour: [1.0; 4],
                    mode: if layer.placing.tiled { 0 } else { 1 },
                    // As above, 100% is as far out as the game shows a map.
                    sharp: (camera.zoom >= 1.0) as u32,
                    _padding: [0; 2],
                };
                queue.write_buffer(&layer.uniform, 0, bytemuck::bytes_of(&uniform));
            }
        }

        self.around.grid_shown = surround.grid.is_some();
        if let Some(grid) = surround.grid {
            // The web editor's lines: fainter the further out, and further apart once cells
            // are under three pixels.
            let opacity = (0.06 * camera.zoom + 0.02).clamp(0.03, 0.2);
            let axis = (opacity * 4.0).min(0.4);
            let uniform = GridUniform {
                centre,
                half,
                origin: grid.origin.into(),
                turn: [grid.rotation.cos(), grid.rotation.sin()],
                line: [opacity; 4],
                axis: [100.0 / 255.0 * axis, 150.0 / 255.0 * axis, axis, axis],
                pixels,
                step: if pixels < 3.0 { (3.0 / pixels).ceil() } else { 1.0 },
                _padding: [0.0; 2],
            };
            queue.write_buffer(&self.around.grid_uniform, 0, bytemuck::bytes_of(&uniform));
        }
    }

    /// The border is the editor's own and is drawn through the target's plain view. Its mask is
    /// drawn with the map's pipeline, so it is in the sRGB form.
    fn selection(device: &wgpu::Device, target: wgpu::TextureFormat) -> Selection {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("selection border"),
            source: wgpu::ShaderSource::Wgsl(include_str!("outline.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection border"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection border"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection border"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vertex"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection border"),
            size: std::mem::size_of::<OutlineUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Selection { overlay: Overlay::default(), pipeline, layout, uniform, mask: None, drawn: false }
    }

    fn instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map instances"),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, atlas: &Atlas, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// The editor's own rectangles for the next frames, in the draw list's layout (texture ids
    /// from [`DrawLists`]): `under` is drawn over the tiles and under everything else (a tile
    /// that has been asked for and not yet answered), `over` on top of the map (what is about
    /// to be placed). They take effect at the next [`Self::prepare`].
    pub fn set_overlays(&mut self, under: &[f32], over: &[f32]) {
        if self.under.quads != under || self.over.quads != over {
            self.under.quads = under.to_vec();
            self.over.quads = over.to_vec();
            self.overlays_stale = true;
        }
    }

    /// The air overlay's rectangles (`crate::air::quads`), with the number they were built
    /// under. They are taken only when the number differs from the last, and take effect at
    /// the next [`Self::prepare`]. They are drawn over the tiles and under all else, as one
    /// batch, whatever the camera does.
    pub fn set_air(&mut self, version: u64, quads: &[f32]) {
        if self.air_version != Some(version) {
            self.air_version = Some(version);
            self.air.quads = quads.to_vec();
            self.air_stale = true;
        }
    }

    /// The rectangles of what is selected, in the draw list's layout, to be given a border
    /// that follows their shapes. They take effect at the next [`Self::prepare`], and the
    /// border is drawn once [`Self::selection_border`] has been called for the frame.
    pub fn set_selected(&mut self, quads: &[f32]) {
        if self.selection.overlay.quads != quads {
            self.selection.overlay.quads = quads.to_vec();
            self.overlays_stale = true;
        }
    }

    /// Draws the selected things on their own, as the camera last given to [`Self::set_view`]
    /// sees them, so that [`Self::draw`] can put a border around them. Call it after
    /// `set_view`, each frame. `width_pixels` is how wide the border is and `colour` what it
    /// looks like (red, green, blue, opacity).
    pub fn selection_border(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, width_pixels: f32, colour: [f32; 4]) {
        self.selection.drawn = false;
        let Some(buffer) = self.selection.overlay.buffer.as_ref().filter(|_| self.selection.overlay.count > 0 && width > 0 && height > 0) else { return };

        if self.selection.mask.as_ref().map(|mask| mask.2) != Some((width, height)) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("selection mask"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.srgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("selection border"),
                layout: &self.selection.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.selection.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.selection.mask = Some((view, bind_group, (width, height)));
        }
        let (view, _, _) = self.selection.mask.as_ref().unwrap();

        let uniform = OutlineUniform {
            colour: [colour[0] * colour[3], colour[1] * colour[3], colour[2] * colour[3], colour[3]],
            texel: [1.0 / width as f32, 1.0 / height as f32],
            width: width_pixels,
            _padding: 0.0,
        };
        queue.write_buffer(&self.selection.uniform, 0, bytemuck::bytes_of(&uniform));

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("selection mask"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..4, 0..self.selection.overlay.count);
        }
        queue.submit([encoder.finish()]);
        self.selection.drawn = true;
    }

    /// Makes sure the GPU has what the document's current draw list needs: its sprite frames in
    /// the atlas and its rectangles, in drawing order, in the instance buffer. Does nothing if
    /// neither the document nor the overlays have changed since the last call.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lists: &mut DrawLists, document: i32, textures: &BakedTextures) -> Prepared {
        let mut prepared = Prepared { atlas_layers: self.atlas.layers, drawn: self.instance_count, ..Default::default() };
        let version = lists.documents.get(&document).map(|d| d.version);
        if version.is_none() {
            self.instance_count = 0;
            self.tiles_end = 0;
            self.built = None;
            prepared.drawn = 0;
        }
        let stale = version.is_some_and(|version| self.built != Some((document, version)));
        if !stale && !self.overlays_stale && !self.air_stale {
            return prepared;
        }

        let same_document = self.built.map(|(built, _)| built) == Some(document);
        let (changed, order_changed) = if stale { lists.documents.get_mut(&document).unwrap().take_changes() } else { (Vec::new(), false) };
        // Rebuild the whole buffer, or patch the changed items into it.
        let mut rebuild = stale && (!same_document || order_changed);
        let mut patch = stale && !rebuild;

        // Place every frame the changed items and the overlays use that is not in the atlas
        // yet. If the atlas fills up, start again with one twice the size, and with every item.
        loop {
            let scope = if rebuild { Scope::Document } else if patch { Scope::Items(&changed) } else { Scope::Nothing };
            match self.place_textures(queue, lists, document, textures, scope) {
                Some(placed) => {
                    prepared.textures_placed += placed;
                    break;
                }
                None => {
                    self.atlas = Atlas::new(device, self.atlas.layers * 2);
                    self.bind_group = Self::bind(device, &self.bind_group_layout, &self.uniform, &self.atlas, &self.sampler);
                    prepared.textures_placed = 0;
                    rebuild = version.is_some();
                    patch = false;
                    self.overlays_stale = true;
                    self.air_stale = true;
                }
            }
        }
        prepared.atlas_layers = self.atlas.layers;

        // The usual edit changes a few items and moves nothing: write just those items' part of
        // the buffer. This holds when each changed item still has as many rectangles as before.
        if patch {
            let doc = &lists.documents[&document];
            let mut patches: Vec<(usize, Vec<f32>)> = Vec::with_capacity(changed.len());
            for id in &changed {
                let converted = doc.items.get(id).map(|item| self.convert_item(item, &mut prepared.missing)).unwrap_or_default();
                match self.ranges.get(id) {
                    Some(&(offset, length)) if length == converted.len() => patches.push((offset, converted)),
                    None if converted.is_empty() => {}
                    _ => {
                        rebuild = true;
                        break;
                    }
                }
            }
            if !rebuild {
                for (offset, converted) in patches {
                    self.scratch[offset..offset + converted.len()].copy_from_slice(&converted);
                    queue.write_buffer(&self.instances, (offset * 4) as u64, bytemuck::cast_slice(&converted));
                }
                prepared.rebuilt = true;
            } else {
                prepared.missing = 0;
            }
        }

        // Otherwise the whole buffer: the recorded rectangles in drawing order.
        if rebuild {
            let doc = lists.documents.get_mut(&document).unwrap();
            let order: Vec<i32> = doc.in_draw_order().to_vec();
            self.scratch.clear();
            self.ranges.clear();
            self.tiles_end = 0;
            for id in order {
                let item = &doc.items[&id];
                let converted = self.convert_item(item, &mut prepared.missing);
                self.ranges.insert(id, (self.scratch.len(), converted.len()));
                self.scratch.extend_from_slice(&converted);
                if item.kind == KIND_TILES {
                    self.tiles_end = (self.scratch.len() / FLOATS_PER_QUAD) as u32;
                }
            }

            let bytes: &[u8] = bytemuck::cast_slice(&self.scratch);
            if bytes.len() as u64 > self.instance_capacity {
                self.instance_capacity = (bytes.len() as u64).next_power_of_two();
                self.instances = Self::instance_buffer(device, self.instance_capacity);
            }
            queue.write_buffer(&self.instances, 0, bytes);
            self.instance_count = (self.scratch.len() / FLOATS_PER_QUAD) as u32;
            prepared.rebuilt = true;
        }
        if let Some(version) = version {
            self.built = Some((document, version));
        }

        if self.overlays_stale {
            self.overlays_stale = false;
            let mut ignored = 0;
            for which in 0..3 {
                let quads = [&self.under.quads, &self.over.quads, &self.selection.overlay.quads][which];
                let converted = self.convert(quads, group::OVERLAY, &mut ignored);
                let overlay = [&mut self.under, &mut self.over, &mut self.selection.overlay].into_iter().nth(which).unwrap();
                overlay.count = (converted.len() / FLOATS_PER_QUAD) as u32;
                if converted.is_empty() {
                    continue;
                }
                let bytes: &[u8] = bytemuck::cast_slice(&converted);
                if overlay.buffer.is_none() || bytes.len() as u64 > overlay.capacity {
                    overlay.capacity = (bytes.len() as u64).next_power_of_two().max(4096);
                    overlay.buffer = Some(Self::instance_buffer(device, overlay.capacity));
                }
                queue.write_buffer(overlay.buffer.as_ref().unwrap(), 0, bytes);
            }
        }

        if self.air_stale {
            self.air_stale = false;
            let mut ignored = 0;
            let converted = self.convert(&self.air.quads, group::OVERLAY, &mut ignored);
            self.air.count = (converted.len() / FLOATS_PER_QUAD) as u32;
            if !converted.is_empty() {
                let bytes: &[u8] = bytemuck::cast_slice(&converted);
                if self.air.buffer.is_none() || bytes.len() as u64 > self.air.capacity {
                    self.air.capacity = (bytes.len() as u64).next_power_of_two().max(4096);
                    self.air.buffer = Some(Self::instance_buffer(device, self.air.capacity));
                }
                queue.write_buffer(self.air.buffer.as_ref().unwrap(), 0, bytes);
            }
        }

        prepared.drawn = self.instance_count;
        prepared
    }

    fn convert_item(&self, item: &Item, missing: &mut u32) -> Vec<f32> {
        self.convert(&item.quads, group_of(item.kind, item.depth), missing)
    }

    /// Rectangles as the GPU takes them: the texture id turned into an atlas layer (with the
    /// map layer above it) and the source rectangle moved to the frame's place in the atlas.
    /// Rectangles whose image is not in the atlas are left out and counted.
    fn convert(&self, quads: &[f32], group: u32, missing: &mut u32) -> Vec<f32> {
        let mut converted = Vec::with_capacity(quads.len());
        for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
            let Some(rect) = self.atlas.rects.get(&(quad[12] as u32)) else {
                *missing += 1;
                continue;
            };
            converted.extend_from_slice(&quad[..12]);
            converted.push((rect.layer + group * 256) as f32);
            converted.extend_from_slice(&[rect.x as f32 + quad[13], rect.y as f32 + quad[14], rect.x as f32 + quad[15], rect.y as f32 + quad[16]]);
        }
        converted
    }

    /// Returns how many frames were placed, or None if the atlas ran out of room. `scope` says
    /// which of the document's items to look through; the overlays are always looked through.
    fn place_textures(&mut self, queue: &wgpu::Queue, lists: &DrawLists, document: i32, textures: &BakedTextures, scope: Scope<'_>) -> Option<u32> {
        let mut wanted: Vec<u32> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        {
            let mut want = |quads: &[f32]| {
                for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
                    let id = quad[12] as u32;
                    if !self.atlas.rects.contains_key(&id) && !self.atlas.missing.contains(&id) && seen.insert(id) {
                        wanted.push(id);
                    }
                }
            };
            if let Some(doc) = lists.documents.get(&document) {
                match scope {
                    Scope::Document => doc.items.values().for_each(|item| want(&item.quads)),
                    Scope::Items(ids) => ids.iter().filter_map(|id| doc.items.get(id)).for_each(|item| want(&item.quads)),
                    Scope::Nothing => {}
                }
            }
            want(&self.under.quads);
            want(&self.over.quads);
            want(&self.selection.overlay.quads);
            if self.air_stale {
                want(&self.air.quads);
            }
        }

        // Frames of one image are placed together, so the image is decompressed once.
        wanted.sort_unstable_by(|a, b| lists.textures.get(a).map(|t| t.file()).cmp(&lists.textures.get(b).map(|t| t.file())));
        let mut sheet: Option<(String, Option<Sheet>)> = None;
        let mut placed = 0;
        for id in wanted {
            let (width, height, pixels) = if id == WHITE_TEXTURE {
                (4, 4, vec![255u8; 64])
            } else {
                let Some(source) = lists.textures.get(&id) else {
                    self.atlas.missing.insert(id);
                    continue;
                };
                let file = source.file();
                if sheet.as_ref().map(|(name, _)| name != &file).unwrap_or(true) {
                    sheet = Some((file.clone(), textures.sheet(&file)));
                }
                let Some(pixels) = sheet.as_ref().and_then(|(_, s)| s.as_ref()).and_then(|s| s.frame(source.index, source.width, source.height)) else {
                    self.atlas.missing.insert(id);
                    continue;
                };
                (source.width, source.height, pixels)
            };

            let rect = self.atlas.allocate(width, height)?;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: rect.x, y: rect.y, z: rect.layer },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
            self.atlas.rects.insert(id, rect);
            placed += 1;
        }
        Some(placed)
    }

    /// Where the camera is for the next [`Self::draw`], and which layers are hidden (one bit
    /// per [`group`]).
    pub fn set_view(&self, queue: &wgpu::Queue, camera: Camera, width: u32, height: u32, hidden: u32) {
        let uniform = ViewUniform {
            scale: [32.0 * camera.zoom * 2.0 / width.max(1) as f32, 32.0 * camera.zoom * 2.0 / height.max(1) as f32],
            centre: [camera.x, camera.y],
            atlas_size: LAYER_SIZE as f32,
            hidden,
            pixels: [width.max(1) as f32, height.max(1) as f32],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// The draw calls, into `target`: a texture as large as the map's area, of the format given
    /// to [`Self::new`], that allows the views in [`Self::view_formats`]. It is first cleared to
    /// `clear`, a colour in sRGB.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::Texture, clear: wgpu::Color) {
        let plain = target.create_view(&Default::default());
        let srgb = target.create_view(&wgpu::TextureViewDescriptor { format: Some(self.srgb), ..Default::default() });
        // The clear goes through the plain view, so that its colour lands as given.
        drop(Self::pass(encoder, &plain, wgpu::LoadOp::Clear(clear)));

        // The backdrop and the map go through the sRGB view. The air overlay and what stands in
        // for tiles not yet answered are the editor's, drawn through the plain view over the
        // tiles and under all else.
        let tiles_end = self.tiles_end.min(self.instance_count);
        let air = self.air.buffer.as_ref().filter(|_| self.air.count > 0).map(|buffer| (buffer, self.air.count));
        let under = self.under.buffer.as_ref().filter(|_| self.under.count > 0).map(|buffer| (buffer, self.under.count));
        let between = air.is_some() || under.is_some();
        {
            let mut pass = Self::pass(encoder, &srgb, wgpu::LoadOp::Load);
            if self.around.backdrop_shown {
                pass.set_pipeline(&self.around.backdrop_pipeline);
                pass.set_bind_group(0, &self.around.floor, &[]);
                pass.draw(0..3, 0..1);
                for layer in &self.around.layers {
                    pass.set_bind_group(0, &layer.bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
            let end = if between { tiles_end } else { self.instance_count };
            if end > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..));
                pass.draw(0..4, 0..end);
            }
        }
        if between {
            {
                let mut pass = Self::pass(encoder, &plain, wgpu::LoadOp::Load);
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                for (buffer, count) in [air, under].into_iter().flatten() {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..4, 0..count);
                }
            }
            if self.instance_count > tiles_end {
                let mut pass = Self::pass(encoder, &srgb, wgpu::LoadOp::Load);
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..));
                pass.draw(0..4, tiles_end..self.instance_count);
            }
        }

        // The editor's own drawing over the map, through the plain view.
        let mut pass = Self::pass(encoder, &plain, wgpu::LoadOp::Load);
        if self.around.grid_shown {
            pass.set_pipeline(&self.around.grid_pipeline);
            pass.set_bind_group(0, &self.around.grid_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(over) = self.over.buffer.as_ref().filter(|_| self.over.count > 0) {
            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, over.slice(..));
            pass.draw(0..4, 0..self.over.count);
        }
        if let (true, Some((_, bind_group, _))) = (self.selection.drawn, self.selection.mask.as_ref()) {
            pass.set_pipeline(&self.selection.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn pass<'a>(encoder: &'a mut wgpu::CommandEncoder, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("map"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

/// Which of a document's items to look through for frames that are not in the atlas yet.
enum Scope<'a> {
    Document,
    Items(&'a [i32]),
    Nothing,
}
