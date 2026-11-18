//! Copy, cut and paste of entities, and of the area tool's region. Paste is a mode of the
//! select tool for entities, and of the area tool for a region.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use super::area::{turned_offset, within, FILL};
use super::select::quarter_on;
use super::*;
use crate::model::{Decal, Grid};
use crate::prefab::Prefab;

/// What takes a press of Escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeTo {
    /// A menu that is open closes.
    Menu,
    /// The dialogs that are open close.
    Dialog,
    /// The text field being typed into takes it, and nothing else does: the selection and the
    /// tools are left alone.
    Field,
    /// The map: [`Session::escape`].
    Map,
}

/// Where Escape goes, by what is open and whether a text field has the keyboard: an open menu
/// first, then the dialogs (a field in a dialog closes it), then a field of a side panel,
/// and the map last.
pub fn escape_to(menu: bool, dialog: bool, typing: bool) -> EscapeTo {
    if menu {
        EscapeTo::Menu
    } else if dialog {
        EscapeTo::Dialog
    } else if typing {
        EscapeTo::Field
    } else {
        EscapeTo::Map
    }
}

/// How faint the ghost of a paste is.
const GHOST: f32 = 0.5;

/// What was last copied. It outlives the map it was copied from. It holds entities, or a
/// region, and a copy of either takes the place of the other.
pub struct Clipboard {
    /// The game's own text of the copied entities, made by the engine. The editor never reads
    /// it. Empty for a region with nothing standing on it.
    text: String,
    roots: usize,
    entities: usize,
    /// The cell of the grid the roots are measured from: the lowest of theirs each way, or for
    /// a region the lowest cell of the box around it.
    from: (i32, i32),
    /// The rectangles the copied roots were drawn with at the copy, with their corners in the
    /// grid's own coordinates measured from the lower left corner of the origin cell.
    ghost: Vec<f32>,
    /// The region's own part, for a copy of a region.
    region: Option<RegionCopy>,
}

/// A tile of a copied region: its cell measured from the origin cell, the tile's id, its
/// variant and its rotation byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopiedTile {
    pub cell: (i32, i32),
    pub tile: String,
    pub variant: u8,
    pub rotation_mirroring: u8,
}

/// What a copy of a region holds besides its entities: its cells, its tiles and its decals,
/// measured from the origin cell. A decal's place is its corner measured from the lower left
/// corner of the origin cell, and its id means nothing.
pub(super) struct RegionCopy {
    pub(super) cells: Vec<(i32, i32)>,
    pub(super) tiles: Vec<CopiedTile>,
    pub(super) decals: Vec<Decal>,
}

impl Clipboard {
    /// The game's own text of what was copied.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many entities were copied that are inside none of the others.
    pub fn roots(&self) -> usize {
        self.roots
    }

    /// How many entities were copied in all, with what they hold.
    pub fn entities(&self) -> usize {
        self.entities
    }

    /// The origin cell, in the coordinates of the grid it was copied from.
    pub fn from(&self) -> (i32, i32) {
        self.from
    }

    /// Whether it holds a region, which the area tool pastes.
    pub fn is_region(&self) -> bool {
        self.region.is_some()
    }

    /// The cells of a copied region measured from the origin cell, in order. Empty for entities.
    pub fn region_cells(&self) -> &[(i32, i32)] {
        self.region.as_ref().map_or(&[], |region| &region.cells)
    }

    /// The tiles of a copied region, in the order of their cells. Empty for entities.
    pub fn region_tiles(&self) -> &[CopiedTile] {
        self.region.as_ref().map_or(&[], |region| &region.tiles)
    }

    /// The decals of a copied region, with their corners measured from the lower left corner
    /// of the origin cell. Empty for entities.
    pub fn region_decals(&self) -> &[Decal] {
        self.region.as_ref().map_or(&[], |region| &region.decals)
    }

    /// A copied region as a prefab file holds it. None for entities.
    pub(super) fn to_prefab(&self) -> Option<Prefab> {
        let region = self.region.as_ref()?;
        Some(Prefab {
            from: self.from,
            roots: self.roots,
            entities: self.entities,
            cells: region.cells.clone(),
            tiles: region.tiles.clone(),
            decals: region.decals.clone(),
            text: self.text.clone(),
        })
    }

