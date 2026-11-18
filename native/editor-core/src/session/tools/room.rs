//! The Room tool: a rectangle dragged out on one grid that becomes a room in one edit. The
//! floor tile in hand goes on the cells inside, the tile laid under walls (`floor_tile`) on
//! the cells of its edge, and the Room tool's wall (`room_wall`) on every cell of the edge
//! that does not hold that wall already. Such a cell is skipped whole. Doors are left to the
//! mapper. A room of more than `SHAPE_LIMIT` cells is refused, as a shape of entities is.

use super::tiles::{cells_holding, rectangle_cells, SHAPE_LIMIT};
use super::*;

/// How faint the ghost of a room's walls is.
const WALL_GHOST: f32 = 0.5;

/// A room being dragged out on one grid.
pub(in crate::session) struct RoomDrag {
    grid: i32,
    from: (i32, i32),
    to: (i32, i32),
    /// The floor tile in hand at the press. None when no tile was in hand. It builds nothing then.
    floor: Option<String>,
}

/// The cells of a room from one corner to the other: those inside, and those of its edge.
pub(super) fn room_cells(from: (i32, i32), to: (i32, i32)) -> (Vec<(i32, i32)>, Vec<(i32, i32)>) {
    let (left, right) = (from.0.min(to.0), from.0.max(to.0));
    let (bottom, top) = (from.1.min(to.1), from.1.max(to.1));
    rectangle_cells(from, to).into_iter().partition(|&(x, y)| x != left && x != right && y != bottom && y != top)
}

impl Session {
    /// The Room tool was pressed: begins a room at the cell under it, with the tile in hand
    /// for its floor. With no tile in hand the room is dragged out all the same, a hint asks
    /// for a tile, and it builds nothing when it is let go.
    pub(super) fn room_down(&mut self, world: (f32, f32)) {
        let floor = self.chosen_tile().map(str::to_owned);
        if floor.is_none() {
            self.hint = Some(Hint::Tile);
        }
        if let Some(index) = self.room_wall() {
            self.want_previews(&[index]);
        }
        if let Some((grid, cell)) = self.cell_at(world) {
            self.drag = Drag::Room(RoomDrag { grid, from: cell, to: cell, floor });
        }
    }

