//! The lasso and the polygon of Paint, Erase and the Room tool: a shape traced on the map
//! whose cells are the very cells Select's lasso and polygon take (`marquee::polygon_cells`),
//! those whose middle is inside it, or the cells its corners are in when it is too small to
//! hold a middle. It is traced on the grid under the press.
//!
//! The lasso is a drag (`TraceDrag`) that takes a corner each time the cursor has gone a little
//! way, and closes when the button comes up. The polygon outlives a press (`Tracing`): each
//! click adds a corner, and a click on the first corner, a second click on the last (a
//! double-click), or Enter closes it. Escape, a right-click or a change of tool drops it.
//!
//! Closed, a shape of Paint or Erase puts the brush of its first press down on its cells, or
//! with Outline on its edge alone (`shapes::edge_cells`), as one edit through `put_shape`, so
//! it skips and refuses as the other shapes do. A shape of more than `FILL_LIMIT` cells is
//! refused whatever it puts down. A shape of the Room tool builds a room of its cells
//! (`room.rs`). While it is traced its outline is drawn as Select's is (`trace_path`), and the
//! cells it would work on are filled (`trace_overlay`), or for a room the room's ghost is drawn.

use super::marquee::{closes_polygon, polygon_cells, TooMany, LASSO_STEP, MOST_POINTS};
use super::shapes::edge_cells;
use super::tiles::{Brush, FILL_LIMIT, SHAPE_LIMIT};
use super::*;

/// The fill of the cells a shape of Paint would put something down on.
const PAINT_FILL: [f32; 4] = [0.25, 1.0, 0.25, 0.25];

/// The fill of the cells a shape of Erase would take things from.
const ERASE_FILL: [f32; 4] = [1.0, 0.27, 0.27, 0.3];

/// Which shape is traced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trace {
    Lasso,
    Polygon,
}

impl Trace {
    /// The shape's word in a message.
    fn name(self) -> &'static str {
        match self {
            Trace::Lasso => "lasso",
            Trace::Polygon => "polygon",
        }
    }

    /// The names its edit goes by in the history, of tiles, of entities and of decals.
    fn labels(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Trace::Lasso => ("Lasso of tiles", "Lasso of entities", "Lasso of decals"),
            Trace::Polygon => ("Polygon of tiles", "Polygon of entities", "Polygon of decals"),
        }
    }
}

/// What a traced shape does when it is closed, read at its first press.
pub(in crate::session) enum Purpose {
    /// Paint or Erase: puts the brush down on its cells, or on its edge alone with `outline`.
    /// None when nothing it can put down was chosen. It does nothing then.
    Brush { brush: Option<Brush>, outline: bool },
    /// The Room tool: builds a room with this floor inside. None when no tile was in hand.
    Room { floor: Option<String> },
}

/// A room being drawn: its grid, its cells (too many past `SHAPE_LIMIT`) and the floor in
/// hand at its press.
pub(super) type DrawnRoom = (i32, Result<Vec<(i32, i32)>, TooMany>, Option<String>);

/// A shape being traced, as it is drawn now.
struct Drawn<'a> {
    grid: i32,
    /// Its corners on the map, with the cursor as a polygon's next corner.
    points: Vec<(f32, f32)>,
    purpose: &'a Purpose,
    /// Whether it is a polygon, which is open while it is clicked out.
    polygon: bool,
}

/// A lasso being dragged out on one grid.
pub(in crate::session) struct TraceDrag {
    grid: i32,
    /// Its corners on the map, in order.
    points: Vec<(f32, f32)>,
    /// Where it took its last corner, in pixels of the view, from the first move on.
    last: Option<(f32, f32)>,
    purpose: Purpose,
}

/// A polygon being clicked out on one grid.
struct TracePolygon {
    grid: i32,
    /// Its corners on the map, in order.
    points: Vec<(f32, f32)>,
    purpose: Purpose,
}

/// What the traced shapes keep from one press to the next: a polygon being clicked out.
#[derive(Default)]
pub(in crate::session) struct Tracing {
    polygon: Option<TracePolygon>,
}

impl Tracing {
    /// Whether a polygon is being clicked out.
    pub(super) fn is_live(&self) -> bool {
        self.polygon.is_some()
    }

    /// Drops the polygon being clicked out, and says whether there was one.
    pub(super) fn end(&mut self) -> bool {
        self.polygon.take().is_some()
    }