    /// A prefab as a copied region. Its ghost has no pictures of its entities, since they were
    /// drawn in another session.
    pub(super) fn from_prefab(prefab: Prefab) -> Clipboard {
        let Prefab { from, roots, entities, cells, tiles, decals, text } = prefab;
        Clipboard { text, roots, entities, from, ghost: Vec::new(), region: Some(RegionCopy { cells, tiles, decals }) }
    }
}

/// A copy asked of the engine and not yet answered.
pub(in crate::session) struct Copying {
    seq: u64,
    /// What was selected as it was asked for.
    uids: Vec<i32>,
    /// Whether the answer deletes what was copied.
    cut: bool,
    /// The rectangles of the roots, with corners in the grid's own coordinates.
    ghost: Vec<f32>,
    /// For a copy of a region, its own part, its origin cell, and for a cut the region to delete.
    region: Option<(RegionCopy, (i32, i32), Option<Region>)>,
    /// The name of the prefab the answer is saved as, for Save as Prefab.
    prefab: Option<String>,
}

impl Copying {
    /// A cut that would come after a map is opened would delete in the new map. It is a copy alone then.
    pub(in crate::session) fn cut_no_more(&mut self) {
        self.cut = false;
    }
}

/// The paste mode of the select tool.
#[derive(Default)]
pub(in crate::session) struct Pasting {
    /// While the mode lasts, the quarter turns of the ghost against the clock, from 0 to 3. That
    /// is how the engine counts them.
    quarters: Option<u8>,
}

impl Pasting {
    /// Whether the mode is on.
    pub(super) fn is_live(&self) -> bool {
        self.quarters.is_some()
    }

    /// Ends the mode, and says whether it was on.
    pub(super) fn end(&mut self) -> bool {
        self.quarters.take().is_some()
    }

    /// A quarter turn of the ghost, with the clock or against it.
    fn turn(&mut self, clockwise: bool) {
        if let Some(quarters) = self.quarters.as_mut() {
            *quarters = (*quarters + if clockwise { 3 } else { 1 }) % 4;
        }
    }
}

/// "1 entity", or "N entities".
fn entities_text(count: usize) -> String {
    if count == 1 { "1 entity".to_owned() } else { format!("{count} entities") }
}

