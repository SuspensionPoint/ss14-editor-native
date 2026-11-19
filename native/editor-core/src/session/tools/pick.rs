//! The pick tool: takes what is under the cursor into the palette, an entity or a tile.

use super::select::Place;
use super::*;

/// One thing the pick tool can take at a place.
#[derive(Clone, Debug, PartialEq)]
pub enum Pickable {
    /// An entity, by its prototype's id.
    Entity(String),
    /// A decal, by its prototype's id, with its colour, angle, z-index and whether it is
    /// cleanable as settings to place more like it with.
    Decal(String, DecalSettings),
    /// The tile of the cell, by its id.
    Tile(String),
}

impl Pickable {
    /// The id it goes by.
    pub fn id(&self) -> &str {
        match self {
            Pickable::Entity(id) | Pickable::Decal(id, _) | Pickable::Tile(id) => id,
        }
    }
}

/// What the pick tool keeps while the wheel steps through the things under the cursor.
#[derive(Default)]
pub(in crate::session) struct Picking {
    /// The list that is open, from the first turn of the wheel over it.
    list: Option<Stack>,
}

/// One list the wheel steps through.
struct Stack {
    /// The cell the cursor was over as the list opened. The list closes when it leaves it.
    place: Place,
    /// What could be picked there then, top first, and the row the wheel is on.
    entries: Vec<Pickable>,
    current: usize,
    /// How far the wheel has turned since the list opened, in notches and the parts they came
    /// in, and how many whole notches of that have been stepped.
    notches: f32,
    stepped: i32,
}

impl Picking {
    /// Closes the list, and says whether one was open.
    pub(in crate::session) fn close(&mut self) -> bool {
        self.list.take().is_some()
    }
}

/// The list the pick tool's wheel steps through, as the interface draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct PickList {
    /// What could be picked, top first. The tile is last.
    pub entries: Vec<Pickable>,
    /// The row the wheel is on. A press picks it.
    pub current: usize,
    /// The corners of the cell the list is of, on the map.
    pub cell: [(f32, f32); 4],
}

impl Session {
    /// What the pick tool can take at a map position, top first: the visible entities, then
    /// the visible decals there (`decals_at`), and then the tile of the cell if it is not
    /// space. The entities are those of the cell's stack (`stack_at`), as the wheel lists
    /// them, or with `pressed` those whose picture is there, found as a click of the select
    /// tool finds them. An entity or a decal of a prototype the engine does not know is left
    /// out, since it could not be chosen.
    fn pickables_at(&mut self, world: (f32, f32), pressed: bool) -> Vec<Pickable> {
        let found = if pressed { self.entities_at(world) } else { self.stack_at(world).1 };
        let decals = self.decals_at(world);
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let mut entries: Vec<Pickable> = found
            .iter()
            .filter_map(|uid| doc.entities.get(uid))
            .filter(|entity| self.prototype(&entity.prototype).is_some())
            .map(|entity| Pickable::Entity(entity.prototype.clone()))
            .collect();
        for decal in decals.into_iter().filter_map(|key| self.decal(key)).filter(|decal| self.decal_prototype(&decal.prototype).is_some()) {
            let settings = DecalSettings { color: decal.color.clone(), angle: decal.angle, z_index: decal.z_index, cleanable: decal.cleanable };
            entries.push(Pickable::Decal(decal.prototype.clone(), settings));
        }
        if let Some(grid) = doc.grid_at(world) {
            let tile = doc.tile_at(grid.uid, grid.cell_at(world));
            if tile != SPACE && !tile.is_empty() {
                entries.push(Pickable::Tile(tile.to_owned()));
            }
        }
        entries
    }

    /// Chooses what was picked. An entity is chosen with `choose_entity`, which takes Place in
    /// hand, and a tile with `choose_tile`, which takes Paint. A prototype with no place in
    /// the palette is chosen as well, since it is chosen by its index among them all. A decal
    /// is chosen with `choose_decal`, which takes Paint, and its colour, angle, z-index and
    /// cleanable become the placement settings. Free placing stays as it was.
    fn take_picked(&mut self, picked: Pickable) {
        match &picked {
            Pickable::Entity(id) => match self.prototype_indices.get(id).copied() {
                Some(index) => self.choose_entity(index),
                None => return,
            },
            Pickable::Decal(id, settings) => {
                self.choose_decal(id);
                self.set_decal_settings(settings.clone());
            }
            Pickable::Tile(id) => self.choose_tile(id),
        }
        self.reveal = true;
        self.set_status(format!("Picked {}", picked.id()));
    }

    /// The pick tool was pressed. With the wheel's list open over the cell it closes the list
    /// and picks the row the list is on. Otherwise it picks the first thing there.
    pub(super) fn pick_down(&mut self, world: (f32, f32)) {
        let place = self.cell_of(world);
        let listed = self.picking.list.take().filter(|list| list.place == place).map(|list| list.entries[list.current].clone());
        if let Some(picked) = listed.or_else(|| self.pickables_at(world, true).into_iter().next()) {
            self.take_picked(picked);
        }
    }

    /// The pick tool was pressed with the secondary button. It closes the wheel's list and
    /// picks nothing.
    pub(super) fn pick_secondary_down(&mut self) {
        self.picking.close();
    }

    /// The wheel turned for the pick tool. It is taken where two or more things could be
    /// picked, with no button down, and steps through them: one down the list for each whole
    /// notch towards the user, one up for each away, round both ends. The list begins on the
    /// first. With fewer it is not taken, and zooms. The list stays open, and counts the
    /// notches and their parts, while the cursor is in its cell and what is there is the same.
    pub(super) fn pick_wheel(&mut self, notches: f32, world: (f32, f32)) -> bool {
        let open = self.picking.list.take();
        if !matches!(self.drag, Drag::None) {
            return false;
        }
        let entries = self.pickables_at(world, false);
        if entries.len() < 2 {
            return false;
        }
        let place = self.cell_of(world);
        let mut list = match open {
            Some(list) if list.place == place && list.entries == entries => list,
            _ => Stack { place, entries, current: 0, notches: 0.0, stepped: 0 },
        };
        list.notches += notches;
        let whole = list.notches.round() as i32;
        // A notch towards the user is negative, and goes down the list.
        list.current = (list.current as i32 - (whole - list.stepped)).rem_euclid(list.entries.len() as i32) as usize;
        list.stepped = whole;
        self.picking.list = Some(list);
        true
    }

    /// The cursor moved. The pick tool's list closes when the cursor has left its cell.
    pub(super) fn pick_moved(&mut self, world: (f32, f32)) {
        if self.picking.list.as_ref().is_some_and(|list| list.place != self.cell_of(world)) {
            self.picking.close();
        }
    }

    /// The list the pick tool's wheel is stepping through, for the interface to draw beside
    /// its cell.
    pub fn pick_list(&self) -> Option<PickList> {
        let list = self.picking.list.as_ref()?;
        Some(PickList { entries: list.entries.clone(), current: list.current, cell: self.place_corners(list.place)? })
    }
}
