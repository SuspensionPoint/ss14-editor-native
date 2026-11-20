//! Select's shapes past the rectangle, as an art program has them: the ellipse, the lasso,
//! the polygon and Select same (its magic wand). With Tiles on each picks cells of one grid
//! for the area tool's region, which is any set of cells (`area::Region`). With Tiles off each
//! picks the visible entities whose picture's middle is inside it and the decals whose middle
//! is, as the rectangle does. The mode, or Shift and Ctrl, says whether what is picked takes
//! the place of what was selected, adds to it or is taken out of it.
//!
//! A cell is inside a shape when its middle is. A shape too small to hold the middle of any
//! cell takes the cells its corners are in, so a small loop round part of a cell takes that
//! cell. A shape of more than `area::LIMIT` cells selects nothing, and the status line says so.
//!
//! The ellipse and the lasso are drags (`MarqueeDrag`). The polygon outlives a press: each
//! click adds a corner, and a click on the first corner, a second click on the last (a
//! double-click), or Enter closes it. Escape, a right-click or a change of tool drops it
//! (`Marqueeing`). Select same is a click.

use super::area::{FILL, LIMIT};
use super::*;
use crate::toolbox::SelectShape;

/// The most corners a lasso or a polygon keeps.
pub(super) const MOST_POINTS: usize = 10_000;

/// How near the first corner of a polygon a click closes it, in pixels.
const CLOSE_ON_FIRST: f32 = 8.0;

/// How near the last corner of a polygon a click closes it, in pixels: the second click of a
/// double-click.
const CLOSE_ON_LAST: f32 = 4.0;

/// How far the cursor goes before a lasso takes another corner, in pixels.
pub(super) const LASSO_STEP: f32 = 2.0;

/// How many sides the outline of an ellipse is drawn with.
const ELLIPSE_SIDES: usize = 64;

/// A shape covers more cells than a region may hold.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TooMany;

/// Whether a point is inside a polygon, by the even-odd rule. A polygon of fewer than three
/// corners has no inside. The cells of a polygon go by the very same rule (`polygon_cells`).
pub(super) fn inside_polygon(points: &[(f32, f32)], (x, y): (f32, f32)) -> bool {
    if points.len() < 3 {
        return false;
    }
    let (x, y) = (f64::from(x), f64::from(y));
    let mut inside = false;
    for (index, &a) in points.iter().enumerate() {
        let b = points[(index + 1) % points.len()];
        let (a, b) = ((f64::from(a.0), f64::from(a.1)), (f64::from(b.0), f64::from(b.1)));
        if (a.1 > y) != (b.1 > y) && x < crossing(a, b, y) {
            inside = !inside;
        }
    }
    inside
}

/// Where an edge from `a` to `b` crosses the line across at `y`.
fn crossing(a: (f64, f64), b: (f64, f64), y: f64) -> f64 {
    a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1)
}

