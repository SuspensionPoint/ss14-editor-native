//! The area tool: a region of cells of one grid, made of boxes, and moved, turned, deleted or
//! copied with what stands on it. A right-click on the region opens its menu.

use super::paste::{CopiedTile, RegionCopy};
use super::select::quarter_on;
use super::*;
use crate::model::Decal;

/// The fill of a selected cell, the web editor's `rgba(50,130,255,0.25)`.
pub(super) const FILL: [f32; 4] = [50.0 / 255.0, 130.0 / 255.0, 1.0, 0.25];

/// How faint the ghost of a region being moved is.
const GHOST: f32 = 0.5;

/// A region of more cells than this is not moved, turned, copied or deleted.
const LIMIT: usize = 50_000;

/// What an entry of the region's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Copy,
    Cut,
    Delete,
    Paste,
    SaveAsPrefab,
}

/// An entry of the region's menu, as the interface draws it: its name, its key and whether it
/// can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub label: &'static str,
    pub shortcut: &'static str,
    pub enabled: bool,
}

/// The cells the area tool has selected, in the cell coordinates of one grid. It always has
/// at least one cell.
pub(in crate::session) struct Region {
    grid: i32,
    cells: HashSet<(i32, i32)>,
    /// The point the region turns about, in half cells, from its first turn on. A move takes
    /// it along, and a region of other cells is a new region with none.
    pivot: Option<(i32, i32)>,
}

impl Region {
    /// A region of these cells of a grid. None if there are no cells.
    pub(in crate::session) fn new(grid: i32, cells: impl IntoIterator<Item = (i32, i32)>) -> Option<Region> {
        let cells: HashSet<(i32, i32)> = cells.into_iter().collect();
        (!cells.is_empty()).then_some(Region { grid, cells, pivot: None })
    }

    /// The point a quarter turn of the region goes about, in half cells: the pivot it has, or
    /// else the middle of the box around it. A quarter turn takes cells to cells only about
    /// a cell's corner or a cell's middle, so a middle that is neither is taken half a cell to
    /// the right.
    fn pivot(&self) -> (i32, i32) {
        self.pivot.unwrap_or_else(|| {
            let (left, bottom, right, top) = self.bounds();
            let (x, y) = (left + right + 1, bottom + top + 1);
            // A corner is even both ways in half cells, and a middle odd both ways.
            (x + (x - y).rem_euclid(2), y)
        })
    }

    pub(in crate::session) fn grid(&self) -> i32 {
        self.grid
    }

    #[cfg(test)]
    pub(in crate::session) fn cells(&self) -> &HashSet<(i32, i32)> {
        &self.cells
    }

    /// The box around the cells: (left, bottom, right, top), all included.
    fn bounds(&self) -> (i32, i32, i32, i32) {
        self.cells.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |(l, b, r, t), &(x, y)| (l.min(x), b.min(y), r.max(x), t.max(y)))
    }
}

/// What a box does with its cells when it is let go.
#[derive(Clone, Copy)]
enum BoxMode {
    Replace,
    Add,
    TakeAway,
}

/// A box of cells being dragged out on one grid.
pub(in crate::session) struct BoxDrag {
    grid: i32,
    from: (i32, i32),
    to: (i32, i32),
    mode: BoxMode,
}

/// The region being moved: the cell that was pressed, how many cells it has gone, and the
/// entities that stood on it at the press, for the ghost.
pub(in crate::session) struct MoveDrag {
    from: (i32, i32),
    by: (i32, i32),
    entities: Vec<i32>,
}

/// Every cell of a box, both corners included.
fn box_cells(from: (i32, i32), to: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let (left, right, bottom, top) = (from.0.min(to.0), from.0.max(to.0), from.1.min(to.1), from.1.max(to.1));
    (left..=right).flat_map(move |x| (bottom..=top).map(move |y| (x, y)))
}

/// Whether a position in a grid's own coordinates is in a cell of the region.
fn on(region: &Region, x: f32, y: f32) -> bool {
    region.cells.contains(&(x.floor() as i32, y.floor() as i32))
}

/// The entities that stand on a region, in the order of their uids: the children of its grid
/// that are inside nothing and whose position is in one of its cells, on any layer.
fn standing<'a>(doc: &'a Doc, region: &Region) -> Vec<&'a Entity> {
    let mut entities: Vec<&Entity> = doc.entities.values().filter(|entity| entity.parent == region.grid && !entity.contained && on(region, entity.x, entity.y)).collect();
    entities.sort_unstable_by_key(|entity| entity.uid);
    entities
}

