//! Drawing the air overlay (`crate::air` has the rules). This makes the rectangles the renderer
//! takes for each tile, its mark, and the leaks' outlines and arrows. It also places room labels,
//! decides what shows at each zoom level, and paints the legend's sample tiles into pixels.
//!
//! Rectangles are in world tiles. Marks are thin rectangles too, so the renderer needs nothing
//! new. A rectangle with two corners in the same place makes a triangle.

use std::collections::HashMap;

use crate::air::{self, breathable, gas_name, AirMirror, AirView, Breath, GridAir, Icon, Look, Mark, Mix, Sample, LEAK, NO_AIR, ROOM_COLOURS};
use crate::drawlist::{WHITE_SOURCE, WHITE_TEXTURE};
use crate::model::Grid;

/// Below this many pixels a tile on screen, in the interface's own pixels, marks are not drawn.
pub const MARKS_FROM: f32 = 8.0;
/// From this many they are drawn in full. Between, they fade in quarters, so the rectangles
/// are built again at most four times on the way.
pub const MARKS_FULL: f32 = 12.0;
/// Below this many pixels a tile, no room is labelled.
pub const LABELS_FROM: f32 = 10.0;
/// A room is labelled only when it covers this many square pixels on screen, about a label's
/// own size, so that labels do not crowd small rooms.
pub const LABEL_ROOM_AREA: f32 = 1200.0;

/// How strongly the rooms' soft colours cover the floor.
const ROOM_ALPHA: f32 = 0.45;
/// The black of the caution tape.
const TAPE_BLACK: [u8; 3] = [20, 20, 20];
/// How wide a leak's dashed outline is, in tiles.
const OUTLINE_WIDTH: f32 = 0.11;
/// The dark ink of an icon in its circle.
const ICON_INK: [u8; 3] = [10, 10, 22];

/// How strongly marks are drawn at a size of tile on screen: none below [`MARKS_FROM`], all
/// from [`MARKS_FULL`], and quarters between.
pub fn mark_strength(tile_pixels: f32) -> f32 {
    if tile_pixels.is_nan() {
        return 1.0;
    }
    let strength = ((tile_pixels - MARKS_FROM) / (MARKS_FULL - MARKS_FROM)).clamp(0.0, 1.0);
    (strength * 4.0).floor() / 4.0
}

/// Whether a room of this many tiles is labelled at a size of tile on screen.
pub fn label_shown(tiles: usize, tile_pixels: f32) -> bool {
    tile_pixels >= LABELS_FROM && tiles as f32 * tile_pixels * tile_pixels >= LABEL_ROOM_AREA
}

/// A point in a tile, from its top left corner across and down, as a picture of it is drawn.
type Point = (f32, f32);

/// One rectangle of the overlay, by its corners in world tiles: bottom left, bottom right,
/// top right, top left, or any four in order round.
fn quad(corners: [Point; 4], colour: [u8; 3], alpha: f32, out: &mut Vec<f32>) {
    for corner in corners {
        out.extend_from_slice(&[corner.0, corner.1]);
    }
    out.extend_from_slice(&[colour[0] as f32 / 255.0, colour[1] as f32 / 255.0, colour[2] as f32 / 255.0, alpha]);
    out.push(WHITE_TEXTURE as f32);
    out.extend_from_slice(&WHITE_SOURCE);
}

/// A straight stroke from one point to another, as four corners round.
fn stroke(from: Point, to: Point, width: f32) -> [Point; 4] {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (nx, ny) = (-dy / length * width / 2.0, dx / length * width / 2.0);
    [(from.0 - nx, from.1 - ny), (to.0 - nx, to.1 - ny), (to.0 + nx, to.1 + ny), (from.0 + nx, from.1 + ny)]
}

/// A small square about a point.
fn square(at: Point, side: f32) -> [Point; 4] {
    let half = side / 2.0;
    [(at.0 - half, at.1 - half), (at.0 + half, at.1 - half), (at.0 + half, at.1 + half), (at.0 - half, at.1 + half)]
}