/// Whether a point is inside the ellipse that fills the box from one corner to the other, edge
/// included. An ellipse with no width or no height has no inside.
pub(super) fn inside_ellipse(a: (f32, f32), b: (f32, f32), (x, y): (f32, f32)) -> bool {
    let (rx, ry) = ((b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (dx, dy) = ((x - cx) / rx, (y - cy) / ry);
    dx * dx + dy * dy <= 1.0
}

/// The cells of a grid whose middles are inside a polygon given in the grid's own coordinates,
/// row by row, or the cells its corners are in when no middle is. More than `limit` is too many.
pub(super) fn polygon_cells(points: &[(f32, f32)], limit: usize) -> Result<Vec<(i32, i32)>, TooMany> {
    let mut cells = Vec::new();
    if points.len() >= 3 {
        let (bottom, top) = points.iter().fold((f32::MAX, f32::MIN), |(low, high), point| (low.min(point.1), high.max(point.1)));
        let points: Vec<(f64, f64)> = points.iter().map(|&(x, y)| (f64::from(x), f64::from(y))).collect();
        let mut crossings = Vec::new();
        for row in (bottom.floor() as i32 - 1)..=(top.floor() as i32) {
            let y = f64::from(row) + 0.5;
            crossings.clear();
            for (index, &a) in points.iter().enumerate() {
                let b = points[(index + 1) % points.len()];
                if (a.1 > y) != (b.1 > y) {
                    crossings.push(crossing(a, b, y));
                }
            }
            crossings.sort_unstable_by(f64::total_cmp);
            // A middle at x is inside when an odd number of crossings lie past it: from one
            // crossing up to the next, the first included.
            for pair in crossings.as_chunks::<2>().0 {
                let (from, to) = ((pair[0] - 0.5).ceil() as i64, (pair[1] - 0.5).ceil() as i64 - 1);
                if to < from {
                    continue;
                }
                if cells.len() as i64 + (to - from + 1) > limit as i64 {
                    return Err(TooMany);
                }
                cells.extend((from..=to).map(|x| (x as i32, row)));
            }
        }
    }
    if cells.is_empty() {
        cells = points.iter().map(|&(x, y)| (x.floor() as i32, y.floor() as i32)).collect();
        cells.sort_unstable();
        cells.dedup();
        if cells.len() > limit {
            return Err(TooMany);
        }
    }
    Ok(cells)
}

/// The cells whose middles are inside the ellipse that fills the box of cells from one to the
/// other, both included, row by row. A box one cell wide or high is all inside. More than
/// `limit` is too many.
pub(super) fn ellipse_cells(from: (i32, i32), to: (i32, i32), limit: usize) -> Result<Vec<(i32, i32)>, TooMany> {
    let (left, right, bottom, top) = (from.0.min(to.0), from.0.max(to.0), from.1.min(to.1), from.1.max(to.1));
    let (a, b) = ((left as f32, bottom as f32), (right as f32 + 1.0, top as f32 + 1.0));
    let (cx, rx) = ((a.0 + b.0) / 2.0, (b.0 - a.0) / 2.0);
    let (cy, ry) = ((a.1 + b.1) / 2.0, (b.1 - a.1) / 2.0);
    let mut cells = Vec::new();
    for row in bottom..=top {
        let dy = (row as f32 + 0.5 - cy) / ry;
        let half = rx * (1.0 - dy * dy).max(0.0).sqrt();
        // The span by the sum, one cell wider each way, and each cell of it tested.
        let from = ((cx - half - 0.5).floor() as i32 - 1).max(left);
        let to = ((cx + half - 0.5).ceil() as i32 + 1).min(right);
        for x in from..=to {
            if inside_ellipse(a, b, (x as f32 + 0.5, row as f32 + 0.5)) {
                if cells.len() >= limit {
                    return Err(TooMany);
                }
                cells.push((x, row));
            }
        }
    }
    Ok(cells)
}

/// The cells that touch a cell side by side, and those that touch them, for as long as each
/// passes `same`, the first included whatever it is. More than `limit` is too many.
pub(super) fn contiguous(start: (i32, i32), same: impl Fn((i32, i32)) -> bool, limit: usize) -> Result<Vec<(i32, i32)>, TooMany> {
    let mut seen: HashSet<(i32, i32)> = HashSet::from([start]);
    let mut pending = vec![start];
    let mut cells = Vec::new();
    while let Some(cell) = pending.pop() {
        if cells.len() >= limit {
            return Err(TooMany);
        }
        cells.push(cell);
        for next in [(cell.0 + 1, cell.1), (cell.0 - 1, cell.1), (cell.0, cell.1 + 1), (cell.0, cell.1 - 1)] {
            if !seen.contains(&next) && same(next) {
                seen.insert(next);
                pending.push(next);
            }
        }
    }
    cells.sort_unstable();
    Ok(cells)
}

/// Whether a click at `point`, in pixels of the view, closes a polygon with these corners on
/// the map: near its first corner, with three or more, or near its last (a double-click).
pub(super) fn closes_polygon(camera: &Camera, view: (u32, u32), points: &[(f32, f32)], point: (f32, f32)) -> bool {
    let near = |corner: (f32, f32), within: f32| {
        let at = camera.view_at(corner, view);
        (at.0 - point.0).hypot(at.1 - point.1) <= within
    };
    (points.len() >= 3 && near(points[0], CLOSE_ON_FIRST)) || points.last().is_some_and(|&last| near(last, CLOSE_ON_LAST))
}

/// The corners of an ellipse filling a box, for its outline.
fn ellipse_points(a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    let (cx, cy, rx, ry) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, (b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
    (0..ELLIPSE_SIDES)
        .map(|side| {
            let turn = side as f32 / ELLIPSE_SIDES as f32 * std::f32::consts::TAU;
            (cx + rx * turn.cos(), cy + ry * turn.sin())
        })
        .collect()
}

/// The look of an outline for what a press of Select does.
pub(super) fn mode_style(mode: SelectMode) -> OutlineStyle {
    match mode {
        SelectMode::Replace => OutlineStyle::Drag,
        SelectMode::Add => OutlineStyle::DragAdd,
        SelectMode::Subtract => OutlineStyle::DragTakeAway,
    }
}

/// The fill of the cells a shape would pick, in the colour of what it does.
fn mode_fill(mode: SelectMode) -> [f32; 4] {
    match mode {
        SelectMode::Replace => FILL,
        SelectMode::Add => [0.25, 1.0, 0.25, 0.25],
        SelectMode::Subtract => [1.0, 0.2, 0.2, 0.3],
    }
}

/// An ellipse or a lasso being dragged out. With Tiles on it picks cells of `grid`.
pub(in crate::session) struct MarqueeDrag {
    shape: SelectShape,
    grid: Option<i32>,
    /// On the map: the corners of an ellipse's box, or the lasso's corners in order.
    points: Vec<(f32, f32)>,
    /// Where the lasso took its last corner, in pixels of the view, from the first move on.
    last: Option<(f32, f32)>,
    mode: SelectMode,
}

/// A polygon being clicked out.
struct Polygon {
    grid: Option<i32>,
    /// Its corners on the map, in order.
    points: Vec<(f32, f32)>,
    mode: SelectMode,
}

/// What Select's shapes keep from one press to the next: a polygon being clicked out.
#[derive(Default)]
pub(in crate::session) struct Marqueeing {
    polygon: Option<Polygon>,
}

impl Marqueeing {
    /// Whether a polygon is being clicked out.
    pub(super) fn is_live(&self) -> bool {
        self.polygon.is_some()
    }

    /// Drops the polygon being clicked out, and says whether there was one.
    pub(super) fn end(&mut self) -> bool {
        self.polygon.take().is_some()
    }
}

/// The outline of a shape of Select being drawn, as points on the map for the interface to
/// join. A closed one is filled faintly. `start` is the polygon's first corner, which closes
/// it when clicked.
#[derive(Clone, Debug, PartialEq)]
pub struct MarqueePath {
    pub points: Vec<(f32, f32)>,
    pub closed: bool,
    pub style: OutlineStyle,
    pub start: Option<(f32, f32)>,
}

/// What a shape covers, on the map.
enum Covered {
    /// The ellipse of the box from one point to the other.
    Ellipse((f32, f32), (f32, f32)),
    Polygon(Vec<(f32, f32)>),
}

impl Session {
    /// The shape Select selects in.
    pub fn select_shape(&self) -> SelectShape {
        self.tooling.toolbox.options.select_shape
    }

    /// Whether Select same takes only what touches the thing clicked.
    pub fn select_contiguous(&self) -> bool {
        self.tooling.toolbox.options.select_contiguous
    }

    /// Select was pressed with the ellipse, the lasso or the polygon, off what a press would
    /// move. `grid` is the grid of the region with Tiles on, and None with it off.
    pub(super) fn marquee_down(&mut self, world: (f32, f32), grid: Option<i32>, shape: SelectShape, mode: SelectMode) {
        let last = None;
        match shape {
            SelectShape::Ellipse => self.drag = Drag::Marquee(MarqueeDrag { shape, grid, points: vec![world, world], last, mode }),
            SelectShape::Lasso => self.drag = Drag::Marquee(MarqueeDrag { shape, grid, points: vec![world], last, mode }),
            SelectShape::Polygon => self.marqueeing.polygon = Some(Polygon { grid, points: vec![world], mode }),
            SelectShape::Rectangle | SelectShape::Same => {}
        }
    }

    /// An ellipse or a lasso was dragged on. The ellipse's far corner follows the cursor. The
    /// lasso takes a corner each time the cursor has gone a little way.
    pub(super) fn marquee_dragged(&self, world: (f32, f32), point: (f32, f32), mut drag: MarqueeDrag) -> Drag {
        match drag.shape {
            SelectShape::Lasso => {
                let far = drag.last.is_none_or(|last| (point.0 - last.0).hypot(point.1 - last.1) >= LASSO_STEP);
                if far && drag.points.len() < MOST_POINTS {
                    drag.points.push(world);
                    drag.last = Some(point);
                }
            }
            _ => drag.points[1] = world,
        }
        Drag::Marquee(drag)
    }

    /// An ellipse or a lasso was let go: it selects what it covers.
    pub(super) fn marquee_up(&mut self, world: (f32, f32), point: (f32, f32), drag: MarqueeDrag) {
        let Drag::Marquee(MarqueeDrag { shape, grid, mut points, mode, .. }) = self.marquee_dragged(world, point, drag) else { return };
        let covered = match shape {
            SelectShape::Ellipse => Covered::Ellipse(points[0], points[1]),
            _ => {
                if points.last() != Some(&world) && points.len() < MOST_POINTS {
                    points.push(world);
                }
                Covered::Polygon(points)
            }
        };
        self.select_covered(grid, &covered, mode);
    }

    /// A press while a polygon is being clicked out, with the primary button. Near its first
    /// corner, with three or more, or near its last, it closes the polygon, which then selects
    /// what it covers. Anywhere else it adds a corner.
    pub(super) fn polygon_click(&mut self, world: (f32, f32), point: (f32, f32), view: (u32, u32)) {
        let camera = self.camera;
        let Some(polygon) = self.marqueeing.polygon.as_mut() else { return };
        if !closes_polygon(&camera, view, &polygon.points, point) {
            if polygon.points.len() < MOST_POINTS {
                polygon.points.push(world);
            }
            return;
        }
        self.close_polygon();
    }

    /// Enter: closes a polygon being clicked out, which selects what it covers, or for Paint
    /// and Erase puts down or takes away what it covers (`trace.rs`). Says whether there was
    /// one.
    pub fn enter_key(&mut self) -> bool {
        if self.close_trace() {
            return true;
        }
        if !self.marqueeing.is_live() || !matches!(self.tool, Tool::Area | Tool::Select) {
            return false;
        }
        self.close_polygon();
        true
    }

    /// Closes the polygon being clicked out, which selects what it covers.
    fn close_polygon(&mut self) {
        let Some(Polygon { grid, points, mode }) = self.marqueeing.polygon.take() else { return };
        self.select_covered(grid, &Covered::Polygon(points), mode);
    }

    /// Selects what a shape covers: the cells of `grid` with Tiles on, and else the entities
    /// and decals. Too many cells select nothing, and the status line says so.
    fn select_covered(&mut self, grid: Option<i32>, covered: &Covered, mode: SelectMode) {
        match grid {
            Some(grid) => match self.covered_cells(grid, covered) {
                Some(Ok(cells)) => self.apply_region(grid, cells, mode),
                Some(Err(TooMany)) => self.too_many(),
                None => {}
            },
            None => {
                let (found, decals) = match covered {
                    Covered::Ellipse(a, b) => (self.entities_where(|at| inside_ellipse(*a, *b, at)), self.decals_where(|at| inside_ellipse(*a, *b, at))),
                    Covered::Polygon(points) => (self.entities_where(|at| inside_polygon(points, at)), self.decals_where(|at| inside_polygon(points, at))),
                };
                self.apply_selection(found, decals, mode);
            }
        }
    }

    /// The status line says a shape covered too many cells.
    fn too_many(&mut self) {
        self.set_status(format!("Not selected: the shape covers more than {} tiles", thousands(LIMIT)));
    }

    /// The cells of a grid a shape covers. An ellipse goes by whole cells, from the cell of one
    /// corner to the cell of the other, as a box does. None with no such grid.
    fn covered_cells(&self, grid: i32, covered: &Covered) -> Option<Result<Vec<(i32, i32)>, TooMany>> {
        let grid = self.doc.as_ref()?.grid(grid)?;
        Some(match covered {
            Covered::Ellipse(a, b) => ellipse_cells(grid.cell_at(*a), grid.cell_at(*b), LIMIT),
            Covered::Polygon(points) => polygon_cells(&points.iter().map(|&point| grid.to_local(point)).collect::<Vec<_>>(), LIMIT),
        })
    }

    /// Select same was clicked with Tiles on: the cells of the tile under the cursor, those
    /// that touch it side by side with Contiguous on, or every one on its grid with it off.
    pub(super) fn select_same_tiles(&mut self, world: (f32, f32), mode: SelectMode) {
        let Some((grid_uid, cell)) = self.cell_at(world) else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(grid) = doc.grid(grid_uid) else { return };
        let kind = |cell: &(i32, i32)| grid.tiles.get(cell).map(|tile| tile.id).filter(|&id| doc.tile_names.get(id as usize).is_some_and(|name| name != SPACE));
        let Some(id) = kind(&cell) else {
            self.set_status("Click a tile to select the tiles like it");
            return;
        };
        let cells = if self.select_contiguous() {
            contiguous(cell, |next| kind(&next) == Some(id), LIMIT)
        } else {
            let cells: Vec<(i32, i32)> = grid.tiles.keys().filter(|cell| kind(cell) == Some(id)).copied().collect();
            if cells.len() > LIMIT { Err(TooMany) } else { Ok(cells) }
        };
        match cells {
            Ok(cells) => self.apply_region(grid_uid, cells, mode),
            Err(TooMany) => self.too_many(),
        }
    }

    /// Select same was clicked with Tiles off: every visible entity of the prototype of the
    /// topmost one under the cursor with its parent, or where none is, every decal of the
    /// topmost decal's prototype on its grid. With Contiguous on only those on cells that
    /// touch its cell side by side through cells holding one. The select targets and the
    /// hidden layers are kept to.
    pub(super) fn select_same_things(&mut self, world: (f32, f32), mode: SelectMode) {
        let targets = self.select_targets();
        let entity = self.entities_at(world).first().copied().filter(|_| targets.entities);
        let decal = if entity.is_none() && targets.decals { self.decals_at(world).first().copied() } else { None };
        let (found, decals) = match (entity, decal) {
            (Some(uid), _) => (self.same_entities(uid), Vec::new()),
            (None, Some(key)) => (Vec::new(), self.same_decals(key)),
            (None, None) => return,
        };
        self.apply_selection(found, decals, mode);
    }

    /// The visible entities of an entity's prototype with its parent, inside nothing, and
    /// with Contiguous on only those touching it cell by cell, in the order of their uids.
    fn same_entities(&self, uid: i32) -> Vec<i32> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let Some(first) = doc.entities.get(&uid) else { return Vec::new() };
        let cell = |entity: &Entity| (entity.x.floor() as i32, entity.y.floor() as i32);
        let alike: Vec<&Entity> = doc
            .entities
            .values()
            .filter(|entity| entity.parent == first.parent && !entity.contained && entity.prototype == first.prototype && (entity.uid == uid || self.entity_shown(entity.uid)))
            .collect();
        let mut found: Vec<i32> = if self.select_contiguous() {
            let held: HashSet<(i32, i32)> = alike.iter().map(|entity| cell(entity)).collect();
            let reached: HashSet<(i32, i32)> = contiguous(cell(first), |next| held.contains(&next), usize::MAX).unwrap_or_default().into_iter().collect();
            alike.iter().filter(|entity| reached.contains(&cell(entity))).map(|entity| entity.uid).collect()
        } else {
            alike.iter().map(|entity| entity.uid).collect()
        };
        found.sort_unstable();
        found
    }

    /// The decals of a decal's prototype on its grid, and with Contiguous on only those
    /// touching it cell by cell, in the order of their ids.
    fn same_decals(&self, (grid, id): (i32, i32)) -> Vec<(i32, i32)> {
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid)) else { return Vec::new() };
        let Some(first) = grid.decals.values().find(|decal| decal.id == id) else { return Vec::new() };
        let cell = |decal: &crate::model::Decal| (decal.x.floor() as i32, decal.y.floor() as i32);
        let alike: Vec<&crate::model::Decal> = grid.decals.values().filter(|decal| decal.prototype == first.prototype).collect();
        let reached: Option<HashSet<(i32, i32)>> = self.select_contiguous().then(|| {
            let held: HashSet<(i32, i32)> = alike.iter().map(|decal| cell(decal)).collect();
            contiguous(cell(first), |next| held.contains(&next), usize::MAX).unwrap_or_default().into_iter().collect()
        });
        let mut found: Vec<(i32, i32)> = alike.iter().filter(|decal| reached.as_ref().is_none_or(|reached| reached.contains(&cell(decal)))).map(|decal| (grid.uid, decal.id)).collect();
        found.sort_unstable();
        found
    }

    /// The shape being drawn, the grid of its region and what a press does: a live ellipse or
    /// lasso, or the polygon with the cursor as its next corner.
    fn drawn_shape(&self) -> Option<(Covered, Option<i32>, SelectMode)> {
        if let Drag::Marquee(drag) = &self.drag {
            let covered = match drag.shape {
                SelectShape::Ellipse => Covered::Ellipse(drag.points[0], drag.points[1]),
                _ => Covered::Polygon(drag.points.clone()),
            };
            return Some((covered, drag.grid, drag.mode));
        }
        let polygon = self.marqueeing.polygon.as_ref()?;
        let mut points = polygon.points.clone();
        points.extend(self.hover);
        Some((Covered::Polygon(points), polygon.grid, polygon.mode))
    }

    /// The outline of the shape of Select being drawn. An ellipse of a region fills the box of
    /// cells it goes by. A polygon is open, from its first corner to the cursor.
    /// A lasso or polygon of Paint or Erase gives its own (`trace_path`).
    pub fn marquee_path(&self) -> Option<MarqueePath> {
        let Some((covered, grid, mode)) = self.drawn_shape() else { return self.trace_path() };
        let style = mode_style(mode);
        let polygon = self.marqueeing.polygon.as_ref().filter(|_| !matches!(self.drag, Drag::Marquee(_)));
        Some(match covered {
            Covered::Ellipse(a, b) => {
                let points = match grid.and_then(|grid| self.doc.as_ref()?.grid(grid)) {
                    Some(grid) => {
                        let (from, to) = (grid.cell_at(a), grid.cell_at(b));
                        let low = (from.0.min(to.0) as f32, from.1.min(to.1) as f32);
                        let high = (from.0.max(to.0) as f32 + 1.0, from.1.max(to.1) as f32 + 1.0);
                        ellipse_points(low, high).into_iter().map(|point| grid.to_world(point)).collect()
                    }
                    None => ellipse_points(a, b),
                };
                MarqueePath { points, closed: true, style, start: None }
            }
            Covered::Polygon(points) => MarqueePath { start: polygon.map(|polygon| polygon.points[0]), closed: polygon.is_none(), points, style },
        })
    }

    /// The cells the shape being drawn would pick for a region, filled in the colour of what
    /// it does. Nothing for a shape of too many cells.
    pub(super) fn marquee_overlay(&self, over: &mut Vec<f32>) {
        let Some((covered, Some(grid_uid), mode)) = self.drawn_shape() else { return };
        let Some(Ok(cells)) = self.covered_cells(grid_uid, &covered) else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid_uid)) else { return };
        for cell in cells {
            for corner in grid.cells_outline(cell, cell) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&mode_fill(mode));
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
    }

    /// How many cells the shape being drawn would pick for a region, as "N tiles", over the
    /// middle of the top of the box around it.
    pub(super) fn marquee_label(&self) -> Option<((f32, f32), String)> {
        let (covered, Some(grid), _) = self.drawn_shape()? else { return None };
        let points = match &covered {
            Covered::Ellipse(a, b) => vec![*a, *b],
            Covered::Polygon(points) => points.clone(),
        };
        let (left, right, top) = points.iter().fold((f32::MAX, f32::MIN, f32::MIN), |(l, r, t), point| (l.min(point.0), r.max(point.0), t.max(point.1)));
        let text = match self.covered_cells(grid, &covered)? {
            Ok(cells) if cells.len() == 1 => "1 tile".to_owned(),
            Ok(cells) => format!("{} tiles", thousands(cells.len())),
            Err(TooMany) => format!("Over {} tiles", thousands(LIMIT)),
        };
        Some((((left + right) / 2.0, top), text))
    }
}