/// The ops that move a region by whole cells with what stands on it, from the mirror alone,
/// in the order the engine check proved (`RegionCheck.cs`, `MovePlan`). First the tiles are
/// written where they go, with their variant and turn. Then come the entities, then the
/// decals whose corner is on the region, with every field resent. Last the cells left behind
/// are set to space. A cell that is left and gone to is written once and not cleared. A cell
/// of the region with no tile writes nothing and clears nothing. After all that, the air the
/// grid stores for each cell with a tile goes where the tile went (`air_ops`).
pub(in crate::session) fn move_ops(doc: &Doc, region: &Region, by: (i32, i32)) -> Vec<Value> {
    let Some(grid) = doc.grid(region.grid) else { return Vec::new() };
    let mut cells: Vec<(i32, i32)> = region.cells.iter().copied().filter(|cell| grid.tiles.contains_key(cell)).collect();
    cells.sort_unstable();
    let (dx, dy) = (by.0 as f32, by.1 as f32);

    let mut ops = Vec::new();
    for cell in &cells {
        let tile = grid.tiles[cell];
        ops.push(json!({
            "kind": "tile", "grid": grid.uid, "x": cell.0 + by.0, "y": cell.1 + by.1,
            "tile": doc.tile_names[tile.id as usize], "variant": tile.variant, "rotationMirroring": tile.rotation_mirroring,
        }));
    }
    for entity in standing(doc, region) {
        ops.push(json!({ "kind": "move", "uid": entity.uid, "x": entity.x + dx, "y": entity.y + dy }));
    }
    let mut decals: Vec<_> = grid.decals.values().filter(|decal| on(region, decal.x, decal.y)).collect();
    decals.sort_unstable_by_key(|decal| decal.id);
    for decal in decals {
        ops.push(json!({
            "kind": "decal.update", "uid": decal.id, "grid": grid.uid, "x": decal.x + dx, "y": decal.y + dy,
            "prototype": decal.prototype, "color": decal.color, "rotation": decal.angle, "zIndex": decal.z_index, "cleanable": decal.cleanable,
        }));
    }
    // A tile op that names no tile sets space.
    for cell in &cells {
        let written = grid.tiles.contains_key(&(cell.0 - by.0, cell.1 - by.1)) && region.cells.contains(&(cell.0 - by.0, cell.1 - by.1));
        if !written {
            ops.push(json!({ "kind": "tile", "grid": grid.uid, "x": cell.0, "y": cell.1 }));
        }
    }
    ops.extend(air_ops(grid.uid, cells.iter().map(|&cell| (cell, (cell.0 + by.0, cell.1 + by.1)))));
    ops
}

/// The ops that carry the air the grid stores for each cell to the cell its tile goes to. The
/// engine reads every cell the air comes from before it writes any, so a region that overlaps
/// itself moves whole. A cell left behind keeps no air. The air has no facing, so a turn turns
/// only where it goes (`Documents.MoveAir` in the kernel).
fn air_ops(grid: i32, cells: impl Iterator<Item = ((i32, i32), (i32, i32))>) -> impl Iterator<Item = Value> {
    cells.map(move |(from, to)| json!({ "kind": "air", "grid": grid, "fromX": from.0, "fromY": from.1, "x": to.0, "y": to.1 }))
}

/// The ops that delete a region with what stands on it, from the mirror alone, in the order
/// the engine check proved (`RegionCheck.cs`, `DeletePlan`). First the entities that stand on
/// it are deleted, and what they hold goes with them unnamed. Then its decals are removed,
/// and its cells are set to space. Last the air the grid stores for each of those cells goes
/// too, by an air op with no destination.
pub(in crate::session) fn delete_ops(doc: &Doc, region: &Region) -> Vec<Value> {
    let Some(grid) = doc.grid(region.grid) else { return Vec::new() };
    let mut ops: Vec<Value> = standing(doc, region).iter().map(|entity| json!({ "kind": "delete", "uid": entity.uid })).collect();
    let mut decals: Vec<i32> = grid.decals.values().filter(|decal| on(region, decal.x, decal.y)).map(|decal| decal.id).collect();
    decals.sort_unstable();
    ops.extend(decals.iter().map(|id| json!({ "kind": "decal.remove", "grid": grid.uid, "uid": id })));
    let mut cells: Vec<(i32, i32)> = region.cells.iter().copied().filter(|cell| grid.tiles.contains_key(cell)).collect();
    cells.sort_unstable();
    ops.extend(cells.iter().map(|cell| json!({ "kind": "tile", "grid": grid.uid, "x": cell.0, "y": cell.1 })));
    ops.extend(cells.iter().map(|cell| json!({ "kind": "air", "grid": grid.uid, "fromX": cell.0, "fromY": cell.1, "clear": true })));
    ops
}