/// The shapes of a mark in a tile one wide, from its top left corner across and down, after
/// the approved mockup's tile of 22 pixels. The tape's are its black stripes.
fn mark_shapes(mark: Mark) -> Vec<[Point; 4]> {
    let px = |value: f32| value / 22.0;
    match mark {
        Mark::None => Vec::new(),
        Mark::Snowflake => (0..3)
            .map(|index| {
                let (sin, cos) = (index as f32 * std::f32::consts::PI / 3.0).sin_cos();
                let reach = px(5.0);
                stroke((0.5 - cos * reach, 0.5 - sin * reach), (0.5 + cos * reach, 0.5 + sin * reach), px(1.3))
            })
            .collect(),
        Mark::Dots => [(6.0, 6.0), (16.0, 11.0), (9.0, 17.0)].iter().map(|&(x, y)| square((px(x), px(y)), px(3.6))).collect(),
        Mark::Bars => [4.0, 10.0, 16.0].iter().map(|&x| stroke((px(x), px(3.0)), (px(x), px(19.0)), px(1.3))).collect(),
        Mark::Waves => {
            // Each a wave of one length across the tile, in eight straight pieces.
            let mut shapes = Vec::new();
            for line in [6.0, 13.0, 20.0] {
                let at = |step: usize| {
                    let across = step as f32 / 8.0;
                    (across, px(line) - px(1.7) * (across * std::f32::consts::TAU).sin())
                };
                shapes.extend((0..8).map(|step| stroke(at(step), at(step + 1), px(1.2))));
            }
            shapes
        }
        Mark::Stars => {
            // Each a small cross.
            let mut shapes = Vec::new();
            for (x, y, reach) in [(5.0, 6.0, 1.8), (15.0, 14.0, 1.8), (11.0, 3.5, 1.3)] {
                shapes.push(stroke((px(x - reach), px(y)), (px(x + reach), px(y)), px(1.0)));
                shapes.push(stroke((px(x), px(y - reach)), (px(x), px(y + reach)), px(1.0)));
            }
            shapes
        }
        Mark::Tape => {
            // Bands across the tile where across plus down is from one quarter to the next,
            // every other one black. None spans the middle diagonal, so each is four corners
            // round, and they meet those of the next tile.
            let mut shapes = Vec::new();
            for band in (0..8).step_by(2) {
                let (low, high) = (band as f32 * 0.25, band as f32 * 0.25 + 0.25);
                shapes.push(if high <= 1.0 { [(low, 0.0), (high, 0.0), (0.0, high), (0.0, low)] } else { [(1.0, low - 1.0), (1.0, high - 1.0), (high - 1.0, 1.0), (low - 1.0, 1.0)] });
            }
            shapes
        }
    }
}

/// The marks' shapes, each worked out once for a build.
struct Marks(HashMap<Mark, Vec<[Point; 4]>>);

impl Marks {
    fn new() -> Marks {
        Marks(HashMap::new())
    }

    fn of(&mut self, mark: Mark) -> &[[Point; 4]] {
        self.0.entry(mark).or_insert_with(|| mark_shapes(mark))
    }
}

/// A tile of a kind: its colour over the floor, and its mark as strong as `marks` says. Its
/// corner is at `cell` of the grid, whose coordinates `world` turns into the map's.
fn tile(world: &impl Fn(Point) -> Point, cell: (i32, i32), look: &Look, marks: f32, shapes: &mut Marks, out: &mut Vec<f32>) {
    let (x, y) = (cell.0 as f32, cell.1 as f32);
    // Down the picture of a tile is down the grid.
    let at = |point: Point| world((x + point.0, y + 1.0 - point.1));
    quad([at((0.0, 1.0)), at((1.0, 1.0)), at((1.0, 0.0)), at((0.0, 0.0))], look.colour, look.alpha, out);
    if marks <= 0.0 || look.mark == Mark::None {
        return;
    }
    let (colour, alpha) = match look.mark {
        Mark::Tape => (TAPE_BLACK, 0.9),
        _ => {
            let [red, green, blue, alpha] = look.ink();
            ([red, green, blue], alpha as f32 / 255.0)
        }
    };
    for shape in shapes.of(look.mark) {
        quad(shape.map(at), colour, alpha * marks, out);
    }
}