    /// Whether a polygon of the Room tool is being clicked out.
    fn is_room(&self) -> bool {
        matches!(self.polygon, Some(TracePolygon { purpose: Purpose::Room { .. }, .. }))
    }
}

impl Session {
    /// Paint in a lasso or a polygon, or Erase in one, was pressed: begins it with the brush
    /// and Fill or Outline as they are now.
    pub(super) fn brush_trace_down(&mut self, world: (f32, f32), trace: Trace) {
        let purpose = Purpose::Brush { brush: self.brush_for_press(), outline: self.press_outline() };
        self.trace_down(world, trace, purpose);
    }

    /// Begins a lasso or a polygon on the grid under the press. Off every grid nothing begins.
    pub(super) fn trace_down(&mut self, world: (f32, f32), trace: Trace, purpose: Purpose) {
        let Some((grid, _)) = self.cell_at(world) else { return };
        match trace {
            Trace::Lasso => self.drag = Drag::Trace(TraceDrag { grid, points: vec![world], last: None, purpose }),
            Trace::Polygon => self.tracing.polygon = Some(TracePolygon { grid, points: vec![world], purpose }),
        }
    }

    /// A lasso was dragged on: it takes a corner each time the cursor has gone a little way.
    pub(super) fn trace_dragged(&self, world: (f32, f32), point: (f32, f32), mut drag: TraceDrag) -> Drag {
        let far = drag.last.is_none_or(|last| (point.0 - last.0).hypot(point.1 - last.1) >= LASSO_STEP);
        if far && drag.points.len() < MOST_POINTS {
            drag.points.push(world);
            drag.last = Some(point);
        }
        Drag::Trace(drag)
    }

    /// A lasso was let go: it closes, and works on what it covers.
    pub(super) fn trace_up(&mut self, world: (f32, f32), point: (f32, f32), drag: TraceDrag) {
        let Drag::Trace(TraceDrag { grid, mut points, purpose, .. }) = self.trace_dragged(world, point, drag) else { return };
        if points.last() != Some(&world) && points.len() < MOST_POINTS {
            points.push(world);
        }
        self.finish_trace(grid, &points, purpose, Trace::Lasso);
    }

    /// A press while a polygon is being clicked out. Near its first corner, with three or
    /// more, or near its last, it closes the polygon. Anywhere else it adds a corner.
    pub(super) fn trace_click(&mut self, world: (f32, f32), point: (f32, f32), view: (u32, u32)) {
        let camera = self.camera;
        let Some(polygon) = self.tracing.polygon.as_mut() else { return };
        if closes_polygon(&camera, view, &polygon.points, point) {
            self.close_trace();
        } else if polygon.points.len() < MOST_POINTS {
            polygon.points.push(world);
        }
    }

    /// Closes the polygon being clicked out, which works on what it covers. Says whether there
    /// was one.
    pub(super) fn close_trace(&mut self) -> bool {
        let Some(TracePolygon { grid, points, purpose }) = self.tracing.polygon.take() else { return false };
        self.finish_trace(grid, &points, purpose, Trace::Polygon);
        true
    }

    /// Drops a lasso or polygon of the Room tool, and says whether there was one.
    pub(super) fn cancel_room_trace(&mut self) -> bool {
        let dragged = matches!(self.drag, Drag::Trace(TraceDrag { purpose: Purpose::Room { .. }, .. }));
        if dragged {
            self.drag = Drag::None;
        }
        let clicked = self.tracing.is_room() && self.tracing.end();
        dragged || clicked
    }

    /// The cells of a grid whose middles are inside a shape traced on the map, or the cells
    /// its corners are in. None with no such grid.
    fn traced_cells(&self, grid: i32, points: &[(f32, f32)], limit: usize) -> Option<Result<Vec<(i32, i32)>, TooMany>> {
        let grid = self.doc.as_ref()?.grid(grid)?;
        Some(polygon_cells(&points.iter().map(|&point| grid.to_local(point)).collect::<Vec<_>>(), limit))
    }

    /// The cells a shape of Paint or Erase works on: all of them, or its edge with `outline`.
    fn brush_cells(&self, grid: i32, points: &[(f32, f32)], outline: bool) -> Option<Result<Vec<(i32, i32)>, TooMany>> {
        Some(self.traced_cells(grid, points, FILL_LIMIT)?.map(|cells| if outline { edge_cells(&cells) } else { cells }))
    }

