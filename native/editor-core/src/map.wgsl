// One instance per rectangle of the engine's draw list: its four corners in world tiles, a
// tint, and where its image is in the atlas.
//
// The map itself is drawn the way the game's Clyde draws it: the atlas is sRGB, so a texel
// arrives in linear light, the tint is turned from sRGB to linear in the vertex shader, and the
// target is sRGB, so blending happens in linear light. The editor's own drawing (`vertex_overlay`
// and `fragment_overlay`) goes into the target's plain view instead, where its colours blend as
// the sRGB values they were chosen as.

struct View {
    scale: vec2<f32>,
    centre: vec2<f32>,
    atlas_size: f32,
    // One bit per map layer; a set bit hides it.
    hidden: u32,
    // The target's size in device pixels.
    pixels: vec2<f32>,
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

// Clyde's zFromSrgb (z-library.glsl).
fn from_srgb(srgb: vec3<f32>) -> vec3<f32> {
    let higher = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    let lower = srgb / 12.92;
    return select(higher, lower, srgb <= vec3<f32>(0.04045));
}

// Clyde's zToSrgb.
fn to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let higher = pow(linear, vec3<f32>(0.41666666666667)) * 1.055 - 0.055;
    let lower = linear * 12.92;
    return select(higher, lower, linear <= vec3<f32>(0.0031308));
}

fn place(index: u32, instance: Instance) -> Varying {
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

@vertex
fn vertex(@builtin(vertex_index) index: u32, instance: Instance) -> Varying {
    var out = place(index, instance);
    // The game puts every corner on the nearest corner of a screen pixel (base-default.vert),
    // so a sprite between two pixels lands on one of them whole.
    let snapped = floor((out.position.xy + 1.0) * 0.5 * view.pixels + 0.5) / view.pixels * 2.0 - 1.0;
    out.position = vec4<f32>(snapped, out.position.zw);
    // The tint is written in sRGB and the game turns it to linear here (base-default.vert).
    out.color = vec4<f32>(from_srgb(out.color.rgb), out.color.a);
    return out;
}

@vertex
fn vertex_overlay(@builtin(vertex_index) index: u32, instance: Instance) -> Varying {
    return place(index, instance);
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    // The game multiplies the texel by the tint (base-default.frag) and blends with SrcAlpha and
    // OneMinusSrcAlpha. Multiplying the alpha into the colour here and blending premultiplied
    // gives the same colour.
    let colour = textureSample(atlas, atlas_sampler, in.uv, in.layer) * in.color;
    return vec4<f32>(colour.rgb * colour.a, colour.a);
}

@fragment
fn fragment_overlay(in: Varying) -> @location(0) vec4<f32> {
    // The texel back in sRGB, to be tinted and blended as the sRGB value it is in the file.
    let texel = textureSample(atlas, atlas_sampler, in.uv, in.layer);
    let colour = vec4<f32>(to_srgb(texel.rgb), texel.a) * in.color;
    return vec4<f32>(colour.rgb * colour.a, colour.a);
}
