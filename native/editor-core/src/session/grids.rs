//! The grids of the open map: the list, centring the view on one, and adding, renaming and
//! removing them. Each is one edit, and so one step to undo. A grid file holds exactly one
//! grid, so there neither adding nor removing is offered.

use super::*;

/// Why grids cannot be added or removed in a grid file.
pub const GRID_FILE: &str = "A grid file holds exactly one grid, so you can't add or remove grids in it";

/// A grid as the grid list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct GridRow {
    pub uid: i32,
    pub name: String,
    pub tiles: usize,
    /// The grid the tile tools work on where no grid has a tile.
    pub active: bool,
}

impl Session {
    /// The open map's grids, in the order the engine told them.
    pub fn grid_rows(&self) -> Vec<GridRow> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        doc.grids.iter().map(|grid| GridRow { uid: grid.uid, name: grid.name.clone(), tiles: grid.tiles.len(), active: grid.uid == doc.active_grid }).collect()
    }

    /// Centres the view on a grid's tiles, or on its origin if it has none, and makes it the
    /// grid the tile tools work on.
    pub fn centre_on_grid(&mut self, uid: i32) {
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(centre) = doc.grid_centre(uid) else { return };
        doc.active_grid = uid;
        if let Some(grid) = doc.grid(uid) {
            self.grid = (grid.uid, grid.x, grid.y);
        }
        self.stop_zoom();
        self.camera.x = centre.0;
        self.camera.y = centre.1;
    }

    /// Why grids cannot be added or removed now, if they cannot.
    pub fn grid_edits_refused(&self) -> Option<&'static str> {
        self.doc.as_ref().filter(|doc| doc.is_grid_file()).map(|_| GRID_FILE)
    }

    /// Adds an empty grid with its origin in the middle of the view. With no name typed the
    /// engine names it "grid". The new grid becomes the one the tile tools work on.
    pub fn add_grid(&mut self, typed: &str) {
        if self.doc.is_none() {
            return;
        }
        if let Some(why) = self.grid_edits_refused() {
            return self.set_status(why);
        }
        let mut op = json!({ "kind": "grid.add", "x": self.camera.x, "y": self.camera.y });
        if !typed.trim().is_empty() {
            op["name"] = json!(typed.trim());
        }
        let seq = self.edit(vec![op], "Add grid", None, false);
        self.grid_adding = Some(seq);
    }

    /// Renames a grid. A name is needed.
    pub fn rename_grid(&mut self, uid: i32, typed: &str) {
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(grid) = doc.grid(uid) else { return };
        let name = typed.trim();
        if name.is_empty() {
            return self.set_status("Grid not renamed: give it a name");
        }
        if name == grid.name {
            return;
        }
        self.edit(vec![json!({ "kind": "rename", "uid": uid, "name": name })], "Rename grid", None, false);
    }

    /// What to ask before a grid is removed: its name and uid, and how many entities stand on
    /// it and go with it. None for a grid the mirror does not have.
    pub fn grid_removal_question(&self, uid: i32) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let grid = doc.grid(uid)?;
        let count = match doc.entities_on(uid) {
            0 => "No entities are on it".to_owned(),
            1 => "The 1 entity on it goes too".to_owned(),
            count => format!("The {count} entities on it go too"),
        };
        Some(format!("Remove the grid {} ({uid}) with its {} tiles and {} decals? {count}. Undo brings it all back.", grid.name, grid.tiles.len(), grid.decals.len()))
    }

    /// Removes a grid with all on it.
    pub fn remove_grid(&mut self, uid: i32) {
        if self.doc.as_ref().and_then(|doc| doc.grid(uid)).is_none() {
            return;
        }
        if let Some(why) = self.grid_edits_refused() {
            return self.set_status(why);
        }
        self.edit(vec![json!({ "kind": "grid.remove", "grid": uid })], "Remove grid", None, false);
    }

    /// After the engine has answered: a grid just added becomes the tile tools' grid, and if
    /// their grid is gone they take the one the mirror chose.
    pub(super) fn grids_answered(&mut self, seq: u64, created: Option<i32>) {
        let added = self.grid_adding.filter(|&adding| adding <= seq).is_some();
        if added {
            let ours = self.grid_adding.take() == Some(seq);
            if let (true, Some(uid), Some(doc)) = (ours, created, self.doc.as_mut()) {
                if doc.grid(uid).is_some() {
                    doc.active_grid = uid;
                }
            }
        }
        let Some(doc) = self.doc.as_ref() else { return };
        if added || doc.grid(self.grid.0).is_none() {
            if let Some(grid) = doc.grid(doc.active_grid) {
                self.grid = (grid.uid, grid.x, grid.y);
            }
        }
    }
}