/// The sides of a cell: the step across it, and its edge from one end to the other in the cell.
const EDGES: [((i32, i32), Point, Point); 4] = [((0, 1), (0.0, 1.0), (1.0, 1.0)), ((0, -1), (0.0, 0.0), (1.0, 0.0)), ((1, 0), (1.0, 0.0), (1.0, 1.0)), ((-1, 0), (0.0, 0.0), (0.0, 1.0))];

/// The dashed orange edges of a cell of a room open to the outside, on each side whose
/// neighbour is not in the room, just inside it so that the walls do not cover them.
fn dashed_edges(world: &impl Fn(Point) -> Point, cell: (i32, i32), inside: impl Fn((i32, i32)) -> bool, out: &mut Vec<f32>) {
    let (x, y) = (cell.0 as f32, cell.1 as f32);
    for ((dx, dy), from, to) in EDGES {
        if inside((cell.0 + dx, cell.1 + dy)) {
            continue;
        }
        let inward = (-dx as f32 * OUTLINE_WIDTH / 2.0, -dy as f32 * OUTLINE_WIDTH / 2.0);
        let along = |t: f32| (x + from.0 + (to.0 - from.0) * t + inward.0, y + from.1 + (to.1 - from.1) * t + inward.1);
        for (start, end) in [(0.1, 0.4), (0.6, 0.9)] {
            quad(stroke(along(start), along(end), OUTLINE_WIDTH).map(world), LEAK, 1.0, out);
        }
    }
}

/// An arrow from a point of the grid out along a step, as the mockup's: a shaft from just
/// behind the point to most of a tile out, and a head beyond. `size` scales it.
fn arrow(world: &impl Fn(Point) -> Point, from: Point, step: (i32, i32), size: f32, out: &mut Vec<f32>) {
    let (dx, dy) = (step.0 as f32, step.1 as f32);
    let along = |tiles: f32, side: f32| world((from.0 + (dx * tiles - dy * side) * size, from.1 + (dy * tiles + dx * side) * size));
    let px = |value: f32| value / 22.0;
    let shaft = stroke((from.0 - dx * px(6.0) * size, from.1 - dy * px(6.0) * size), (from.0 + dx * px(18.0) * size, from.1 + dy * px(18.0) * size), px(2.5) * size);
    quad(shaft.map(world), LEAK, 1.0, out);
    let (tip, left, right) = (along(px(24.0), 0.0), along(px(16.0), px(6.0)), along(px(16.0), -px(6.0)));
    quad([tip, left, right, right], LEAK, 1.0, out);
}

/// Each room of a grid open to the outside, the outside itself left out: a dashed orange
/// outline round it, and an arrow out through each gap.
fn leak_quads(grid: &Grid, air: &GridAir, out: &mut Vec<f32>) {
    let world = |point: Point| grid.to_world(point);
    let leaking = |room: usize| air.rooms.rooms.get(room).is_some_and(|room| room.is_open() && !room.is_outside());
    for (&cell, &room) in &air.rooms.room_of {
        if leaking(room) {
            dashed_edges(&world, cell, |next| air.rooms.room_of.get(&next) == Some(&room), out);
        }
    }
    for room in air.rooms.rooms.iter().filter(|room| room.is_open() && !room.is_outside()) {
        for &(cell, step) in &room.ways_out {
            arrow(&world, (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5), step, 1.0, out);
        }
    }
}

