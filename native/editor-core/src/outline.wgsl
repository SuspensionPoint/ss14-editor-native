// The border around what is selected. The selected things are first drawn on their own into a
// picture the size of the view (the mask). This pass covers the view and colours every pixel
// that is outside that picture's shapes but within the border's width of one, so the border
// follows the outline of the sprites themselves, however many pieces they are drawn from.

struct Outline {
    // premultiplied
    colour: vec4<f32>,
    // One pixel of the mask, in texture coordinates.
    texel: vec2<f32>,
    // The border's width in pixels.
    width: f32,
};

@group(0) @binding(0) var<uniform> outline: Outline;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var mask_sampler: sampler;

struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Varying {
    // One triangle that covers the view.
    let corner = vec2<f32>(f32(i32(index & 1u) * 4 - 1), f32(i32(index >> 1u) * 4 - 1));
    var out: Varying;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return out;
}

fn covered(uv: vec2<f32>) -> bool {
    return textureSampleLevel(mask, mask_sampler, uv, 0.0).a > 0.1;
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    if covered(in.uv) {
        discard;
    }
    // Look around at the border's width and at half of it, so that thin shapes are not missed.
    var near = false;
    for (var ring = 1; ring <= 2; ring++) {
        let reach = outline.width * f32(ring) * 0.5;
        for (var step = 0; step < 16; step++) {
            let angle = f32(step) * 0.39269908;
            if covered(in.uv + vec2<f32>(cos(angle), sin(angle)) * reach * outline.texel) {
                near = true;
            }
        }
    }
    if !near {
        discard;
    }
    return outline.colour;
}