/// The cell a cell goes to in a quarter turn about a point given in half cells.
fn turned_cell((x, y): (i32, i32), pivot: (i32, i32), clockwise: bool) -> (i32, i32) {
    // The cell's middle from the pivot, in half cells.
    let (dx, dy) = (2 * x + 1 - pivot.0, 2 * y + 1 - pivot.1);
    let (dx, dy) = if clockwise { (dy, -dx) } else { (-dy, dx) };
    ((pivot.0 + dx - 1).div_euclid(2), (pivot.1 + dy - 1).div_euclid(2))
}

/// A tile's rotation byte a quarter on. Its low two bits count quarter turns against the
/// clock, and the bit above them is the mirroring, which stays.
pub(super) fn turned_byte(byte: u8, clockwise: bool) -> u8 {
    (byte & 4) | (byte.wrapping_add(if clockwise { 3 } else { 1 }) & 3)
}

/// Where a decal's corner is within its cell when the decal turns with that cell alone, the
/// offset from 0 up to 1 each way turned about the cell's middle. The decal's own square
/// turns with it, so a 1 that would come of a 0 is 0 (`RegionCheck.cs`, `TurnedOffset`).
pub(super) fn turned_offset((x, y): (f32, f32), clockwise: bool) -> (f32, f32) {
    let across = |offset: f32| if offset == 0.0 { 0.0 } else { 1.0 - offset };
    if clockwise { (y, across(x)) } else { (across(y), x) }
}

/// A cell's number and an offset in it as one number, which is still in the cell.
pub(super) fn within(cell: i32, offset: f32) -> f32 {
    (cell as f32 + offset).min((cell as f32 + 1.0).next_down())
}

/// What a copy of a region holds of its own, from the mirror alone, with its origin cell,
/// which is the lowest cell of the box around it each way, and the entities that stand on it,
/// which the engine copies. Its cells and its tiles are measured from the origin cell, in
/// order. Its decals are those whose corner is on a cell of it that has a tile, as the engine
/// has none elsewhere, in the order of their ids. None if its grid is not in the mirror.
pub(super) fn region_copy(doc: &Doc, region: &Region) -> Option<(RegionCopy, (i32, i32), Vec<i32>)> {
    let grid = doc.grid(region.grid)?;
    let (left, bottom, _, _) = region.bounds();
    let from = |(x, y): (i32, i32)| (x - left, y - bottom);
    let mut cells: Vec<(i32, i32)> = region.cells.iter().map(|&cell| from(cell)).collect();
    cells.sort_unstable();
    let mut tiles: Vec<CopiedTile> = region
        .cells
        .iter()
        .filter_map(|cell| {
            let tile = grid.tiles.get(cell)?;
            Some(CopiedTile { cell: from(*cell), tile: doc.tile_names[tile.id as usize].clone(), variant: tile.variant, rotation_mirroring: tile.rotation_mirroring })
        })
        .collect();
    tiles.sort_unstable_by_key(|tile| tile.cell);
    let mut decals: Vec<Decal> = grid
        .decals
        .values()
        .filter(|decal| {
            let cell = (decal.x.floor() as i32, decal.y.floor() as i32);
            region.cells.contains(&cell) && grid.tiles.contains_key(&cell)
        })
        .map(|decal| Decal { x: decal.x - left as f32, y: decal.y - bottom as f32, ..decal.clone() })
        .collect();
    decals.sort_unstable_by_key(|decal| decal.id);
    let uids = standing(doc, region).iter().map(|entity| entity.uid).collect();
    Some((RegionCopy { cells, tiles, decals }, (left, bottom), uids))
}