/// A label for the history with a count in it, as "Paste 3 entities". The engine's thread
/// takes a label that lasts as long as the program, so each one is made once and kept.
fn counted(verb: &'static str, count: usize) -> &'static str {
    static MADE: Mutex<BTreeMap<(&'static str, usize), &'static str>> = Mutex::new(BTreeMap::new());
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    made.entry((verb, count)).or_insert_with(|| Box::leak(format!("{verb} {}", entities_text(count)).into_boxed_str()))
}

/// Of a set of entities, those inside none of the others, in the order given.
fn roots_of(doc: &Doc, uids: &[i32]) -> Vec<i32> {
    let inside = |uid: i32| {
        let mut at = uid;
        // A chain of parents is short. The bound keeps a mirror that is wrong from looping.
        for _ in 0..64 {
            let Some(entity) = doc.entities.get(&at) else { return false };
            if uids.contains(&entity.parent) {
                return true;
            }
            at = entity.parent;
        }
        false
    };
    uids.iter().copied().filter(|&uid| !inside(uid)).collect()
}

/// The grid an entity is on, through whatever holds it.
fn grid_above(doc: &Doc, uid: i32) -> Option<i32> {
    let mut at = uid;
    for _ in 0..64 {
        let entity = doc.entities.get(&at)?;
        if doc.grid(entity.parent).is_some() {
            return Some(entity.parent);
        }
        at = entity.parent;
    }
    None
}

/// "N tiles", with " and N entities" when any stood on them.
fn region_text(tiles: usize, entities: usize) -> String {
    let tiles = if tiles == 1 { "1 tile".to_owned() } else { format!("{tiles} tiles") };
    if entities == 0 { tiles } else { format!("{tiles} and {}", entities_text(entities)) }
}

/// A cell measured from the origin cell, turned a number of quarters against the clock about
/// the middle of the origin cell, as the engine turns a paste.
fn turned_cell((x, y): (i32, i32), quarters: u8) -> (i32, i32) {
    match quarters % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    }
}

/// The ops of one stamp of a copied region with its lower left origin cell at a cell of a
/// grid, turned a number of quarters against the clock about the middle of that cell. First
/// the tiles are written, with the rotation byte turned for the tiles named in `turning`,
/// which the game draws by it. Then come the entities, as one paste op, so that each lands on
/// a cell that has a tile. Last come the decals, as new ones. What is there already stays
/// under what lands on it, as under a moved region. A cell of the region with no tile writes
/// nothing.
///
/// A decal turns as one piece with the region, as in a turn of the region (`area::turn_ops`).
/// Where its corner would then be on a cell with no tile, which the engine refuses, it goes to
/// the cell its own cell turns to, and keeps its place in that cell turned. A decal of a
/// prototype not in `decals_known` is left out, since the engine refuses to make one.
pub(super) fn stamp_ops(
    grid: &Grid,
    clipboard: &Clipboard,
    at: (i32, i32),
    quarters: u8,
    turning: &HashSet<&str>,
    decals_known: &HashSet<&str>,
) -> Vec<Value> {
    let Some(region) = clipboard.region.as_ref() else { return Vec::new() };
    let to = |cell: (i32, i32)| {
        let (x, y) = turned_cell(cell, quarters);
        (at.0 + x, at.1 + y)
    };
    let mut ops = Vec::new();
    let mut written = HashSet::new();
    for tile in &region.tiles {
        let cell = to(tile.cell);
        let mut byte = tile.rotation_mirroring;
        if turning.contains(tile.tile.as_str()) {
            for _ in 0..quarters {
                byte = super::area::turned_byte(byte, false);
            }
        }
        written.insert(cell);
        ops.push(json!({ "kind": "tile", "grid": grid.uid, "x": cell.0, "y": cell.1, "tile": tile.tile, "variant": tile.variant, "rotationMirroring": byte }));
    }
    if !clipboard.text.is_empty() {
        ops.push(json!({
            "kind": "paste",
            "text": clipboard.text,
            "grid": grid.uid,
            "fromX": clipboard.from.0,
            "fromY": clipboard.from.1,
            "x": at.0 as f32,
            "y": at.1 as f32,
            "quarters": quarters,
        }));
    }
    let tiled = |cell: (i32, i32)| written.contains(&cell) || grid.tiles.contains_key(&cell);
    for decal in region.decals.iter().filter(|decal| decals_known.contains(decal.prototype.as_str())) {
        // Its middle, half a cell from its corner, turned about the middle of the origin cell.
        let (x, y) = turned((decal.x + 0.5, decal.y + 0.5), quarters);
        let mut corner = (at.0 as f32 + x - 0.5, at.1 as f32 + y - 0.5);
        if !tiled((corner.0.floor() as i32, corner.1.floor() as i32)) {
            let own = (decal.x.floor() as i32, decal.y.floor() as i32);
            let cell = to(own);
            if !tiled(cell) {
                continue;
            }
            let mut offset = (decal.x - own.0 as f32, decal.y - own.1 as f32);
            for _ in 0..quarters {
                offset = turned_offset(offset, false);
            }
            corner = (within(cell.0, offset.0), within(cell.1, offset.1));
        }
        let mut angle = decal.angle;
        for _ in 0..quarters {
            angle = quarter_on(angle, false);
        }
        ops.push(json!({
            "kind": "decal", "grid": grid.uid, "x": corner.0, "y": corner.1,
            "prototype": decal.prototype, "color": decal.color, "rotation": angle, "zIndex": decal.z_index, "cleanable": decal.cleanable,
        }));
    }
    ops
}

/// A point of the ghost, measured from the lower left corner of the origin cell, turned a
/// number of quarters against the clock about the middle of that cell.
fn turned((x, y): (f32, f32), quarters: u8) -> (f32, f32) {
    let (dx, dy) = (x - 0.5, y - 0.5);
    let (dx, dy) = match quarters % 4 {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        _ => (dy, -dx),
    };
    (dx + 0.5, dy + 0.5)
}

impl Session {
    /// What was last copied, if anything has been.
    pub fn clipboard(&self) -> Option<&Clipboard> {
        self.clipboard.as_ref()
    }

    /// Whether the select tool is pasting.
    pub fn is_pasting(&self) -> bool {
        self.pasting.is_live()
    }

    /// Ctrl+C, and Copy in the Edit menu: a copy of the area tool's region when that tool is
    /// in hand with a region, and else of the selected entities.
    pub fn copy(&mut self) {
        match self.tool {
            Tool::Area if self.region.is_some() => self.copy_region(false),
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Room | Tool::Area | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => self.copy_selection(),
        }
    }

    /// Ctrl+X, and Cut in the Edit menu: as [`Self::copy`], with the delete after.
    pub fn cut(&mut self) {
        match self.tool {
            Tool::Area if self.region.is_some() => self.copy_region(true),
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Room | Tool::Area | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => self.cut_selection(),
        }
    }

    /// Whether [`Self::copy`] and [`Self::cut`] have something to work on now.
    pub fn can_copy(&self) -> bool {
        self.holds_region() || !self.selection.is_empty()
    }

    /// Whether the area tool is in hand with a region, which copy, cut, delete and a turn then act on.
    pub fn holds_region(&self) -> bool {
        self.tool == Tool::Area && self.region.is_some()
    }

    /// What copy, cut, a turn and delete act on now, in a word for an Edit menu to name them
    /// by: "region" while the area tool holds one, and else "selection".
    pub fn edit_target(&self) -> &'static str {
        if self.holds_region() {
            "region"
        } else {
            "selection"
        }
    }

    /// Asks the engine for a copy of the selected entities. The clipboard is filled when the
    /// answer comes. With nothing selected nothing is asked.
    pub fn copy_selection(&mut self) {
        self.ask_copy(false);
    }

    /// A copy, and when its answer has come the delete of what was copied, as one edit.
    /// Nothing is deleted if the engine refuses the copy.
    pub fn cut_selection(&mut self) {
        self.ask_copy(true);
    }

    /// The pictures of entities as drawn now, with their corners in the grid's own coordinates.
    fn ghost_of(&self, grid: &Grid, uids: &[i32]) -> Vec<f32> {
        let mut ghost = Vec::new();
        let Some(list) = self.document.and_then(|id| self.lists.documents.get(&id)) else { return ghost };
        for item in uids.iter().filter_map(|uid| list.items.get(uid)) {
            for quad in item.quads.chunks_exact(FLOATS_PER_QUAD) {
                for corner in quad[..8].chunks_exact(2) {
                    let local = grid.to_local((corner[0], corner[1]));
                    ghost.extend_from_slice(&[local.0, local.1]);
                }
                ghost.extend_from_slice(&quad[8..]);
            }
        }
        ghost
    }

    fn ask_copy(&mut self, cut: bool) {
        let Some(doc) = self.doc.as_ref() else { return };
        if self.selection.is_empty() {
            return;
        }
        let uids = self.selection.clone();
        // The pictures of the roots, as drawn now.
        let grid = uids.iter().find_map(|&uid| grid_above(doc, uid)).and_then(|grid| doc.grid(grid));
        let ghost = grid.map(|grid| self.ghost_of(grid, &roots_of(doc, &uids))).unwrap_or_default();
        let seq = self.seq();
        self.copying = Some(Copying { seq, uids: uids.clone(), cut, ghost, region: None, prefab: None });
        self.send(ToEngine::Copy { uids, seq });
    }

    /// A copy of the area tool's region: its tiles and decals from the mirror, beside the
    /// engine's copy of what stands on it. With nothing standing on it the engine is not
    /// asked and the clipboard is filled at once. A cut drops the region at once, and deletes
    /// it with what stands on it when the clipboard is filled, as one edit. A region that is
    /// too large, or whose last move or turn the engine has yet to answer, is not copied.
    pub fn copy_region(&mut self, cut: bool) {
        self.copy_region_as(cut, None);
    }

    /// As [`Self::copy_region`], and with a prefab's name the clipboard is saved as that
    /// prefab when it is filled (`prefabs.rs`).
    pub(super) fn copy_region_as(&mut self, cut: bool, prefab: Option<String>) {
        let done = match (cut, &prefab) {
            (_, Some(_)) => "saved as a prefab",
            (true, None) => "cut",
            (false, None) => "copied",
        };
        if self.region_held_back(done) {
            return;
        }
        let (Some(doc), Some(region)) = (self.doc.as_ref(), self.region.as_ref()) else { return };
        let Some((copy, origin, uids)) = super::area::region_copy(doc, region) else { return };
        let ghost = doc.grid(region.grid()).map(|grid| self.ghost_of(grid, &uids)).unwrap_or_default();
        let cut_region = if cut { self.region.take() } else { None };
        if cut {
            self.clear_region();
        }
        if uids.is_empty() {
            // Nothing for the engine to copy. An answer still to come counts no more.
            self.copying = None;
            self.fill_clipboard(Clipboard { text: String::new(), roots: 0, entities: 0, from: origin, ghost, region: Some(copy) }, cut_region, prefab);
            return;
        }
        let seq = self.seq();
        self.copying = Some(Copying { seq, uids: uids.clone(), cut, ghost, region: Some((copy, origin, cut_region)), prefab });
        self.send(ToEngine::Copy { uids, seq });
    }

    /// The engine answered a copy. The answer to the last copy asked for fills the clipboard,
    /// and for a cut deletes what was copied that is inside nothing else copied, as one edit.
    pub(in crate::session) fn copied(&mut self, seq: u64, text: String, roots: usize, entities: usize, from: (i32, i32)) {
        if self.copying.as_ref().is_none_or(|copying| copying.seq != seq) {
            return;
        }
        let Some(Copying { uids, cut, mut ghost, region, prefab, .. }) = self.copying.take() else { return };
        // A region is measured from its own origin cell, and the paste is told that one.
        let from = region.as_ref().map_or(from, |(_, origin, _)| *origin);
        for quad in ghost.chunks_exact_mut(FLOATS_PER_QUAD) {
            for corner in quad[..8].chunks_exact_mut(2) {
                corner[0] -= from.0 as f32;
                corner[1] -= from.1 as f32;
            }
        }
        if let Some((copy, _, cut_region)) = region {
            // A map opened before the answer makes the cut a copy alone.
            let cut_region = cut_region.filter(|_| cut);
            self.fill_clipboard(Clipboard { text, roots, entities, from, ghost, region: Some(copy) }, cut_region, prefab);
            return;
        }
        self.fill_clipboard(Clipboard { text, roots, entities, from, ghost, region: None }, None, None);
        if cut {
            let Some(doc) = self.doc.as_ref() else { return };
            let ops: Vec<Value> =
                roots_of(doc, &uids).into_iter().filter(|uid| doc.entities.contains_key(uid)).map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
            self.edit(ops, counted("Cut", entities), None, false);
        }
    }

    /// Fills the clipboard and says so, and for a cut of a region deletes the region with what
    /// stands on it now, as one edit. Pasting of the other kind ends, since its ghost is gone.
    /// With a prefab's name the clipboard is saved as that prefab too.
    fn fill_clipboard(&mut self, clipboard: Clipboard, cut_region: Option<Region>, prefab: Option<String>) {
        let text = match &clipboard.region {
            Some(region) => region_text(region.tiles.len(), clipboard.entities),
            None => entities_text(clipboard.entities),
        };
        let tool = if clipboard.region.is_some() { Tool::Area } else { Tool::Select };
        self.clipboard = Some(clipboard);
        self.clipboard_filled();
        if self.tool != tool {
            self.pasting.end();
        }
        self.set_status(format!("Copied {text}"));
        if let Some(name) = prefab {
            self.save_clipboard_as(&name);
        }
        if let (Some(region), Some(doc)) = (cut_region, self.doc.as_ref()) {
            let ops = super::area::delete_ops(doc, &region);
            self.edit(ops, "Cut region", None, false);
        }
    }

    /// The engine refused something. A copy it refused is no longer waited for.
    pub(in crate::session) fn copy_refused(&mut self, seq: u64) {
        if let Some(copying) = self.copying.take_if(|copying| copying.seq == seq) {
            if let Some(name) = copying.prefab {
                self.set_status(format!("Prefab {name} not saved: the engine refused the copy"));
            }
        }
    }

    /// Ctrl+V: begins pasting, with the ghost unturned. Entities are pasted with Select in
    /// hand, and a region with the area tool, whose region is dropped. With nothing copied it
    /// does nothing. While the mode is on it changes nothing, the turn included.
    pub fn begin_paste(&mut self) {
        let Some(clipboard) = self.clipboard.as_ref() else { return };
        let tool = if clipboard.region.is_some() { Tool::Area } else { Tool::Select };
        self.set_tool(tool);
        if tool == Tool::Area {
            self.clear_region();
        }
        if !self.pasting.is_live() {
            self.pasting.quarters = Some(0);
        }
    }

    /// Escape on the map: drops a room being dragged out, or else closes the area tool's
    /// menu, or else ends linking, or else ends pasting, or else clears the selection and the region. An interface asks [`escape_to`]
    /// first whether Escape is the map's.
    pub fn escape(&mut self) {
        if self.cancel_room() || self.region_menu.take().is_some() || self.cancel_linking() {
            return;
        }
        if !self.pasting.end() {
            self.selection.clear();
            self.selected_decals.clear();
            self.clear_region();
        }
    }

    /// R while pasting: a quarter turn of the ghost and of what the next stamp pastes.
    pub(super) fn turn_paste(&mut self, clockwise: bool) {
        self.pasting.turn(clockwise);
    }

    /// Where a stamp at a map position would put the origin cell: the grid with a tile there,
    /// and the lower left corner of the origin cell in that grid's coordinates. That is the
    /// cursor's cell, or with Shift the place that puts the middle of the origin cell on the
    /// cursor itself. None where no grid has a tile.
    fn paste_target(&self, world: (f32, f32)) -> Option<(&Grid, (f32, f32))> {
        let grid = self.doc.as_ref()?.grid_with_tile_at(world)?;
        let at = if self.held.shift {
            let local = grid.to_local(world);
            (local.0 - 0.5, local.1 - 0.5)
        } else {
            let cell = grid.cell_at(world);
            (cell.0 as f32, cell.1 as f32)
        };
        Some((grid, at))
    }

    /// The select tool was pressed while pasting: one edit with one paste op, at the place
    /// the ghost is. Off every grid nothing is asked. The mode stays for more.
    pub(super) fn paste_down(&mut self, world: (f32, f32)) {
        let (Some(quarters), Some(clipboard)) = (self.pasting.quarters, self.clipboard.as_ref()) else { return };
        if clipboard.region.is_some() {
            return;
        }
        let Some((grid, at)) = self.paste_target(world) else { return };
        let op = json!({
            "kind": "paste",
            "text": clipboard.text,
            "grid": grid.uid,
            "fromX": clipboard.from.0,
            "fromY": clipboard.from.1,
            "x": at.0,
            "y": at.1,
            "quarters": quarters,
        });
        let label = counted("Paste", clipboard.entities);
        self.edit(vec![op], label, None, false);
    }

    /// The ghost of a paste where a stamp would put it, faint. It is the pictures the copied
    /// roots had at the copy, turned rigidly about the middle of the origin cell. The pictures
    /// themselves do not change with the turn, as the engine's own drawing of a turned wall
    /// might: it is a guide to where things go and not how they will look.
    pub(super) fn paste_overlay(&mut self, over: &mut Vec<f32>) {
        let (Some(quarters), Some(world), Some(clipboard)) = (self.pasting.quarters, self.hover, self.clipboard.as_ref()) else { return };
        let (grid, at) = match self.tool {
            Tool::Select if clipboard.region.is_none() => match self.paste_target(world) {
                Some(target) => target,
                None => return,
            },
            Tool::Area if clipboard.region.is_some() => match self.region_paste_target(world) {
                Some((grid, cell)) => (grid, (cell.0 as f32, cell.1 as f32)),
                None => return,
            },
            _ => return,
        };
        let grid = grid.uid;
        if clipboard.region.is_some() {
            self.region_paste_ghost(grid, (at.0 as i32, at.1 as i32), quarters, over);
        }
        let (Some(grid), Some(clipboard)) = (self.doc.as_ref().and_then(|doc| doc.grid(grid)), self.clipboard.as_ref()) else { return };
        for quad in clipboard.ghost.chunks_exact(FLOATS_PER_QUAD) {
            for corner in quad[..8].chunks_exact(2) {
                let (x, y) = turned((corner[0], corner[1]), quarters);
                let world = grid.to_world((at.0 + x, at.1 + y));
                over.extend_from_slice(&[world.0, world.1]);
            }
            over.extend_from_slice(&[quad[8], quad[9], quad[10], quad[11] * GHOST]);
            over.extend_from_slice(&quad[12..]);
        }
    }

    /// Where a stamp of a region at a map position would put its origin cell: the grid the
    /// tile tools would work on there, and the cursor's cell on it. A region stamps by whole
    /// cells, with Shift or without.
    fn region_paste_target(&self, world: (f32, f32)) -> Option<(&Grid, (i32, i32))> {
        let grid = self.doc.as_ref()?.grid_at(world)?;
        Some((grid, grid.cell_at(world)))
    }

    /// The cells of the grid a stamp of the copied region would cover with the cursor where it
    /// is, in order. Empty while the area tool is not pasting a region.
    pub fn region_paste_cells(&self) -> Vec<(i32, i32)> {
        let (Tool::Area, Some(quarters), Some(world), Some(clipboard)) = (self.tool, self.pasting.quarters, self.hover, self.clipboard.as_ref()) else { return Vec::new() };
        let Some((_, at)) = self.region_paste_target(world) else { return Vec::new() };
        let mut cells: Vec<(i32, i32)> = clipboard
            .region_cells()
            .iter()
            .map(|&cell| {
                let (x, y) = turned_cell(cell, quarters);
                (at.0 + x, at.1 + y)
            })
            .collect();
        cells.sort_unstable();
        cells
    }

    /// The area tool was pressed while pasting a region: one stamp at the cursor's cell, as
    /// one edit with no stroke (`stamp_ops`). The mode stays for more.
    pub(super) fn region_paste_down(&mut self, world: (f32, f32)) {
        let (Some(quarters), Some(clipboard)) = (self.pasting.quarters, self.clipboard.as_ref()) else { return };
        let Some((grid, at)) = self.region_paste_target(world) else { return };
        let turning: HashSet<&str> = self.tiles.iter().filter(|tile| tile.allows_rotation).map(|tile| tile.id.as_str()).collect();
        let decals_known: HashSet<&str> = self.decal_prototypes.iter().map(|decal| decal.id.as_str()).collect();
        let ops = stamp_ops(grid, clipboard, at, quarters, &turning, &decals_known);
        self.edit(ops, "Paste region", None, false);
    }

    /// The ghost of a region's stamp, faint: the fill of each of its cells, and its tiles by
    /// their pictures, turned with the stamp. The tiles' own pictures are not turned.
    fn region_paste_ghost(&mut self, grid: i32, at: (i32, i32), quarters: u8, over: &mut Vec<f32>) {
        let (Some(doc), Some(clipboard)) = (self.doc.as_ref(), self.clipboard.as_ref()) else { return };
        let (Some(grid), Some(region)) = (doc.grid(grid), clipboard.region.as_ref()) else { return };
        let to = |cell: (i32, i32)| {
            let (x, y) = turned_cell(cell, quarters);
            (at.0 + x, at.1 + y)
        };
        for &cell in &region.cells {
            for corner in grid.cells_outline(to(cell), to(cell)) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&FILL);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
        // The texture of each kind of tile, found once.
        let mut textures: HashMap<&str, Option<u32>> = HashMap::new();
        for tile in &region.tiles {
            let texture = *textures.entry(tile.tile.as_str()).or_insert_with(|| {
                let sprite = self.tile_sprites.get(&tile.tile)?.as_ref()?;
                Some(self.lists.local_texture(sprite, 0, 32, 32))
            });
            let Some(texture) = texture else { continue };
            for corner in grid.cells_outline(to(tile.cell), to(tile.cell)) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&[1.0, 1.0, 1.0, GHOST, texture as f32, 0.0, 0.0, 32.0, 32.0]);
        }
    }
}