/// The overlay's rectangles for every grid, in world tiles, as the renderer takes them. In
/// the Breathable, Pressure and Temperature views: each tile with air in its kind's colour and
/// mark, each floor with none in caution tape, and walls with no air left out. In the
/// Breathable view, each room open to the outside outlined, with arrows out. In the Rooms view,
/// each room in its soft colour, the outside left out, with the same outlines and arrows.
/// `marks` is how strongly marks are drawn ([`mark_strength`]).
pub fn quads(grids: &[Grid], mirror: &AirMirror, view: AirView, marks: f32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut shapes = Marks::new();
    for grid in grids {
        let Some(air) = mirror.grids.get(&grid.uid) else { continue };
        let world = |point: Point| grid.to_world(point);
        if view == AirView::Rooms {
            let room = Look { colour: [0, 0, 0], alpha: ROOM_ALPHA, mark: Mark::None, icon: Icon::Tick };
            for (&cell, &number) in &air.rooms.room_of {
                let Some(found) = air.rooms.rooms.get(number).filter(|found| !found.is_outside()) else { continue };
                let look = Look { colour: ROOM_COLOURS[found.colour % ROOM_COLOURS.len()], ..room };
                tile(&world, cell, &look, 0.0, &mut shapes, &mut out);
            }
            leak_quads(grid, air, &mut out);
            continue;
        }
        let looks: Vec<Look> = air.mixes.iter().map(|mix| air::look(view, mix, &mirror.gases)).collect();
        for (&cell, &index) in &air.tiles {
            let Some(look) = looks.get(index as usize) else { continue };
            tile(&world, cell, look, marks, &mut shapes, &mut out);
        }
        // A floor under an open door with the air the game gives it, as if it stored that.
        for (&cell, mix) in &air.filled {
            tile(&world, cell, &air::look(view, mix, &mirror.gases), marks, &mut shapes, &mut out);
        }
        for &cell in &air.unfilled {
            tile(&world, cell, &NO_AIR, marks, &mut shapes, &mut out);
        }
        if view == AirView::Breathable {
            leak_quads(grid, air, &mut out);
        }
    }
    out
}

/// A label on a room: a small rounded badge at a tile near the room's middle, with an icon
/// in a coloured circle, or none, and plain words.
#[derive(Clone, Debug, PartialEq)]
pub struct AirLabel {
    /// The middle of its tile, on the map.
    pub at: (f32, f32),
    pub icon: Option<Icon>,
    pub text: String,
    /// The colour of its circle and its edge.
    pub colour: [u8; 3],
    /// How many tiles its room has, for [`label_shown`].
    pub tiles: usize,
}

/// How tall a label is on screen, in the interface's pixels, with a little room round it.
const LABEL_HEIGHT: f32 = 22.0;

/// How wide a label is on screen, in the interface's pixels, near enough: its words at the
/// labels' size, its icon and its ends.
pub fn label_width(label: &AirLabel) -> f32 {
    label.text.chars().count() as f32 * 6.3 + if label.icon.is_some() { 34.0 } else { 20.0 }
}

/// Of labels placed on screen by their middles, those that do not cover another, the larger
/// rooms' first. Where two would overlap, the smaller room's is left out.
pub fn uncrowded(mut placed: Vec<((f32, f32), AirLabel)>) -> Vec<((f32, f32), AirLabel)> {
    placed.sort_by(|a, b| b.1.tiles.cmp(&a.1.tiles));
    let mut kept: Vec<((f32, f32), AirLabel)> = Vec::new();
    for (at, label) in placed {
        let width = label_width(&label);
        if kept.iter().all(|(other_at, other)| (other_at.0 - at.0).abs() >= (width + label_width(other)) / 2.0 || (other_at.1 - at.1).abs() >= LABEL_HEIGHT) {
            kept.push((at, label));
        }
    }
    kept
}

/// The tile of a room nearest its middle, so that a label sits inside an odd shape. Of tiles
/// as near, the lowest.
fn middle_tile(cells: &[(i32, i32)]) -> Option<(i32, i32)> {
    let count = cells.len() as f32;
    let mean = cells.iter().fold((0.0, 0.0), |sum, cell| (sum.0 + cell.0 as f32 / count, sum.1 + cell.1 as f32 / count));
    let distance = |cell: &(i32, i32)| (cell.0 as f32 - mean.0).powi(2) + (cell.1 as f32 - mean.1).powi(2);
    cells.iter().copied().min_by(|a, b| distance(a).total_cmp(&distance(b)).then(a.cmp(b)))
}