    /// A traced shape was closed: Paint or Erase works on its cells as one edit, and the Room
    /// tool builds a room of them.
    fn finish_trace(&mut self, grid: i32, points: &[(f32, f32)], purpose: Purpose, trace: Trace) {
        match purpose {
            Purpose::Brush { brush: None, .. } | Purpose::Room { floor: None } => {}
            Purpose::Brush { brush: Some(brush), outline } => match self.brush_cells(grid, points, outline) {
                Some(Ok(cells)) => self.put_shape(grid, &cells, &brush, trace.labels(), trace.name()),
                Some(Err(TooMany)) => {
                    let not = if matches!(brush, Brush::Erase(_)) { "Not erased" } else { "Not painted" };
                    self.set_status(format!("{not}: the {} covers more than {} cells", trace.name(), thousands(FILL_LIMIT)));
                }
                None => {}
            },
            Purpose::Room { floor: Some(floor) } => match self.traced_cells(grid, points, SHAPE_LIMIT) {
                Some(Ok(cells)) => self.build_room(grid, &cells, &floor, false),
                Some(Err(TooMany)) => self.set_status(format!("Not built: the room has more than {} cells", thousands(SHAPE_LIMIT))),
                None => {}
            },
        }
    }

    /// The shape being traced: a live lasso, or the polygon with the cursor as its next corner.
    fn drawn_trace(&self) -> Option<Drawn<'_>> {
        if let Drag::Trace(drag) = &self.drag {
            return Some(Drawn { grid: drag.grid, points: drag.points.clone(), purpose: &drag.purpose, polygon: false });
        }
        let polygon = self.tracing.polygon.as_ref()?;
        let mut points = polygon.points.clone();
        points.extend(self.hover);
        Some(Drawn { grid: polygon.grid, points, purpose: &polygon.purpose, polygon: true })
    }

    /// The room being traced with the Room tool.
    pub(super) fn traced_room(&self) -> Option<DrawnRoom> {
        let Drawn { grid, points, purpose, .. } = self.drawn_trace()?;
        let Purpose::Room { floor } = purpose else { return None };
        Some((grid, self.traced_cells(grid, &points, SHAPE_LIMIT)?, floor.clone()))
    }

    /// The outline of the shape being traced, as Select's is drawn. A lasso is closed and a
    /// polygon open, from its first corner to the cursor. Erase's is red.
    pub(super) fn trace_path(&self) -> Option<MarqueePath> {
        let Drawn { points, purpose, polygon, .. } = self.drawn_trace()?;
        let style = match purpose {
            Purpose::Brush { brush: Some(Brush::Erase(_)), .. } => OutlineStyle::DragTakeAway,
            _ => OutlineStyle::Drag,
        };
        let start = polygon.then(|| points[0]);
        Some(MarqueePath { points, closed: !polygon, style, start })
    }

    /// The cells a shape of Paint or Erase being traced would work on, filled, for
    /// `overlay_quads`: green for Paint and red for Erase. Nothing for a shape of too many cells.
    pub(super) fn trace_overlay(&self, over: &mut Vec<f32>) {
        let Some(Drawn { grid: grid_uid, points, purpose: Purpose::Brush { brush, outline }, .. }) = self.drawn_trace() else { return };
        let Some(Ok(cells)) = self.brush_cells(grid_uid, &points, *outline) else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid_uid)) else { return };
        let fill = if matches!(brush, Some(Brush::Erase(_))) { ERASE_FILL } else { PAINT_FILL };
        for cell in cells {
            for corner in grid.cells_outline(cell, cell) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&fill);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
    }

    /// How many cells the shape being traced would work on, as "N cells", over the middle of
    /// the top of the box around it.
    pub(super) fn trace_label(&self) -> Option<((f32, f32), String)> {
        let Drawn { grid, points, purpose, .. } = self.drawn_trace()?;
        let (cells, limit) = match purpose {
            Purpose::Brush { outline, .. } => (self.brush_cells(grid, &points, *outline)?, FILL_LIMIT),
            Purpose::Room { .. } => (self.traced_cells(grid, &points, SHAPE_LIMIT)?, SHAPE_LIMIT),
        };
        let (left, right, top) = points.iter().fold((f32::MAX, f32::MIN, f32::MIN), |(l, r, t), point| (l.min(point.0), r.max(point.0), t.max(point.1)));
        let text = match cells {
            Ok(cells) if cells.len() == 1 => "1 cell".to_owned(),
            Ok(cells) => format!("{} cells", thousands(cells.len())),
            Err(TooMany) => format!("Over {} cells", thousands(limit)),
        };
        Some((((left + right) / 2.0, top), text))
    }
}