    /// A room was dragged on: its far corner follows the cursor.
    pub(super) fn room_dragged(&self, world: (f32, f32), drag: RoomDrag) -> Drag {
        let to = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)).map_or(drag.to, |grid| grid.cell_at(world));
        Drag::Room(RoomDrag { to, ..drag })
    }

    /// A room was let go: builds it, as one edit.
    pub(super) fn room_up(&mut self, drag: RoomDrag) {
        let RoomDrag { grid, from, to, floor } = drag;
        if let Some(floor) = floor {
            self.build_room(grid, from, to, &floor);
        }
    }

    /// A room being dragged out is dropped and builds nothing, as by Escape or a press of the
    /// secondary button while it is dragged. Returns whether there was one.
    pub fn cancel_room(&mut self) -> bool {
        let dropped = matches!(self.drag, Drag::Room(_));
        if dropped {
            self.drag = Drag::None;
        }
        dropped
    }

    /// Builds a room on a grid from one corner to the other, as one edit with no stroke: the
    /// tiles first, so that no wall goes on a cell with no tile, then the walls. A room of more
    /// than `SHAPE_LIMIT` cells, or with no wall to build with, is refused with a line on the
    /// status bar.
    fn build_room(&mut self, grid_uid: i32, from: (i32, i32), to: (i32, i32), floor: &str) {
        let size = (((from.0 - to.0).abs() + 1) as usize) * (((from.1 - to.1).abs() + 1) as usize);
        if size > SHAPE_LIMIT {
            self.set_status(format!("Not built: the room has more than {} cells", thousands(SHAPE_LIMIT)));
            return;
        }
        let Some(wall) = self.room_wall().map(|index| self.prototypes[index].id.clone()) else {
            self.set_status("Not built: choose a wall for the room in the Room panel");
            return;
        };
        let edge_tile = self.floor_tile().to_owned();
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(grid) = doc.grids.iter().find(|grid| grid.uid == grid_uid) else { return };
        doc.active_grid = grid_uid;
        self.grid = (grid.uid, grid.x, grid.y);

        let (inside, edge) = room_cells(from, to);
        let holding = cells_holding(doc, grid_uid, &wall);
        let edge: Vec<(i32, i32)> = edge.into_iter().filter(|cell| !holding.contains(cell)).collect();
        let mut ops = Vec::new();
        let mut laid: Vec<([(f32, f32); 4], &str)> = Vec::new();
        for (cells, tile) in [(&inside, floor), (&edge, edge_tile.as_str())] {
            for &cell in cells {
                if doc.tile_at(grid_uid, cell) != tile {
                    ops.push(json!({ "kind": "tile", "grid": grid_uid, "x": cell.0, "y": cell.1, "tile": tile }));
                    laid.push((grid.cells_outline(cell, cell), tile));
                }
            }
        }
        for &(x, y) in &edge {
            ops.push(json!({ "kind": "spawn", "prototype": wall, "parent": grid_uid, "x": x as f32 + 0.5, "y": y as f32 + 0.5, "rotation": 0.0 }));
        }
        let laid: Vec<([(f32, f32); 4], String)> = laid.into_iter().map(|(corners, tile)| (corners, tile.to_owned())).collect();
        if ops.is_empty() {
            self.set_status("Nothing to build: the room is there already");
            return;
        }
        let seq = self.edit(ops, "Build room", None, false);
        for (corners, tile) in laid {
            let sprite = self.tile_sprites.get(&tile).cloned().flatten();
            self.painting.lay_ahead(seq, vec![corners], sprite);
        }
    }

    /// The ghost of a room being dragged out, for `overlay_quads`: its floor and edge tiles
    /// faint over the map (`under`), and the wall's picture faint on each cell of its edge
    /// (`over`) once the engine has sent it. A room too large to build shows its outline alone.
    pub(super) fn room_overlay(&mut self, under: &mut Vec<f32>, over: &mut Vec<f32>) {
        let Drag::Room(drag) = &self.drag else { return };
        let (grid, from, to, floor) = (drag.grid, drag.from, drag.to, drag.floor.clone());
        let size = (((from.0 - to.0).abs() + 1) as usize) * (((from.1 - to.1).abs() + 1) as usize);
        if size > SHAPE_LIMIT {
            return;
        }
        let (inside, edge) = room_cells(from, to);
        if let Some(floor) = floor {
            self.faint_tiles(grid, &inside, &floor, under);
        }
        let edge_tile = self.floor_tile().to_owned();
        self.faint_tiles(grid, &edge, &edge_tile, under);
        let Some(wall) = self.room_wall() else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid)) else { return };
        let Some(quads) = self.preview(wall, 0) else { return };
        let (sin, cos) = grid.rotation.sin_cos();
        for &cell in &edge {
            let at = grid.to_world((cell.0 as f32 + 0.5, cell.1 as f32 + 0.5));
            for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
                for corner in quad[..8].chunks_exact(2) {
                    over.extend_from_slice(&[at.0 + corner[0] * cos - corner[1] * sin, at.1 + corner[0] * sin + corner[1] * cos]);
                }
                over.extend_from_slice(&[quad[8], quad[9], quad[10], quad[11] * WALL_GHOST]);
                over.extend_from_slice(&quad[12..]);
            }
        }
    }

    /// The outline of a room being dragged out.
    pub(super) fn room_outlines(&self, drag: &RoomDrag, outlines: &mut Vec<Outline>) {
        if let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)) {
            outlines.push(Outline { corners: grid.cells_outline(drag.from, drag.to), style: OutlineStyle::Drag });
        }
    }

    /// The size of a room being dragged out, as "WxH" over the middle of its top edge.
    pub(super) fn room_label(&self, drag: &RoomDrag) -> Option<((f32, f32), String)> {
        let grid = self.doc.as_ref()?.grid(drag.grid)?;
        let (width, height) = ((drag.from.0 - drag.to.0).abs() + 1, (drag.from.1 - drag.to.1).abs() + 1);
        let (left, top) = (drag.from.0.min(drag.to.0) as f32, drag.from.1.max(drag.to.1) as f32 + 1.0);
        Some((grid.to_world((left + width as f32 / 2.0, top)), format!("{width}x{height}")))
    }
}
