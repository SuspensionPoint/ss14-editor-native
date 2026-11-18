//! Favourites: tiles, entities and decals a mapper keeps at hand in one list, in the order they
//! put them. They are kept per fork in the settings file, under the fork's folder, since forks
//! differ in prototypes. A favourite the fork does not have is kept, shown as missing and not
//! chosen. The number keys 1 to 9 choose the first nine.

use super::*;
use crate::forks::settings::{Favourite, FavouriteKind, Settings};

/// How many favourites the number keys reach.
pub const FAVOURITE_KEYS: usize = 9;

/// The favourites, and where they are kept.
#[derive(Default)]
pub(in crate::session) struct Favourites {
    items: Vec<Favourite>,
    /// The settings file and the fork they are kept under. None keeps them in the session only.
    kept: Option<(PathBuf, String)>,
}

impl Favourites {
    /// The favourites a settings file keeps for a fork, kept there again at each change.
    pub(in crate::session) fn load(file: PathBuf, fork: &str) -> Favourites {
        let items = Settings::load(&file).favourites(fork).to_vec();
        Favourites { items, kept: Some((file, fork.to_owned())) }
    }

    /// Writes them into the settings file, which is read again first so that nothing else in
    /// it is lost.
    fn save(&self) -> Result<(), String> {
        let Some((file, fork)) = &self.kept else { return Ok(()) };
        Settings::update(file, |settings| settings.set_favourites(fork, self.items.clone())).map(|_| ())
    }
}

/// Whether a favourite can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavouriteState {
    /// The fork has it, at this index of `Session::tiles`, `Session::prototypes` or
    /// `Session::decal_prototypes` by its kind.
    Here(usize),
    /// The engine has not said yet what the fork has.
    Waiting,
    /// The fork has no prototype of its id.
    Missing,
}

/// A favourite as its row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct FavouriteRow {
    pub favourite: Favourite,
    /// The name its own palette shows it by, or its id.
    pub name: String,
    pub state: FavouriteState,
    /// Its number key, 1 to 9, for the first nine.
    pub key: Option<usize>,
    /// Whether it is what the palettes hold.
    pub chosen: bool,
}

impl Session {
    /// The favourites, in their order, as their rows show them.
    pub fn favourites(&self) -> Vec<FavouriteRow> {
        let held = self.held_favourite();
        self.favourites
            .items
            .iter()
            .enumerate()
            .map(|(at, favourite)| {
                let state = self.favourite_state(favourite);
                let name = match (favourite.kind, state) {
                    (FavouriteKind::Entity, FavouriteState::Here(index)) => shown_name(&self.prototypes[index]),
                    _ => favourite.id.clone(),
                };
                let key = (at < FAVOURITE_KEYS).then_some(at + 1);
                FavouriteRow { favourite: favourite.clone(), name, state, key, chosen: held.as_ref() == Some(favourite) }
            })
            .collect()
    }

    /// Whether the fork has it, and where.
    fn favourite_state(&self, favourite: &Favourite) -> FavouriteState {
        // The engine tells the tiles, the prototypes and the decals at once.
        if self.tiles.is_empty() && self.prototypes.is_empty() {
            return FavouriteState::Waiting;
        }
        let index = match favourite.kind {
            FavouriteKind::Tile => self.tiles.iter().position(|tile| tile.id == favourite.id),
            FavouriteKind::Entity => self.prototype_indices.get(&favourite.id).copied(),
            FavouriteKind::Decal => self.decal_prototypes.iter().position(|decal| decal.id == favourite.id),
        };
        index.map_or(FavouriteState::Missing, FavouriteState::Here)
    }

    /// What the palettes hold, as a favourite would name it.
    pub fn held_favourite(&self) -> Option<Favourite> {
        match &self.palette {
            Palette::None => None,
            Palette::Tile(id) => Some(Favourite::new(FavouriteKind::Tile, id.clone())),
            Palette::Decal(id) => Some(Favourite::new(FavouriteKind::Decal, id.clone())),
            Palette::Entity(index) => self.prototypes.get(*index).map(|prototype| Favourite::new(FavouriteKind::Entity, prototype.id.clone())),
        }
    }

    pub fn is_favourite(&self, favourite: &Favourite) -> bool {
        self.favourites.items.contains(favourite)
    }

    /// Adds it at the end of the favourites, or takes it off if it is one.
    pub fn toggle_favourite(&mut self, favourite: Favourite) {
        match self.favourites.items.iter().position(|kept| *kept == favourite) {
            Some(at) => self.remove_favourite(at),
            None => {
                self.set_status(format!("Added {} to favourites", favourite.id));
                self.favourites.items.push(favourite);
                self.save_favourites();
            }
        }
    }

    /// Takes off the favourite at this place in the list.
    pub fn remove_favourite(&mut self, at: usize) {
        if at < self.favourites.items.len() {
            let favourite = self.favourites.items.remove(at);
            self.set_status(format!("Removed {} from favourites", favourite.id));
            self.save_favourites();
        }
    }

    /// Moves the favourite at this place one up the list, or one down.
    pub fn move_favourite(&mut self, at: usize, up: bool) {
        let to = if up { at.checked_sub(1) } else { Some(at + 1) };
        let Some(to) = to.filter(|&to| to < self.favourites.items.len() && at < self.favourites.items.len()) else { return };
        self.favourites.items.swap(at, to);
        self.save_favourites();
    }

    fn save_favourites(&mut self) {
        if let Err(error) = self.favourites.save() {
            self.set_status(format!("Favourites not saved: {error}"));
        }
    }

    /// Chooses the favourite at this place in the list as its own palette chooses it, with the
    /// same change of tool. One the fork does not have, or before the engine has said what it
    /// has, is not chosen. Returns whether it was.
    pub fn choose_favourite(&mut self, at: usize) -> bool {
        let Some(favourite) = self.favourites.items.get(at).cloned() else { return false };
        match self.favourite_state(&favourite) {
            FavouriteState::Here(index) => {
                match favourite.kind {
                    FavouriteKind::Tile => self.choose_tile(&favourite.id),
                    FavouriteKind::Entity => self.choose_entity(index),
                    FavouriteKind::Decal => self.choose_decal(&favourite.id),
                }
                true
            }
            FavouriteState::Missing => {
                self.set_status(format!("{} isn't in this fork", favourite.id));
                false
            }
            FavouriteState::Waiting => false,
        }
    }

    /// A number key from 1 to 9 went down on the map: it chooses that favourite and says so on
    /// the status line. The interface hands it on only where it hands on a tool's key: not
    /// while a field is typed into or a dialog is open. A drag that is live keeps what it has,
    /// so the key does nothing then. Returns whether anything changed.
    pub fn favourite_key(&mut self, number: usize) -> bool {
        if !(1..=FAVOURITE_KEYS).contains(&number) || !matches!(self.drag, Drag::None) {
            return false;
        }
        let at = number - 1;
        if at >= self.favourites.items.len() {
            self.set_status(format!("No favourite {number}"));
            return true;
        }
        if self.choose_favourite(at) {
            let name = self.in_hand().map_or(String::new(), |held| held.name().to_owned());
            self.set_status(format!("Favourite {number}: {name}"));
        }
        true
    }
}