/// The ops that turn a region a quarter about a point given in half cells, with what stands
/// on it, from the mirror alone, in the order of a move. Only the region's own cells are
/// read. First the tiles are written at the cells they turn to, with the rotation byte
/// turned for the tiles named in `turning`, which the game draws by it. Then each entity
/// goes about the pivot and is turned a quarter with it. The engine keeps the facing of what
/// it never turns. Then come the decals, and the cells left behind are set to space. Last the
/// air of each cell goes where its tile went.
///
/// A decal turns as one piece with the region: its middle, half a cell from its corner, goes
/// about the pivot, and its angle a quarter on. Where its corner would then be on a cell
/// that holds no tile after the edit, which the engine refuses, it goes to the cell its own
/// cell turns to, and keeps its place in that cell turned (`turned_offset`).
pub(in crate::session) fn turn_ops(doc: &Doc, region: &Region, pivot: (i32, i32), clockwise: bool, turning: &HashSet<&str>) -> Vec<Value> {
    let Some(grid) = doc.grid(region.grid) else { return Vec::new() };
    let mut cells: Vec<(i32, i32)> = region.cells.iter().copied().filter(|cell| grid.tiles.contains_key(cell)).collect();
    cells.sort_unstable();
    let written: HashSet<(i32, i32)> = cells.iter().map(|&cell| turned_cell(cell, pivot, clockwise)).collect();
    // Positions are worked out as the engine check works them out, in 64 bits.
    let (px, py) = (f64::from(pivot.0) / 2.0, f64::from(pivot.1) / 2.0);
    let about = |x: f64, y: f64| if clockwise { (px + (y - py), py - (x - px)) } else { (px - (y - py), py + (x - px)) };
    // Whether a cell holds a tile once the edit is done.
    let tiled = |cell: (i32, i32)| written.contains(&cell) || (grid.tiles.contains_key(&cell) && !region.cells.contains(&cell));

    let mut ops = Vec::new();
    for cell in &cells {
        let tile = grid.tiles[cell];
        let name = &doc.tile_names[tile.id as usize];
        let byte = if turning.contains(name.as_str()) { turned_byte(tile.rotation_mirroring, clockwise) } else { tile.rotation_mirroring };
        let (x, y) = turned_cell(*cell, pivot, clockwise);
        ops.push(json!({ "kind": "tile", "grid": grid.uid, "x": x, "y": y, "tile": name, "variant": tile.variant, "rotationMirroring": byte }));
    }
    for entity in standing(doc, region) {
        let (x, y) = about(entity.x.into(), entity.y.into());
        ops.push(json!({ "kind": "move", "uid": entity.uid, "x": x as f32, "y": y as f32 }));
        ops.push(json!({ "kind": "rotate", "uid": entity.uid, "rotation": quarter_on(entity.rotation, clockwise) }));
    }
    let mut decals: Vec<_> = grid.decals.values().filter(|decal| on(region, decal.x, decal.y)).collect();
    decals.sort_unstable_by_key(|decal| decal.id);
    for decal in decals {
        let middle = about(f64::from(decal.x) + 0.5, f64::from(decal.y) + 0.5);
        let mut at = ((middle.0 - 0.5) as f32, (middle.1 - 0.5) as f32);
        if !tiled((at.0.floor() as i32, at.1.floor() as i32)) {
            let own = (decal.x.floor() as i32, decal.y.floor() as i32);
            let to = turned_cell(own, pivot, clockwise);
            // A decal with no tile under it either way stays as it is.
            if !tiled(to) {
                continue;
            }
            let offset = turned_offset((decal.x - own.0 as f32, decal.y - own.1 as f32), clockwise);
            at = (within(to.0, offset.0), within(to.1, offset.1));
        }
        ops.push(json!({
            "kind": "decal.update", "uid": decal.id, "grid": grid.uid, "x": at.0, "y": at.1,
            "prototype": decal.prototype, "color": decal.color, "rotation": quarter_on(decal.angle, clockwise), "zIndex": decal.z_index, "cleanable": decal.cleanable,
        }));
    }
    // A tile op that names no tile sets space.
    for cell in cells.iter().filter(|cell| !written.contains(cell)) {
        ops.push(json!({ "kind": "tile", "grid": grid.uid, "x": cell.0, "y": cell.1 }));
    }
    ops.extend(air_ops(grid.uid, cells.iter().map(|&cell| (cell, turned_cell(cell, pivot, clockwise)))));
    ops
}

