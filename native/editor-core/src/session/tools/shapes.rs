//! The line and circle tools: a shape dragged out from the cell pressed to the cursor's cell,
//! with the cells as the web editor gives them.

use super::tiles::Brush;
use super::*;

/// The fill of a line's cells while it is dragged, the web editor's `rgba(255,200,0,0.25)`.
const LINE_FILL: [f32; 4] = [1.0, 200.0 / 255.0, 0.0, 0.25];

/// The fill of a circle's cells while it is dragged, the web editor's `rgba(0,200,255,0.2)`.
const CIRCLE_FILL: [f32; 4] = [0.0, 200.0 / 255.0, 1.0, 0.2];

/// Which shape a drag makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Line,
    Circle,
}

/// A line or a circle being dragged out on one grid.
pub(in crate::session) struct ShapeDrag {
    shape: Shape,
    grid: i32,
    /// The cell pressed: a line's first cell, or a circle's centre.
    from: (i32, i32),
    /// The cell the cursor is on.
    to: (i32, i32),
    /// None when nothing it can put down was chosen at the press. It sets nothing then.
    brush: Option<Brush>,
}

/// The cells of a line from one cell to another, both ends included. This is the web
/// editor's `bresenham` in `lineTool.ts`, step for step.
pub(super) fn line_cells(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((to.0 - from.0).abs(), (to.1 - from.1).abs());
    let (sx, sy) = (if from.0 < to.0 { 1 } else { -1 }, if from.1 < to.1 { 1 } else { -1 });
    let mut err = dx - dy;
    let mut at = from;
    let mut cells = Vec::with_capacity(dx.max(dy) as usize + 1);
    loop {
        cells.push(at);
        if at == to {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            at.0 += sx;
        }
        if e2 < dx {
            err += dx;
            at.1 += sy;
        }
    }
    cells
}

/// The cells of a filled circle: each cell whose offset from the centre has dx squared plus
/// dy squared at most the radius squared. Radius 0 is the one cell. This is the web editor's
/// `filledCircleTiles` in `circleTool.ts`, in its order.
pub(super) fn disc_cells(centre: (i32, i32), radius: i32) -> Vec<(i32, i32)> {
    if radius <= 0 {
        return vec![centre];
    }
    let mut cells = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                cells.push((centre.0 + dx, centre.1 + dy));
            }
        }
    }
    cells
}

/// The radius of a circle from its centre to the cursor's cell: the distance between the two
/// cells, rounded.
fn radius(centre: (i32, i32), to: (i32, i32)) -> i32 {
    let (dx, dy) = (f64::from(to.0 - centre.0), f64::from(to.1 - centre.1));
    (dx * dx + dy * dy).sqrt().round() as i32
}

impl Shape {
    /// The shape's word in a message.
    fn name(self) -> &'static str {
        match self {
            Shape::Line => "line",
            Shape::Circle => "circle",
        }
    }

    /// The names its edit goes by in the history, of tiles, of entities and of decals.
    fn labels(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Shape::Line => ("Line of tiles", "Line of entities", "Line of decals"),
            Shape::Circle => ("Circle of tiles", "Circle of entities", "Circle of decals"),
        }
    }

    /// The colour its cells are filled with while it is dragged.
    fn fill(self) -> [f32; 4] {
        match self {
            Shape::Line => LINE_FILL,
            Shape::Circle => CIRCLE_FILL,
        }
    }

    /// The style of the outlines around its cells.
    fn style(self) -> OutlineStyle {
        match self {
            Shape::Line => OutlineStyle::Line,
            Shape::Circle => OutlineStyle::Circle,
        }
    }
}

impl ShapeDrag {
    /// The cells it covers now.
    fn cells(&self) -> Vec<(i32, i32)> {
        match self.shape {
            Shape::Line => line_cells(self.from, self.to),
            Shape::Circle => disc_cells(self.from, radius(self.from, self.to)),
        }
    }

    /// Its grid, its cells and the entity it puts down, if it puts one down.
    pub(super) fn entity_cells(&self) -> Option<(i32, Vec<(i32, i32)>, usize)> {
        let Some(Brush::Entity(index)) = &self.brush else { return None };
        Some((self.grid, self.cells(), *index))
    }
}

impl Session {
    /// The line or circle tool, or the eraser in that shape, was pressed: begins its shape at
    /// the cell under it.
    pub(super) fn shape_down(&mut self, world: (f32, f32), shape: Shape) {
        // With nothing it can put down chosen the shape is dragged out all the same, and sets
        // nothing when it is let go.
        let brush = self.brush_for_press();
        if let Some((grid, cell)) = self.cell_at(world) {
            self.drag = Drag::Shape(ShapeDrag { shape, grid, from: cell, to: cell, brush });
        }
    }

    /// A shape was dragged on: its far cell follows the cursor.
    pub(super) fn shape_dragged(&self, world: (f32, f32), drag: ShapeDrag) -> Drag {
        let to = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)).map_or(drag.to, |grid| grid.cell_at(world));
        Drag::Shape(ShapeDrag { to, ..drag })
    }

    /// A shape was let go: puts the brush it was pressed with down on its cells, as one edit.
    pub(super) fn shape_up(&mut self, drag: ShapeDrag) {
        let Some(brush) = &drag.brush else { return };
        let cells = drag.cells();
        self.put_shape(drag.grid, &cells, brush, drag.shape.labels(), drag.shape.name());
    }

    /// The cells of a shape being dragged, filled, for `overlay_quads`.
    pub(super) fn shape_overlay(&self, over: &mut Vec<f32>) {
        let Drag::Shape(drag) = &self.drag else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)) else { return };
        let fill = drag.shape.fill();
        for cell in drag.cells() {
            for corner in grid.cells_outline(cell, cell) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&fill);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
    }

    /// The outline of the cell a shape was pressed on, and of a line's last cell.
    pub(super) fn shape_outlines(&self, drag: &ShapeDrag, outlines: &mut Vec<Outline>) {
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)) else { return };
        let style = drag.shape.style();
        outlines.push(Outline { corners: grid.cells_outline(drag.from, drag.from), style });
        if drag.shape == Shape::Line && drag.to != drag.from {
            outlines.push(Outline { corners: grid.cells_outline(drag.to, drag.to), style });
        }
    }

    /// A label over the middle of the top edge of the cursor's cell: the number of cells of a
    /// line, or "r=N" for a circle.
    pub(super) fn shape_label(&self, drag: &ShapeDrag) -> Option<((f32, f32), String)> {
        let grid = self.doc.as_ref()?.grid(drag.grid)?;
        let text = match drag.shape {
            Shape::Line => line_cells(drag.from, drag.to).len().to_string(),
            Shape::Circle => format!("r={}", radius(drag.from, drag.to)),
        };
        Some((grid.to_world((drag.to.0 as f32 + 0.5, drag.to.1 as f32 + 1.0)), text))
    }
}
