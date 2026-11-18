// One instance per rectangle of the engine's draw list: its four corners in world tiles, a
// tint, and where its image is in the atlas.

struct View {
    scale: vec2<f32>,
    centre: vec2<f32>,
    atlas_size: f32,
    // One bit per map layer; a set bit hides it.
    hidden: u32,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Instance {
    @location(0) bottom_left: vec2<f32>,
    @location(1) bottom_right: vec2<f32>,
    @location(2) top_right: vec2<f32>,
    @location(3) top_left: vec2<f32>,
    @location(4) color: vec4<f32>,
    // The atlas layer, plus 256 times the map layer the rectangle belongs to.
    @location(5) layer: f32,
    // left, top, right, bottom, in atlas pixels
    @location(6) source: vec4<f32>,
};

struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) color: vec4<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32, instance: Instance) -> Varying {
    // A triangle strip over the rectangle: bottom-left, bottom-right, top-left, top-right.
    var position: vec2<f32>;
    var pixel: vec2<f32>;
    switch index {
        case 0u: { position = instance.bottom_left; pixel = instance.source.xw; }
        case 1u: { position = instance.bottom_right; pixel = instance.source.zw; }
        case 2u: { position = instance.top_left; pixel = instance.source.xy; }
        default: { position = instance.top_right; pixel = instance.source.zy; }
    }

    let packed = u32(instance.layer);
    var out: Varying;
    out.position = vec4<f32>((position - view.centre) * view.scale, 0.0, 1.0);
    if ((view.hidden >> (packed >> 8u)) & 1u) == 1u {
        // A hidden layer: every corner in one place outside the view, so nothing is drawn.
        out.position = vec4<f32>(2.0, 2.0, 0.0, 1.0);
    }
    out.uv = pixel / view.atlas_size;
    out.layer = packed & 255u;
    out.color = instance.color;
    return out;
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    // The atlas holds premultiplied colour; tint it the same way.
    let texel = textureSample(atlas, atlas_sampler, in.uv, in.layer);
    return texel * vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