/// A whole number of degrees, never "-0 °C".
fn degrees(celsius: f32) -> String {
    format!("{:.0} °C", celsius.round() + 0.0)
}

fn kilopascals(pressure: f32) -> String {
    format!("{:.0} kPa", pressure.round() + 0.0)
}

/// What makes a mix another mix, in a few words: its main odd gas, too little or too much
/// oxygen, or its heat. In the order `breathable` tells them.
pub fn other_name(mix: &Mix, gases: &[String]) -> String {
    let total = mix.total_moles();
    let odd = gases.iter().zip(&mix.moles).filter(|(gas, _)| !["Oxygen", "Nitrogen", "WaterVapor"].contains(&gas.as_str())).max_by(|a, b| a.1.total_cmp(b.1));
    if let Some((gas, &moles)) = odd {
        if total > 0.0 && moles / total >= air::OTHER_GAS {
            return gas_name(gas);
        }
    }
    let oxygen = mix.share(gases, "Oxygen");
    if oxygen < air::OXYGEN_LOW {
        "Too little oxygen".to_owned()
    } else if oxygen > air::OXYGEN_HIGH {
        "Too much oxygen".to_owned()
    } else if mix.temperature <= air::FREEZER {
        format!("Too cold, {}", degrees(mix.celsius()))
    } else {
        format!("Too hot, {}", degrees(mix.celsius()))
    }
}

/// The words of a kind of air, with the number that matters.
fn breath_text(breath: Breath, mix: &Mix, gases: &[String]) -> String {
    match breath {
        Breath::Normal => "Normal air".to_owned(),
        Breath::Cold => format!("Cold, {}", degrees(mix.celsius())),
        Breath::Thin => format!("Thin air, {}", kilopascals(mix.pressure())),
        Breath::HighPressure => format!("Too much pressure, {}", kilopascals(mix.pressure())),
        Breath::Vacuum => "Vacuum".to_owned(),
        Breath::Other => other_name(mix, gases),
    }
}

/// A problem a room's air can have, worst first, for which one a room's label tells when
/// two have as many tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Problem {
    Vacuum,
    NoAir,
    Other,
    HighPressure,
    Cold,
    Thin,
}

impl Problem {
    const ALL: [Problem; 6] = [Problem::Vacuum, Problem::NoAir, Problem::Other, Problem::HighPressure, Problem::Cold, Problem::Thin];

    fn of(breath: Breath) -> Option<Problem> {
        match breath {
            Breath::Normal => None,
            Breath::Cold => Some(Problem::Cold),
            Breath::Thin => Some(Problem::Thin),
            Breath::HighPressure => Some(Problem::HighPressure),
            Breath::Vacuum => Some(Problem::Vacuum),
            Breath::Other => Some(Problem::Other),
        }
    }
}

/// The air of each tile of a room: its mix, or none for a floor with no air. Walls are left out.
fn room_air<'a>(air: &'a GridAir, cells: &[(i32, i32)]) -> Vec<Option<&'a Mix>> {
    cells
        .iter()
        .filter_map(|cell| match air.tiles.get(cell).and_then(|&index| air.mixes.get(index as usize)) {
            Some(mix) => Some(Some(mix)),
            None => match air.filled.get(cell) {
                Some(mix) => Some(Some(mix)),
                None => air.unfilled.contains(cell).then_some(None),
            },
        })
        .collect()
}

/// The mix most of the tiles have, the first of those as many. Tiles that store one mix of the
/// grid share it, so a mix is known by where it is.
fn most_common<'a>(mixes: impl Iterator<Item = &'a Mix>) -> Option<&'a Mix> {
    let mut counted: HashMap<*const Mix, (usize, usize, &Mix)> = HashMap::new();
    for (order, mix) in mixes.enumerate() {
        counted.entry(mix as *const Mix).or_insert((0, order, mix)).0 += 1;
    }
    counted.into_values().max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1))).map(|(_, _, mix)| mix)
}

