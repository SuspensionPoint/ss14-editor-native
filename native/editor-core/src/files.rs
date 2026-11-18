//! What the system's file dialogs are given: the folder they open in, the name a map is saved
//! under, and the last folder used, which they open in next time.

use std::path::{Path, PathBuf};

/// The folder a kind of file was last chosen in, while the editor runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastFolder(Option<PathBuf>);

impl LastFolder {
    /// The folder a dialog starts in: the last one used, while it is still there, and else
    /// the one given.
    pub fn start_in(&self, otherwise: &Path) -> PathBuf {
        match &self.0 {
            Some(folder) if folder.is_dir() => folder.clone(),
            _ => otherwise.to_path_buf(),
        }
    }

    /// Notes the folder of a file that was chosen.
    pub fn used(&mut self, file: &Path) {
        if let Some(folder) = file.parent().filter(|folder| !folder.as_os_str().is_empty()) {
            self.0 = Some(folder.to_path_buf());
        }
    }
}

/// A map's path as it is saved: one with no extension is given `.yml`. Any other is kept, as
/// the user typed it.
pub fn with_yml(path: &Path) -> PathBuf {
    match path.extension() {
        Some(_) => path.to_path_buf(),
        None => path.with_extension("yml"),
    }
}

/// Where Save As starts and the name it offers: the folder and name of the map's file, or for
/// a map never saved, `untitled.yml` in the folder last used or the maps folder.
pub fn save_as_start(saved_at: Option<&Path>, last: &LastFolder, maps: &Path) -> (PathBuf, String) {
    match saved_at {
        Some(path) => {
            let folder = path.parent().map_or_else(|| last.start_in(maps), Path::to_path_buf);
            let name = path.file_name().map_or("untitled.yml".to_owned(), |name| name.to_string_lossy().into_owned());
            (folder, name)
        }
        None => (last.start_in(maps), "untitled.yml".to_owned()),
    }
}

/// The editor's own folder in the user's data: `%LOCALAPPDATA%` on Windows, Application Support
/// on a Mac, and the XDG data folder elsewhere. The system's temporary folder only when none of
/// those is known.
pub fn user_data() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|home| home.join(".local").join("share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("ss14-editor")
}

/// A path as the engine and the status bar are given it, with forward slashes.
pub fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
#[path = "tests/files.rs"]
mod tests;