impl Session {
    /// Turns the region a quarter about its pivot with what stands on it, as one edit, and
    /// the region is then the cells it turned to. The pivot is chosen at the first turn and
    /// kept, so that four turns bring everything back where it was. Nothing is turned while
    /// a drag is live, or while the engine has yet to answer a region's last move or turn,
    /// which the status line then says. A region that is too large is not turned either.
    pub fn turn_region(&mut self, clockwise: bool) {
        if !matches!(self.drag, Drag::None) || self.region_held_back("turned") {
            return;
        }
        let (Some(doc), Some(region)) = (self.doc.as_ref(), self.region.as_mut()) else { return };
        let pivot = region.pivot();
        let turning: HashSet<&str> = self.tiles.iter().filter(|tile| tile.allows_rotation).map(|tile| tile.id.as_str()).collect();
        let ops = turn_ops(doc, region, pivot, clockwise, &turning);
        region.pivot = Some(pivot);
        region.cells = region.cells.iter().map(|&cell| turned_cell(cell, pivot, clockwise)).collect();
        let asked = !ops.is_empty();
        let seq = self.edit(ops, "Turn region", None, false);
        if asked {
            self.region_waiting = Some(seq);
        }
    }

    /// Whether the region may not be moved, turned or deleted now, with the status line
    /// saying why. It is too large, or the engine has yet to answer the last move or turn of
    /// a region, so the mirror does not hold what stands on these cells. False with no
    /// region.
    pub(super) fn region_held_back(&mut self, done: &str) -> bool {
        let Some(region) = self.region.as_ref() else { return false };
        let why = if region.cells.len() > LIMIT {
            format!("Not {done}: the region has more than {} tiles", thousands(LIMIT))
        } else if self.region_waiting.is_some() {
            format!("Not {done}: the engine hasn't answered the region's last change yet")
        } else {
            return false;
        };
        self.set_status(why);
        true
    }

    /// The engine has answered a request. If it was the last move or turn of a region, the
    /// mirror has it now. If the engine refused it, the region is not where it was taken
    /// to be, and is dropped.
    pub(in crate::session) fn region_answered(&mut self, seq: u64, refused: bool) {
        let Some(waiting) = self.region_waiting.filter(|&waiting| waiting <= seq) else { return };
        self.region_waiting = None;
        if refused && waiting == seq {
            self.clear_region();
        }
    }

    /// Delete or Backspace was pressed. With the area tool in hand and a region selected the
    /// region goes. With any other tool, or no region, the selected entities go.
    pub fn delete_key(&mut self) {
        match self.tool {
            Tool::Area if self.region.is_some() => self.delete_region(),
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Room | Tool::Area | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => self.delete_selection(),
        }
    }

    /// Whether Delete, and a quarter turn from the Edit menu, have something to act on now:
    /// the area tool's region, or selected entities.
    pub fn can_delete(&self) -> bool {
        self.holds_region() || !self.selection.is_empty() || !self.selected_decals.is_empty()
    }

    /// Deletes the region with what stands on it, as one edit, and drops it. One that is too
    /// large is kept, and so is one the engine has yet to answer the last move or turn of.
    /// The status line says so.
    pub fn delete_region(&mut self) {
        if self.region_held_back("deleted") {
            return;
        }
        let (Some(doc), Some(region)) = (self.doc.as_ref(), self.region.as_ref()) else { return };
        let ops = delete_ops(doc, region);
        self.clear_region();
        self.edit(ops, "Delete region", None, false);
    }

    /// The area tool was pressed. A plain press on a cell of the region begins a move of it,
    /// unless the region is too large to move or the engine has yet to answer its last move
    /// or turn, which the status line then says. Any other
    /// press begins a box on the grid under it: one that takes away with Ctrl held, one that
    /// adds with Shift, and else one that takes the region's place.
    pub(super) fn area_down(&mut self, world: (f32, f32)) {
        let Some((grid, cell)) = self.cell_at(world) else { return };
        let mode = match (self.held.ctrl, self.held.shift) {
            (true, _) => BoxMode::TakeAway,
            (false, true) => BoxMode::Add,
            (false, false) => BoxMode::Replace,
        };
        let pressed = self.region.as_ref().filter(|region| matches!(mode, BoxMode::Replace) && region.grid == grid && region.cells.contains(&cell));
        if pressed.is_none() {
            self.drag = Drag::AreaBox(BoxDrag { grid, from: cell, to: cell, mode });
            return;
        }
        if self.region_held_back("moved") {
            return;
        }
        let Some(region) = self.region.as_ref() else { return };
        let entities = self.doc.as_ref().map(|doc| standing(doc, region).iter().map(|entity| entity.uid).collect()).unwrap_or_default();
        self.drag = Drag::AreaMove(MoveDrag { from: cell, by: (0, 0), entities });
    }