/// The Breathable view's label of a sealed room: its main problem, the one with the most
/// tiles, the worst first of those as many, in words with its number, and how many of its tiles
/// have it if not all. None for a room whose air is all normal.
pub fn breathable_label(air: &GridAir, cells: &[(i32, i32)], gases: &[String]) -> Option<(Icon, String, [u8; 3])> {
    let tiles = room_air(air, cells);
    let breaths: Vec<Option<Breath>> = tiles.iter().map(|mix| mix.map(|mix| breathable(mix, gases))).collect();
    let problem_of = |breath: &Option<Breath>| match breath {
        Some(breath) => Problem::of(*breath),
        None => Some(Problem::NoAir),
    };
    let count = |problem: Problem| breaths.iter().filter(|breath| problem_of(breath) == Some(problem)).count();
    let main = Problem::ALL.into_iter().fold(None, |best: Option<(Problem, usize)>, problem| {
        let found = count(problem);
        if found > 0 && best.is_none_or(|(_, most)| found > most) { Some((problem, found)) } else { best }
    });
    let (problem, found) = main?;
    if problem == Problem::NoAir {
        let text = if found == tiles.len() { "No air stored" } else { "Some floor has no air" };
        return Some((NO_AIR.icon, text.to_owned(), NO_AIR.colour));
    }
    let of_problem = tiles.iter().zip(&breaths).filter(|(_, breath)| problem_of(breath) == Some(problem)).filter_map(|(mix, _)| *mix);
    let mix = most_common(of_problem)?;
    let breath = breathable(mix, gases);
    let mut text = breath_text(breath, mix, gases);
    if found < tiles.len() {
        text.push_str(&format!(" ({found} of {} tiles)", tiles.len()));
    }
    let look = breath.look();
    Some((look.icon, text, look.badge()))
}

/// The labels of a view's rooms: in the Breathable view each room whose air is not normal and
/// each room open to the outside, in the Pressure and Temperature views each sealed room's
/// value, and in the Rooms view each room open to the outside. The outside has none.
pub fn labels(grids: &[Grid], mirror: &AirMirror, view: AirView) -> Vec<AirLabel> {
    let mut labels = Vec::new();
    for grid in grids {
        let Some(air) = mirror.grids.get(&grid.uid) else { continue };
        let mut cells: Vec<Vec<(i32, i32)>> = vec![Vec::new(); air.rooms.rooms.len()];
        for (&cell, &room) in &air.rooms.room_of {
            if let Some(list) = cells.get_mut(room) {
                list.push(cell);
            }
        }
        for (room, cells) in air.rooms.rooms.iter().zip(&mut cells) {
            if room.is_outside() || cells.is_empty() {
                continue;
            }
            cells.sort_unstable();
            let label = if room.is_open() {
                // Leaks are shown in the Breathable and Rooms views only.
                (view == AirView::Breathable || view == AirView::Rooms).then(|| (Some(Icon::Arrow), format!("Leaking to {}", air::outside_name(mirror.map_has_air)), LEAK))
            } else {
                match view {
                    AirView::Breathable => breathable_label(air, cells, &mirror.gases).map(|(icon, text, colour)| (Some(icon), text, colour)),
                    AirView::Pressure | AirView::Temperature => {
                        let mix = most_common(room_air(air, cells).into_iter().flatten());
                        Some(match (mix, view) {
                            (None, _) => (Some(NO_AIR.icon), "No air stored".to_owned(), NO_AIR.colour),
                            (Some(mix), AirView::Pressure) => (None, kilopascals(mix.pressure()), air::PRESSURE_SCALE.colour(mix.pressure())),
                            (Some(mix), _) => (None, degrees(mix.celsius()), air::TEMPERATURE_SCALE.colour(mix.celsius())),
                        })
                    }
                    AirView::Rooms => None,
                }
            };
            let (Some((icon, text, colour)), Some(middle)) = (label, middle_tile(cells)) else { continue };
            labels.push(AirLabel { at: grid.to_world((middle.0 as f32 + 0.5, middle.1 as f32 + 0.5)), icon, text, colour, tiles: room.size });
        }
    }
    labels
}

