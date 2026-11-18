// One layer of the backdrop behind the map, drawn over the whole view: a picture placed in the
// world (see backdrop.rs for where), repeated or shown once, or a flat colour under them all.

struct Layer {
    // The view in the world: its centre and half its size, in tiles.
    centre: vec2<f32>,
    half: vec2<f32>,
    // The picture's bottom left corner and its size in the world, in tiles.
    origin: vec2<f32>,
    size: vec2<f32>,
    // Premultiplied. The flat colour, or what the picture is multiplied by.
    colour: vec4<f32>,
    // 0 repeated, 1 shown once, 2 a flat colour.
    mode: u32,
};

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var picture_sampler: sampler;

struct Varying {
    @builtin(position) position: vec4<f32>,
    // -1 to 1 across the view, y up.
    @location(0) across: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Varying {
    // One triangle that covers the view.
    let corner = vec2<f32>(f32(i32(index & 1u) * 4 - 1), f32(i32(index >> 1u) * 4 - 1));
    var out: Varying;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    out.across = corner;
    return out;
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    let world = layer.centre + in.across * layer.half;
    let at = (world - layer.origin) / layer.size;
    // The picture's first row is its top; the world's y goes up.
    let texel = textureSample(picture, picture_sampler, vec2<f32>(at.x, 1.0 - at.y));
    if layer.mode == 2u {
        return layer.colour;
    }
    if layer.mode == 1u && (at.x < 0.0 || at.y < 0.0 || at.x > 1.0 || at.y > 1.0) {
        discard;
    }
    return texel * layer.colour;
}
