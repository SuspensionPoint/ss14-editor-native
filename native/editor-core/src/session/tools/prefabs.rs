//! Prefabs: Save as Prefab from the area tool's menu, the Prefabs tab's list of the prefabs
//! folder, and choosing a prefab, which puts it on the clipboard as a region and begins the
//! area tool's paste of it, the same as Ctrl+V. The files themselves are `crate::prefab`'s.

use std::path::{Path, PathBuf};

use super::paste::Clipboard;
use super::*;
use crate::prefab::{self, Listed};

/// What the session keeps of prefabs.
pub(in crate::session) struct Prefabs {
    folder: PathBuf,
    /// The folder's prefabs as last listed. None until they are first asked for.
    listed: Option<Vec<Listed>>,
    /// Whether the interface is to ask for a name to save the region under.
    naming: bool,
    /// The prefab last put on the clipboard, while the clipboard still holds it.
    chosen: Option<String>,
}

impl Default for Prefabs {
    fn default() -> Self {
        Prefabs { folder: prefab::default_folder(), listed: None, naming: false, chosen: None }
    }
}

impl Prefabs {
    /// Save as Prefab was chosen: the interface is to ask for a name.
    pub(super) fn ask_name(&mut self) {
        self.naming = true;
    }
}

impl Session {
    /// The folder prefabs are kept in.
    pub fn prefabs_folder(&self) -> &Path {
        &self.prefabs.folder
    }

    /// Keeps prefabs in another folder. They are listed again when next asked for.
    pub fn set_prefabs_folder(&mut self, folder: PathBuf) {
        self.prefabs.folder = folder;
        self.prefabs.listed = None;
    }

    /// The prefabs in the folder, as last listed, or listed now if they never were.
    pub fn prefabs(&mut self) -> &[Listed] {
        self.prefabs.listed.get_or_insert_with(|| prefab::list(&self.prefabs.folder))
    }

    /// Lists the folder's prefabs again, as the Prefabs tab's refresh does.
    pub fn refresh_prefabs(&mut self) {
        self.prefabs.listed = Some(prefab::list(&self.prefabs.folder));
    }

    /// Where in [`Self::prefabs`] the prefabs are whose name has every word of a search in it.
    pub fn prefabs_matching(&mut self, search: &str) -> Vec<usize> {
        prefab::matching(self.prefabs(), search)
    }

    /// The name of the prefab on the clipboard, if one was chosen and nothing copied since.
    pub fn chosen_prefab(&self) -> Option<&str> {
        self.prefabs.chosen.as_deref()
    }

    /// Whether Save as Prefab was chosen since the last call, so that the interface asks for
    /// a name and hands it to [`Self::save_region_as_prefab`].
    pub fn wants_prefab_name(&mut self) -> bool {
        std::mem::take(&mut self.prefabs.naming)
    }

    /// Saves the area tool's region as a prefab under a name the user typed, which may have
    /// subfolders in it. The region is copied as Ctrl+C copies it, and the file is written
    /// when the clipboard is filled. A name that cannot be a file's or is taken is refused
    /// before anything is asked, and so is a save with no region. The status line says how it went.
    pub fn save_region_as_prefab(&mut self, typed: &str) {
        let name = match prefab::clean_name(typed) {
            Ok(name) => name,
            Err(why) => return self.set_status(format!("Prefab not saved: {why}")),
        };
        if prefab::exists(&self.prefabs.folder, &name) {
            return self.set_status(format!("Prefab {name} not saved: a prefab with that name already exists"));
        }
        if !self.holds_region() {
            return self.set_status(format!("Prefab {name} not saved: no region is selected"));
        }
        self.copy_region_as(false, Some(name));
    }

    /// Writes the region on the clipboard to the prefab of a name, and lists the folder again.
    pub(super) fn save_clipboard_as(&mut self, name: &str) {
        let Some(prefab) = self.clipboard.as_ref().and_then(Clipboard::to_prefab) else { return };
        match prefab::save(&self.prefabs.folder, name, &prefab) {
            Ok(name) => self.set_status(format!("Saved prefab {name}")),
            Err(why) => self.set_status(format!("Prefab {name} not saved: {why}")),
        }
        self.refresh_prefabs();
    }

    /// The clipboard was filled by a copy, so no prefab is on it any more.
    pub(super) fn clipboard_filled(&mut self) {
        self.prefabs.chosen = None;
    }

    /// Puts a prefab of the folder on the clipboard as a region and begins pasting it with the
    /// area tool, unturned, as Ctrl+V does. A file that cannot be read changes nothing, and
    /// the status line says why.
    pub fn choose_prefab(&mut self, name: &str) {
        let prefab = match prefab::load(&self.prefabs.folder, name) {
            Ok(prefab) => prefab,
            Err(why) => return self.set_status(format!("Prefab {name} not read: {why}")),
        };
        let (width, height) = prefab.size();
        self.clipboard = Some(Clipboard::from_prefab(prefab));
        // A paste that was on begins again, unturned.
        self.pasting.end();
        self.begin_paste();
        self.prefabs.chosen = Some(name.to_owned());
        self.set_status(format!("Prefab {name} ({width}x{height}): click to place, R to turn, right-click or Escape to stop"));
    }

    /// Deletes a prefab's file, and lists the folder again. What is on the clipboard stays.
    pub fn delete_prefab(&mut self, name: &str) {
        match prefab::delete(&self.prefabs.folder, name) {
            Ok(()) => self.set_status(format!("Deleted prefab {name}")),
            Err(why) => self.set_status(format!("Prefab {name} not deleted: {why}")),
        }
        self.refresh_prefabs();
    }

    /// Copies a prefab file from a path the user typed into the top of the folder, and lists
    /// the folder again. Quotes around the path are taken off.
    pub fn import_prefab(&mut self, typed: &str) {
        let path = typed.trim().trim_matches('"').trim();
        if path.is_empty() {
            return self.set_status("Prefab not imported: enter a file path");
        }
        match prefab::import(&self.prefabs.folder, Path::new(path)) {
            Ok(name) => self.set_status(format!("Imported prefab {name}")),
            Err(why) => self.set_status(format!("Prefab not imported: {why}")),
        }
        self.refresh_prefabs();
    }
}
