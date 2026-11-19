//! The Room tool: a rectangle dragged out on one grid that becomes a room in one edit. The
//! floor tile in hand goes on the cells inside, the tile laid under walls (`floor_tile`) on
//! the cells of its edge, and the Room tool's wall (`room_wall`) on the cells of its edge.
//! Doors are left to the mapper. A room of more than `SHAPE_LIMIT` cells is refused, as a
//! shape of entities is.
//!
//! What it does where walls stand already (any wall, as `Prototype::is_wall` tells) depends
//! on its Intersection switch:
//!
//! - Off, rooms stay separate. An edge cell that holds a wall is skipped whole, so no wall is
//!   stacked on another, and a cell inside that holds a wall keeps it and its tile. No wall is
//!   ever taken away.
//! - On, the new room joins the rooms it overlaps into one. The walls on cells inside are
//!   taken away and the floor laid there. An edge cell that holds a wall keeps it. An edge
//!   cell with no wall whose tile is a floor other than the edge tile lies inside a room
//!   already, so it is left open with its tile. The other edge cells are walled as usual. So
//!   the old room's walls that fall inside the new one open up, the new room's walls that fall
//!   inside the old one are not built, and the two outlines meet where they cross.
//!
//! A room drawn wholly inside another with Intersection on joins it too: every cell of its
//! edge is left open, so it builds no wall and only lays its floor inside, and the status bar
//! says so.

use super::tiles::{rectangle_cells, SHAPE_LIMIT};
use super::*;

/// How faint the ghost of a room's walls is.
const WALL_GHOST: f32 = 0.5;

/// The colour of the mark on a cell whose wall a room would take away.
const REMOVED_MARK: [f32; 4] = [1.0, 0.25, 0.2, 0.45];

/// A room being dragged out on one grid.
pub(in crate::session) struct RoomDrag {
    grid: i32,
    from: (i32, i32),
    to: (i32, i32),
    /// The floor tile in hand at the press. None when no tile was in hand. It builds nothing then.
    floor: Option<String>,
}

/// What a room would do to its cells, as worked out from the map before it is built, and for
/// its ghost.
#[derive(Debug, Default, PartialEq)]
pub(super) struct RoomPlan {
    /// The cells inside that take the floor in hand.
    pub(super) floor: Vec<(i32, i32)>,
    /// The cells of the edge that take the edge tile and a wall.
    pub(super) walled: Vec<(i32, i32)>,
    /// The cells of the edge left open, inside a room it joins. They keep their tile.
    pub(super) open: Vec<(i32, i32)>,
    /// The walls taken away, by uid, with their cells.
    pub(super) removed: Vec<((i32, i32), i32)>,
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

    /// The walls standing on a grid inside a rectangle of cells, by cell: entities not inside
    /// another whose prototype is a wall, as uids in order.
    fn walls_in(&self, grid_uid: i32, from: (i32, i32), to: (i32, i32)) -> HashMap<(i32, i32), Vec<i32>> {
        let mut walls: HashMap<(i32, i32), Vec<i32>> = HashMap::new();
        let Some(doc) = self.doc.as_ref() else { return walls };
        let (left, right) = (from.0.min(to.0), from.0.max(to.0));
        let (bottom, top) = (from.1.min(to.1), from.1.max(to.1));
        for entity in doc.entities.values().filter(|entity| entity.parent == grid_uid && !entity.contained) {
            let cell = (entity.x.floor() as i32, entity.y.floor() as i32);
            if cell.0 < left || cell.0 > right || cell.1 < bottom || cell.1 > top {
                continue;
            }
            if self.prototype_indices.get(&entity.prototype).is_some_and(|&index| self.prototypes[index].is_wall()) {
                walls.entry(cell).or_default().push(entity.uid);
            }
        }
        for uids in walls.values_mut() {
            uids.sort_unstable();
        }
        walls
    }

