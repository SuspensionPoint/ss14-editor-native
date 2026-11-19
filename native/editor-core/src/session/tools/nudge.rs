//! Nudging, as in the Hammer editor: the arrow keys move what is selected by one cell, or by
//! one pixel of a tile with Shift.

use super::decals::decal_update_op;
use super::*;
use crate::model::Decal;

/// A fine step: a thirty-second of a cell, one pixel of a tile's texture.
pub(super) const FINE_STEP: f32 = 1.0 / 32.0;

/// An arrow key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

impl Arrow {
    /// The way it moves things in a grid's own cells, and that way's name.
    fn way(self) -> ((i32, i32), &'static str) {
        match self {
            Arrow::Left => ((-1, 0), "west"),
            Arrow::Right => ((1, 0), "east"),
            Arrow::Up => ((0, 1), "north"),
            Arrow::Down => ((0, -1), "south"),
        }
    }
}

/// Whether the arrow keys reach the map, by what is open and whether a text field has the
/// keyboard. They go where Escape goes (`escape_to`), so a menu, a dialog or a field takes them.
pub fn arrows_reach_map(menu: bool, dialog: bool, typing: bool) -> bool {
    escape_to(menu, dialog, typing) == EscapeTo::Map
}

/// A count of things in words: "1 entity", "3 entities".
fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 { format!("1 {one}") } else { format!("{count} {many}") }
}

impl Session {
    /// An arrow key went down, or the keyboard repeated it, with Shift (`fine`) or without.
    /// With the select tool the selected entities and decals move one cell that way, in the
    /// cells of each one's own grid, so a turned grid turns the way. With Shift and nothing
    /// anchored selected they move one pixel (`FINE_STEP`), as Shift frees a drag. With the
    /// area tool its region moves one cell with what stands on it, as a drag moves it. Each
    /// press is one edit and one step to undo. With nothing selected, or another tool in
    /// hand, nothing happens and the map does not pan.
    ///
    /// The moves are built from the mirror. So a press is dropped while the engine has yet to
    /// answer the last edit, undo or redo, as a held key's repeats are while their move is in
    /// flight. Moves are never stacked up on a mirror that is behind. A press is dropped too
    /// during a drag, while pasting and while a dialog is open.
    pub fn nudge(&mut self, arrow: Arrow, fine: bool) {
        if self.doc.is_none() || self.dialog_open || !matches!(self.drag, Drag::None) || self.pasting.is_live() || self.last_edit > self.last_answered {
            return;
        }
        let (way, name) = arrow.way();
        match self.tool {
            Tool::Area => self.nudge_region(way, name),
            Tool::Select => self.nudge_selection(way, name, fine),
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Room | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => {}
        }
    }

    /// The region moves one cell, with its tiles, what stands on it, its decals and its air.
    /// One too large to move is kept, and the status line says so, as for a drag.
    fn nudge_region(&mut self, way: (i32, i32), name: &str) {
        if self.region.is_none() || self.region_held_back("moved") {
            return;
        }
        if let Some(seq) = self.move_region(way) {
            self.edit_says = Some((seq, format!("Moved region one cell {name}")));
        }
    }

    /// The selected entities and decals move one cell, or one pixel when `fine` and nothing
    /// selected is anchored. The engine's `move` op keeps links and containers whole.
    fn nudge_selection(&mut self, way: (i32, i32), name: &str, fine: bool) {
        let Some(doc) = self.doc.as_ref() else { return };
        let entities: Vec<&Entity> = self.selection.iter().filter_map(|uid| doc.entities.get(uid)).collect();
        let decals = self.selected_decal_list();
        if entities.is_empty() && decals.is_empty() {
            return;
        }
        let fine = fine && !entities.iter().any(|entity| entity.anchored);
        let step = if fine { FINE_STEP } else { 1.0 };
        let (dx, dy) = (way.0 as f32 * step, way.1 as f32 * step);
        let mut ops: Vec<Value> = entities.iter().map(|entity| json!({ "kind": "move", "uid": entity.uid, "x": entity.x + dx, "y": entity.y + dy })).collect();
        ops.extend(decals.iter().map(|(grid, decal)| decal_update_op(*grid, &Decal { x: decal.x + dx, y: decal.y + dy, ..decal.clone() })));
        let what = match (entities.len(), decals.len()) {
            (count, 0) => counted(count, "entity", "entities"),
            (0, count) => counted(count, "decal", "decals"),
            (things, marks) => format!("{} and {}", counted(things, "entity", "entities"), counted(marks, "decal", "decals")),
        };
        let seq = self.edit(ops, "Move", None, false);
        self.edit_says = Some((seq, format!("Moved {what} {} {name}", if fine { "one pixel" } else { "one cell" })));
        // The wheel's list is of the cell the entity has left.
        self.stepping.close();
    }
}