/// The floor's grey under a legend's sample, as the mockup's.
const FLOOR: [u8; 3] = [74, 77, 85];

/// A legend's sample tile, `size` pixels each way, as premultiplied RGBA: the floor with the
/// kind drawn on it by the same code as the map, its mark in full.
pub fn sample(sample: Sample, size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let world = |point: Point| point;
    let mut shapes = Marks::new();
    let whole = |colour: [u8; 3], inset: f32, out: &mut Vec<f32>| quad([(inset, inset), (1.0 - inset, inset), (1.0 - inset, 1.0 - inset), (inset, 1.0 - inset)], colour, 1.0, out);
    match sample {
        Sample::Floor => {}
        Sample::Breath(breath) => tile(&world, (0, 0), &breath.look(), 1.0, &mut shapes, &mut out),
        Sample::NoAir => tile(&world, (0, 0), &NO_AIR, 1.0, &mut shapes, &mut out),
        Sample::Wall => {
            whole([35, 37, 44], 0.0, &mut out);
            whole([48, 51, 59], 0.1, &mut out);
        }
        Sample::Outside => {
            whole([7, 7, 15], 0.0, &mut out);
            for at in [(0.25, 0.7), (0.7, 0.35), (0.55, 0.85)] {
                quad(square(at, 0.07), [255, 255, 255], 0.6, &mut out);
            }
        }
        Sample::Room(colour) => tile(&world, (0, 0), &Look { colour: ROOM_COLOURS[colour % ROOM_COLOURS.len()], alpha: ROOM_ALPHA, mark: Mark::None, icon: Icon::Tick }, 0.0, &mut shapes, &mut out),
        Sample::Leak => {
            // A tile open on its right, with its arrow out that way, a little smaller to fit.
            dashed_edges(&world, (0, 0), |next| next == (1, 0), &mut out);
            arrow(&world, (0.2, 0.5), (1, 0), 0.75, &mut out);
        }
    }
    paint(&out, size, Some(FLOOR))
}

/// A disc as triangles round its middle, each four corners with two in one place.
fn disc(middle: Point, radius: f32) -> Vec<[Point; 4]> {
    let at = |step: usize| {
        let (sin, cos) = (step as f32 / 24.0 * std::f32::consts::TAU).sin_cos();
        (middle.0 + cos * radius, middle.1 + sin * radius)
    };
    (0..24).map(|step| [middle, at(step), at(step + 1), at(step + 1)]).collect()
}

/// The shapes of an icon in a circle one wide, from its top left corner across and down.
/// Strokes that meet have a square at the joint, so that no gap shows.
fn icon_shapes(icon: Icon) -> Vec<[Point; 4]> {
    let width = 0.12;
    let path = |points: &[Point]| {
        let mut shapes: Vec<[Point; 4]> = points.windows(2).map(|pair| stroke(pair[0], pair[1], width)).collect();
        shapes.extend(points[1..points.len() - 1].iter().map(|&joint| square(joint, width)));
        shapes
    };
    match icon {
        Icon::Tick => path(&[(0.27, 0.52), (0.43, 0.68), (0.74, 0.33)]),
        Icon::Snowflake => (0..3)
            .map(|index| {
                let (sin, cos) = ((30.0 + 60.0 * index as f32).to_radians()).sin_cos();
                stroke((0.5 - cos * 0.3, 0.5 - sin * 0.3), (0.5 + cos * 0.3, 0.5 + sin * 0.3), 0.09)
            })
            .collect(),
        Icon::Bang => vec![stroke((0.5, 0.2), (0.5, 0.56), 0.14), square((0.5, 0.75), 0.14)],
        Icon::Triangle => vec![[(0.5, 0.22), (0.79, 0.73), (0.21, 0.73), (0.21, 0.73)]],
        Icon::Cross => vec![stroke((0.3, 0.3), (0.7, 0.7), width), stroke((0.7, 0.3), (0.3, 0.7), width)],
        Icon::Cloud => {
            let mut shapes = disc((0.37, 0.57), 0.14);
            shapes.extend(disc((0.53, 0.46), 0.18));
            shapes.extend(disc((0.67, 0.59), 0.12));
            shapes.push([(0.37, 0.58), (0.67, 0.58), (0.67, 0.71), (0.37, 0.71)]);
            shapes
        }
        Icon::Question => {
            let mut shapes = path(&[(0.35, 0.38), (0.41, 0.26), (0.59, 0.26), (0.65, 0.37), (0.5, 0.5), (0.5, 0.6)]);
            shapes.push(square((0.5, 0.76), 0.13));
            shapes
        }
        Icon::Arrow => vec![stroke((0.2, 0.5), (0.6, 0.5), width), [(0.81, 0.5), (0.55, 0.28), (0.55, 0.72), (0.55, 0.72)]],
    }
}