    /// What a room from one corner to the other would do to the cells of a grid, as the
    /// module's notes tell, with the Intersection switch as it is.
    pub(super) fn plan_room(&self, grid_uid: i32, from: (i32, i32), to: (i32, i32)) -> RoomPlan {
        let mut plan = RoomPlan::default();
        let Some(doc) = self.doc.as_ref() else { return plan };
        let join = self.room_intersection();
        let edge_tile = self.floor_tile();
        let walls = self.walls_in(grid_uid, from, to);
        let (inside, edge) = room_cells(from, to);
        for cell in inside {
            match walls.get(&cell) {
                Some(uids) if join => {
                    plan.removed.extend(uids.iter().map(|&uid| (cell, uid)));
                    plan.floor.push(cell);
                }
                Some(_) => {}
                None => plan.floor.push(cell),
            }
        }
        for cell in edge {
            if walls.contains_key(&cell) {
                continue;
            }
            let tile = doc.tile_at(grid_uid, cell);
            if join && tile != SPACE && tile != edge_tile {
                plan.open.push(cell);
            } else {
                plan.walled.push(cell);
            }
        }
        plan
    }

    /// Builds a room on a grid from one corner to the other, as one edit with no stroke: the
    /// walls it takes away first, then the tiles, so that no wall goes on a cell with no tile,
    /// then the new walls. A room of more than `SHAPE_LIMIT` cells, or with no wall to build
    /// with, is refused with a line on the status bar.
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
        let plan = self.plan_room(grid_uid, from, to);
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(grid) = doc.grids.iter().find(|grid| grid.uid == grid_uid) else { return };
        doc.active_grid = grid_uid;
        self.grid = (grid.uid, grid.x, grid.y);

        let mut ops: Vec<Value> = plan.removed.iter().map(|&(_, uid)| json!({ "kind": "delete", "uid": uid })).collect();
        let mut laid: Vec<([(f32, f32); 4], &str)> = Vec::new();
        for (cells, tile) in [(&plan.floor, floor), (&plan.walled, edge_tile.as_str())] {
            for &cell in cells {
                if doc.tile_at(grid_uid, cell) != tile {
                    ops.push(json!({ "kind": "tile", "grid": grid_uid, "x": cell.0, "y": cell.1, "tile": tile }));
                    laid.push((grid.cells_outline(cell, cell), tile));
                }
            }
        }
        for &(x, y) in &plan.walled {
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
        if plan.walled.is_empty() && !plan.open.is_empty() {
            self.set_status("Joined the room around it. No walls built.");
        }
    }

    /// The ghost of a room being dragged out, for `overlay_quads`, as it would be built: its
    /// floor and edge tiles faint over the map, and the cells of its edge left open in their
    /// own tile (`under`), the wall's picture faint on each cell it would wall once the engine
    /// has sent it, and a red mark on each cell whose wall it would take away (`over`). A room
    /// too large to build shows its outline alone.
    pub(super) fn room_overlay(&mut self, under: &mut Vec<f32>, over: &mut Vec<f32>) {
        let Drag::Room(drag) = &self.drag else { return };
        let (grid, from, to, floor) = (drag.grid, drag.from, drag.to, drag.floor.clone());
        let size = (((from.0 - to.0).abs() + 1) as usize) * (((from.1 - to.1).abs() + 1) as usize);
        if size > SHAPE_LIMIT {
            return;
        }
        let plan = self.plan_room(grid, from, to);
        if let Some(floor) = floor {
            self.faint_tiles(grid, &plan.floor, &floor, under);
        }
        let edge_tile = self.floor_tile().to_owned();
        self.faint_tiles(grid, &plan.walled, &edge_tile, under);
        for &cell in &plan.open {
            let Some(tile) = self.doc.as_ref().map(|doc| doc.tile_at(grid, cell).to_owned()) else { break };
            self.faint_tiles(grid, &[cell], &tile, under);
        }
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid)) else { return };
        let mut marked: Vec<(i32, i32)> = plan.removed.iter().map(|&(cell, _)| cell).collect();
        marked.dedup();
        for cell in marked {
            for corner in grid.cells_outline(cell, cell) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&REMOVED_MARK);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
        let Some(wall) = self.room_wall() else { return };
        let Some(quads) = self.preview(wall, 0) else { return };
        let (sin, cos) = grid.rotation.sin_cos();
        for &cell in &plan.walled {
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
