// The lines between cells, drawn over the whole view on top of the map: one device pixel wide
// whatever the zoom, with the two lines through the grid's origin in another colour.

struct Grid {
    // The view in the world: its centre and half its size, in tiles.
    centre: vec2<f32>,
    half: vec2<f32>,
    // Where the grid's origin is in the world, and the cosine and sine of its rotation.
    origin: vec2<f32>,
    turn: vec2<f32>,
    // Premultiplied.
    line: vec4<f32>,
    axis: vec4<f32>,
    // Device pixels to a tile, and how many cells apart the lines are (more than one when
    // zoomed far out, so that they do not run together).
    pixels: f32,
    step: f32,
};

@group(0) @binding(0) var<uniform> grid: Grid;

struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) across: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Varying {
    let corner = vec2<f32>(f32(i32(index & 1u) * 4 - 1), f32(i32(index >> 1u) * 4 - 1));
    var out: Varying;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    out.across = corner;
    return out;
}

// Whether a pixel whose centre is `away` pixels from a line is the one pixel that shows it.
// The small shift decides for one side when the line runs exactly between two pixels.
fn on_line(away: f32) -> bool {
    let shifted = away + 0.01;
    return shifted > -0.5 && shifted <= 0.5;
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    let offset = grid.centre + in.across * grid.half - grid.origin;
    // In the grid's own cells.
    let local = vec2<f32>(offset.x * grid.turn.x + offset.y * grid.turn.y, offset.y * grid.turn.x - offset.x * grid.turn.y);
    let to_axis = local * grid.pixels;
    if on_line(to_axis.x) || on_line(to_axis.y) {
        return grid.axis;
    }
    let cell = local / grid.step;
    let away = (cell - round(cell)) * grid.step * grid.pixels;
    if on_line(away.x) || on_line(away.y) {
        return grid.line;
    }
    discard;
}