    /// The area tool was pressed with the secondary button. On a cell of the region it opens
    /// the region's menu there. Anywhere else no region is selected any more.
    pub(super) fn area_secondary_down(&mut self, world: (f32, f32)) {
        let on_region = self.region.as_ref().is_some_and(|region| {
            self.doc.as_ref().and_then(|doc| doc.grid(region.grid)).is_some_and(|grid| region.cells.contains(&grid.cell_at(world)))
        });
        if on_region && matches!(self.drag, Drag::None) {
            self.region_menu = Some(world);
        } else {
            self.clear_region();
        }
    }

    /// The region's menu, while it is open: where on the map it was opened, and its entries
    /// in order. None while it is closed, and with no region.
    pub fn region_menu(&self) -> Option<((f32, f32), Vec<MenuEntry>)> {
        let at = self.region_menu?;
        self.region.as_ref()?;
        let entry = |action, label, shortcut, enabled| MenuEntry { action, label, shortcut, enabled };
        Some((
            at,
            vec![
                entry(MenuAction::Copy, "Copy", "Ctrl+C", true),
                entry(MenuAction::Cut, "Cut", "Ctrl+X", true),
                entry(MenuAction::Delete, "Delete", "Del", true),
                entry(MenuAction::Paste, "Paste", "Ctrl+V", self.clipboard.is_some()),
                entry(MenuAction::SaveAsPrefab, "Save as Prefab...", "", true),
            ],
        ))
    }

    /// Closes the region's menu with nothing done, as a press anywhere else does.
    pub fn close_region_menu(&mut self) {
        self.region_menu = None;
    }

    /// An entry of the region's menu was chosen. The menu closes. An entry that is not
    /// enabled does nothing, nor does any with the menu closed.
    pub fn choose_from_region_menu(&mut self, action: MenuAction) {
        let enabled = self.region_menu().is_some_and(|(_, entries)| entries.iter().any(|entry| entry.action == action && entry.enabled));
        self.region_menu = None;
        if !enabled {
            return;
        }
        match action {
            MenuAction::Copy => self.copy_region(false),
            MenuAction::Cut => self.copy_region(true),
            MenuAction::Delete => self.delete_region(),
            MenuAction::Paste => self.begin_paste(),
            MenuAction::SaveAsPrefab => self.prefabs.ask_name(),
        }
    }

    /// Drops the region and closes its menu, and ends a drag of the area tool with nothing asked.
    pub fn clear_region(&mut self) {
        self.region = None;
        self.region_menu = None;
        if matches!(self.drag, Drag::AreaBox(_) | Drag::AreaMove(_)) {
            self.drag = Drag::None;
        }
    }