/// An icon in its coloured circle, `size` pixels each way, as premultiplied RGBA, clear round
/// the circle.
pub fn icon_picture(icon: Icon, colour: [u8; 3], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    // Down the picture is down the page.
    let flip = |point: Point| (point.0, 1.0 - point.1);
    for shape in disc((0.5, 0.5), 0.5) {
        quad(shape.map(flip), colour, 1.0, &mut out);
    }
    for shape in icon_shapes(icon) {
        quad(shape.map(flip), ICON_INK, 1.0, &mut out);
    }
    paint(&out, size, None)
}

/// The rectangles of a picture one wide at the origin, painted on a colour or on nothing,
/// `size` pixels each way, as premultiplied RGBA, with four by four samples a pixel. A
/// rectangle is the two triangles the renderer draws.
fn paint(quads: &[f32], size: u32, background: Option<[u8; 3]>) -> Vec<u8> {
    let under = background.map_or([0.0; 4], |colour| [colour[0] as f32 / 255.0, colour[1] as f32 / 255.0, colour[2] as f32 / 255.0, 1.0]);
    let mut pixels: Vec<[f32; 4]> = vec![under; (size * size) as usize];
    let inside = |p: Point, a: Point, b: Point, c: Point| {
        let side = |from: Point, to: Point| (to.0 - from.0) * (p.1 - from.1) - (to.1 - from.1) * (p.0 - from.0);
        let (ab, bc, ca) = (side(a, b), side(b, c), side(c, a));
        (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
    };
    for quad in quads.chunks_exact(crate::drawlist::FLOATS_PER_QUAD) {
        let corner = |index: usize| (quad[index * 2], quad[index * 2 + 1]);
        // The renderer's strip: bottom left, bottom right, top left, then top right.
        let (a, b, c, d) = (corner(0), corner(1), corner(3), corner(2));
        let (colour, alpha) = ([quad[8], quad[9], quad[10]], quad[11]);
        for py in 0..size {
            for px in 0..size {
                let mut covered = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let point = ((px as f32 + (sx as f32 + 0.5) / 4.0) / size as f32, 1.0 - (py as f32 + (sy as f32 + 0.5) / 4.0) / size as f32);
                        if inside(point, a, b, c) || inside(point, b, c, d) {
                            covered += 1;
                        }
                    }
                }
                if covered == 0 {
                    continue;
                }
                let strength = alpha * covered as f32 / 16.0;
                let pixel = &mut pixels[(py * size + px) as usize];
                for channel in 0..3 {
                    pixel[channel] = colour[channel] * strength + pixel[channel] * (1.0 - strength);
                }
                pixel[3] = strength + pixel[3] * (1.0 - strength);
            }
        }
    }
    pixels.iter().flat_map(|pixel| pixel.map(|channel| (channel * 255.0).round() as u8)).collect()
}

#[cfg(test)]
#[path = "tests/air_draw.rs"]
mod tests;