    /// Drops the region if its grid is not in the mirror any more.
    pub(in crate::session) fn region_grid_checked(&mut self) {
        let gone = match (&self.region, &self.doc) {
            (Some(region), Some(doc)) => doc.grid(region.grid).is_none(),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if gone {
            self.clear_region();
        }
    }

    /// A box was dragged on: its far corner follows the cursor, in the cells of its own grid.
    pub(super) fn area_box_dragged(&self, world: (f32, f32), drag: BoxDrag) -> Drag {
        let BoxDrag { grid, from, to, mode } = drag;
        let to = self.doc.as_ref().and_then(|doc| doc.grid(grid)).map_or(to, |grid| grid.cell_at(world));
        Drag::AreaBox(BoxDrag { grid, from, to, mode })
    }

    /// A box was let go: its cells take the region's place, are added to it or are taken out
    /// of it, as the box was begun for. A region with no cells left is no region. A region is
    /// of one grid, so a box on another grid adds to nothing and takes nothing away.
    pub(super) fn area_box_up(&mut self, world: (f32, f32), drag: BoxDrag) {
        let Drag::AreaBox(BoxDrag { grid, from, to, mode }) = self.area_box_dragged(world, drag) else { return };
        let boxed = box_cells(from, to);
        let mut cells = match self.region.take() {
            Some(region) if region.grid == grid => region.cells,
            Some(region) if matches!(mode, BoxMode::TakeAway) => {
                self.region = Some(region);
                return;
            }
            _ => HashSet::new(),
        };
        match mode {
            BoxMode::Replace => cells = boxed.collect(),
            BoxMode::Add => cells.extend(boxed),
            BoxMode::TakeAway => boxed.for_each(|cell| {
                cells.remove(&cell);
            }),
        }
        self.region = Region::new(grid, cells);
    }

    /// A move was dragged on: the region follows the cursor by whole cells of its grid.
    /// Nothing is asked of the engine. The interface draws a ghost where the region would go.
    pub(super) fn area_move_dragged(&self, world: (f32, f32), drag: MoveDrag) -> Drag {
        let MoveDrag { from, by, entities } = drag;
        let grid = self.region.as_ref().and_then(|region| self.doc.as_ref()?.grid(region.grid));
        let by = grid.map_or(by, |grid| {
            let cell = grid.cell_at(world);
            (cell.0 - from.0, cell.1 - from.1)
        });
        Drag::AreaMove(MoveDrag { from, by, entities })
    }

    /// A move was let go: the region goes where it was dragged with what stands on it, as one
    /// edit, and is then the cells it went to, with its pivot taken along. A move that ends
    /// where it began asks nothing.
    pub(super) fn area_move_up(&mut self, world: (f32, f32), drag: MoveDrag) {
        let Drag::AreaMove(MoveDrag { by, .. }) = self.area_move_dragged(world, drag) else { return };
        if by != (0, 0) {
            self.move_region(by);
        }
    }

    /// Moves the region by whole cells of its grid with what stands on it, as one edit, and
    /// the region is then the cells it went to, with its pivot taken along. Returns the
    /// edit's number when something was asked of the engine.
    pub(super) fn move_region(&mut self, by: (i32, i32)) -> Option<u64> {
        let (Some(doc), Some(region)) = (self.doc.as_ref(), self.region.as_mut()) else { return None };
        let ops = move_ops(doc, region, by);
        region.cells = region.cells.iter().map(|&(x, y)| (x + by.0, y + by.1)).collect();
        region.pivot = region.pivot.map(|(x, y)| (x + 2 * by.0, y + 2 * by.1));
        let asked = !ops.is_empty();
        let seq = self.edit(ops, "Move region", None, false);
        asked.then(|| {
            self.region_waiting = Some(seq);
            seq
        })
    }

    /// The outline of a box being dragged out, in the colour of what the box does.
    pub(super) fn area_box_outlines(&self, drag: &BoxDrag, outlines: &mut Vec<Outline>) {
        let style = match drag.mode {
            BoxMode::Replace => OutlineStyle::Drag,
            BoxMode::Add => OutlineStyle::DragAdd,
            BoxMode::TakeAway => OutlineStyle::DragTakeAway,
        };
        if let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)) {
            outlines.push(Outline { corners: grid.cells_outline(drag.from, drag.to), style });
        }
    }

    /// The size of a box being dragged out as "WxH", over the middle of its top edge.
    pub(super) fn area_box_label(&self, drag: &BoxDrag) -> Option<((f32, f32), String)> {
        let BoxDrag { grid, from, to, .. } = drag;
        let grid = self.doc.as_ref()?.grid(*grid)?;
        let (width, height) = ((from.0 - to.0).abs() + 1, (from.1 - to.1).abs() + 1);
        let (left, top) = (from.0.min(to.0) as f32, from.1.max(to.1) as f32 + 1.0);
        Some((grid.to_world((left + width as f32 / 2.0, top)), format!("{width}x{height}")))
    }

    /// The region and its grid, and how many cells a move that is live has taken it.
    fn region_shown(&self) -> Option<(&Region, &crate::model::Grid, (i32, i32))> {
        let region = self.region.as_ref()?;
        let grid = self.doc.as_ref()?.grid(region.grid)?;
        let by = match &self.drag {
            Drag::AreaMove(drag) => drag.by,
            _ => (0, 0),
        };
        Some((region, grid, by))
    }

    /// How many cells the region has. 0 with no region.
    pub fn region_size(&self) -> usize {
        self.region.as_ref().map_or(0, |region| region.cells.len())
    }

    /// The corners of each cell of the region on the map, for its fill. While the region is
    /// being moved they are where it would go.
    pub fn region_cells(&self) -> Vec<[(f32, f32); 4]> {
        let Some((region, grid, by)) = self.region_shown() else { return Vec::new() };
        region.cells.iter().map(|&(x, y)| grid.cells_outline((x + by.0, y + by.1), (x + by.0, y + by.1))).collect()
    }

    /// The outline of the region as segments on the map: each edge of a cell whose neighbour
    /// across it is not in the region. Edges along one line all run the same way.
    pub fn region_edges(&self) -> Vec<[(f32, f32); 2]> {
        let Some((region, grid, by)) = self.region_shown() else { return Vec::new() };
        let mut edges = Vec::new();
        for &(x, y) in &region.cells {
            let (l, b) = ((x + by.0) as f32, (y + by.1) as f32);
            let (r, t) = (l + 1.0, b + 1.0);
            let sides = [((x, y - 1), (l, b), (r, b)), ((x, y + 1), (l, t), (r, t)), ((x - 1, y), (l, b), (l, t)), ((x + 1, y), (r, b), (r, t))];
            for (neighbour, from, to) in sides {
                if !region.cells.contains(&neighbour) {
                    edges.push([grid.to_world(from), grid.to_world(to)]);
                }
            }
        }
        edges
    }

    /// How many cells the region has, as "N tiles", and where on the map to write it: the
    /// middle of the top of the box around the region.
    pub fn region_label(&self) -> Option<((f32, f32), String)> {
        let (region, grid, by) = self.region_shown()?;
        let (left, _, right, top) = region.bounds();
        let at = grid.to_world(((left + right + 1) as f32 / 2.0 + by.0 as f32, (top + 1 + by.1) as f32));
        let count = region.cells.len();
        Some((at, if count == 1 { "1 tile".to_owned() } else { format!("{count} tiles") }))
    }

    /// The area tool's own rectangles, to go on top of the map: the fill of the region, or
    /// while it is being moved its ghost where it would go.
    pub(super) fn area_overlay(&mut self, over: &mut Vec<f32>) {
        if let Drag::AreaMove(MoveDrag { by, entities, .. }) = &self.drag {
            if *by != (0, 0) {
                let (by, entities) = (*by, entities.clone());
                self.area_ghost(by, &entities, over);
                return;
            }
        }
        for corners in self.region_cells() {
            for corner in corners {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&FILL);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
    }

    /// The ghost of the region moved by whole cells, both faint: its tiles by their pictures,
    /// and the entities that stand on it by the rectangles the engine draws them with, moved
    /// over. What is on a hidden layer is left out of the ghost and moves all the same.
    fn area_ghost(&mut self, by: (i32, i32), entities: &[i32], over: &mut Vec<f32>) {
        let (Some(doc), Some(region)) = (self.doc.as_ref(), self.region.as_ref()) else { return };
        let Some(grid) = doc.grid(region.grid) else { return };

        // The texture of each kind of tile, found once.
        let mut textures: HashMap<u32, Option<u32>> = HashMap::new();
        for cell in &region.cells {
            let Some(tile) = grid.tiles.get(cell) else { continue };
            let texture = *textures.entry(tile.id).or_insert_with(|| {
                let sprite = self.tile_sprites.get(&doc.tile_names[tile.id as usize])?.as_ref()?;
                Some(self.lists.local_texture(sprite, 0, 32, 32))
            });
            let Some(texture) = texture else { continue };
            let to = (cell.0 + by.0, cell.1 + by.1);
            for corner in grid.cells_outline(to, to) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&[1.0, 1.0, 1.0, GHOST, texture as f32, 0.0, 0.0, 32.0, 32.0]);
        }

        let Some(list) = self.document.and_then(|id| self.lists.documents.get(&id)) else { return };
        let (origin, moved) = (grid.to_world((0.0, 0.0)), grid.to_world((by.0 as f32, by.1 as f32)));
        let (dx, dy) = (moved.0 - origin.0, moved.1 - origin.1);
        let hidden = self.layers.hidden_mask();
        for item in entities.iter().filter_map(|uid| list.items.get(uid)).filter(|item| hidden & (1 << group_of(item.kind, item.depth)) == 0) {
            for quad in item.quads.chunks_exact(FLOATS_PER_QUAD) {
                for corner in quad[..8].chunks_exact(2) {
                    over.extend_from_slice(&[corner[0] + dx, corner[1] + dy]);
                }
                over.extend_from_slice(&[quad[8], quad[9], quad[10], quad[11] * GHOST]);
                over.extend_from_slice(&quad[12..]);
            }
        }
    }
}
