//! The editor's settings, a JSON file in the user data folder: the forks it knows, the maps
//! opened last, the fork used last, and each fork's favourites and way of placing walls. Later
//! the keybinds too.
//!
//! A settings file that cannot be read is taken as empty. It is written whole each time, beside
//! its place first, so that a stopped write leaves the old one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::files::{forward_slashes, user_data};

/// How many maps the start screen lists.
pub const RECENT_MAPS: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The forks, the one used last first.
    pub forks: Vec<KnownFork>,
    /// The maps opened or saved last, the last first.
    pub recent_maps: Vec<RecentMap>,
    pub last_fork: Option<String>,
    /// The favourites of each fork, by its folder. A fork given on the command line that is not
    /// in `forks` keeps its own here too.
    pub favourites: Vec<ForkFavourites>,
    /// The forks, by folder, whose maps show the Problems overlay. It is off for the others.
    pub problems_shown: Vec<String>,
    /// How each fork's walls are placed, by its folder: the tile laid under them, the Room
    /// tool's wall and its Intersection switch.
    pub placing: Vec<ForkPlacing>,
}

/// How one fork's walls are placed. A fork with no entry places them as `ForkPlacing::default`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForkPlacing {
    /// The fork's folder, as [`KnownFork::path`].
    pub fork: String,
    /// Whether a brush lays a tile under a wall it puts on a cell with no tile.
    pub floor_under_walls: bool,
    /// The tile it lays there, which the Room tool also lays under its walls.
    pub floor_tile: String,
    /// The wall prototype the Room tool puts on a room's edge, if one was chosen.
    pub room_wall: Option<String>,
    /// Whether the Room tool joins a new room to the rooms it overlaps (its Intersection
    /// switch), in the place of keeping them separate.
    pub room_intersection: bool,
}

/// The tile laid under walls until a mapper picks another.
pub const PLATING: &str = "Plating";

impl Default for ForkPlacing {
    fn default() -> Self {
        ForkPlacing { fork: String::new(), floor_under_walls: true, floor_tile: PLATING.to_owned(), room_wall: None, room_intersection: false }
    }
}

/// What a favourite is: a tile, an entity or a decal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FavouriteKind {
    Tile,
    Entity,
    Decal,
}

impl FavouriteKind {
    /// The kind in a word, as a favourite's row labels it.
    pub fn word(self) -> &'static str {
        match self {
            FavouriteKind::Tile => "Tile",
            FavouriteKind::Entity => "Entity",
            FavouriteKind::Decal => "Decal",
        }
    }
}

/// A tile, entity or decal a mapper keeps at hand, by its prototype's id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Favourite {
    pub kind: FavouriteKind,
    pub id: String,
}

impl Favourite {
    pub fn new(kind: FavouriteKind, id: impl Into<String>) -> Favourite {
        Favourite { kind, id: id.into() }
    }
}

/// One fork's favourites, in the order the mapper put them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForkFavourites {
    /// The fork's folder, as [`KnownFork::path`].
    pub fork: String,
    pub items: Vec<Favourite>,
}

/// A fork checkout the editor has been given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KnownFork {
    /// Its folder, with forward slashes.
    pub path: String,
    pub name: String,
    /// The folder its maps were last opened from or saved to. None is its `Resources/Maps`.
    pub maps_folder: Option<String>,
    pub last_map: Option<String>,
    /// The keys of the pack and textures it was last opened with (`Fingerprint::key`).
    pub prototypes: Option<String>,
    pub textures: Option<String>,
    /// The key of the engine side built for it that it last opened with, if it needed one
    /// (`engines::key`).
    pub engine: Option<String>,
    /// Whether the editor downloaded it into its own forks folder, which gives it Update and
    /// lets Remove delete it. A mapper's own checkout is never changed.
    pub managed: bool,
    /// Where it was downloaded from, for a managed fork.
    pub url: Option<String>,
}

impl KnownFork {
    /// The folder the open dialog starts in for this fork.
    pub fn maps_folder(&self) -> PathBuf {
        match &self.maps_folder {
            Some(folder) => PathBuf::from(folder),
            None => Path::new(&self.path).join("Resources").join("Maps"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentMap {
    /// The fork it was opened in, as [`KnownFork::path`].
    pub fork: String,
    pub map: String,
}

impl Settings {
    /// Where the settings are kept.
    pub fn file() -> PathBuf {
        user_data().join("settings.json")
    }

    /// The settings in a file, or none if it is missing or cannot be read.
    pub fn load(file: &Path) -> Settings {
        std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Changes the settings as they are in the file now, and saves them: what another part of
    /// the editor, or another of its windows, wrote since a copy was read is kept. A file that
    /// is there but cannot be read is moved aside, not written over. Gives the settings saved.
    pub fn update(file: &Path, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut settings = match std::fs::read_to_string(file) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => settings,
                Err(_) => {
                    let aside = file.with_extension("json.unreadable");
                    std::fs::rename(file, &aside).map_err(|error| format!("couldn't move the unreadable {} out of the way: {error}", file.display()))?;
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        change(&mut settings);
        settings.save(file)?;
        Ok(settings)
    }

    pub fn save(&self, file: &Path) -> Result<(), String> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let partial = file.with_extension("json.partial");
        std::fs::write(&partial, text).map_err(|error| format!("couldn't write {}: {error}", partial.display()))?;
        std::fs::rename(&partial, file).map_err(|error| format!("couldn't write {}: {error}", file.display()))
    }

    pub fn fork(&self, path: &str) -> Option<&KnownFork> {
        self.forks.iter().find(|fork| same_path(&fork.path, path))
    }

    /// Adds a fork, named for its folder, at the end of the list. One already known stays as it is.
    pub fn add_fork(&mut self, path: &Path) -> &mut KnownFork {
        let path = forward_slashes(path);
        let at = match self.forks.iter().position(|fork| same_path(&fork.path, &path)) {
            Some(at) => at,
            None => {
                let name = super::default_name(Path::new(&path));
                self.forks.push(KnownFork { path, name, ..KnownFork::default() });
                self.forks.len() - 1
            }
        };
        &mut self.forks[at]
    }

    /// Adds a fork the editor downloaded, marked as managed by it, named as given.
    pub fn add_downloaded(&mut self, path: &Path, url: &str, name: &str) -> &mut KnownFork {
        let fork = self.add_fork(path);
        fork.managed = true;
        fork.url = Some(url.to_owned());
        fork.name = name.to_owned();
        fork
    }

    /// Takes a fork off the list, with its recent maps. Its files are not touched.
    pub fn remove_fork(&mut self, path: &str) {
        self.forks.retain(|fork| !same_path(&fork.path, path));
        self.recent_maps.retain(|map| !same_path(&map.fork, path));
        self.favourites.retain(|kept| !same_path(&kept.fork, path));
        self.problems_shown.retain(|shown| !same_path(shown, path));
        self.placing.retain(|kept| !same_path(&kept.fork, path));
        if self.last_fork.as_deref().is_some_and(|last| same_path(last, path)) {
            self.last_fork = None;
        }
    }

    /// A fork was opened, with the keys of its pack and textures: it goes first, and is the last fork.
    pub fn used_fork(&mut self, path: &Path, prototypes: Option<String>, textures: Option<String>) {
        let fork = self.add_fork(path).clone();
        let fork = KnownFork { prototypes: prototypes.or(fork.prototypes), textures: textures.or(fork.textures), ..fork };
        self.last_fork = Some(fork.path.clone());
        self.forks.retain(|known| !same_path(&known.path, &fork.path));
        self.forks.insert(0, fork);
    }

    /// A map of a fork was opened or saved: it is the fork's last map, its folder the fork's
    /// maps folder, and it goes first in the recent maps.
    pub fn opened_map(&mut self, fork: &Path, map: &Path) {
        let map_text = forward_slashes(map);
        let known = self.add_fork(fork);
        known.last_map = Some(map_text.clone());
        if let Some(folder) = map.parent().filter(|folder| !folder.as_os_str().is_empty()) {
            known.maps_folder = Some(forward_slashes(folder));
        }
        let fork = known.path.clone();
        self.recent_maps.retain(|recent| !same_path(&recent.map, &map_text));
        self.recent_maps.insert(0, RecentMap { fork, map: map_text });
        self.recent_maps.truncate(RECENT_MAPS);
    }

    /// A fork opened with an engine side built for it, by its key, or with the editor's own (None).
    pub fn used_engine(&mut self, path: &Path, engine: Option<String>) {
        self.add_fork(path).engine = engine;
    }

    /// A fork's favourites, by its folder. None kept is an empty list.
    pub fn favourites(&self, fork: &str) -> &[Favourite] {
        self.favourites.iter().find(|kept| same_path(&kept.fork, fork)).map_or(&[], |kept| &kept.items[..])
    }

    /// Keeps a fork's favourites in the place of those it had. An empty list takes its entry away.
    pub fn set_favourites(&mut self, fork: &str, items: Vec<Favourite>) {
        match self.favourites.iter_mut().find(|kept| same_path(&kept.fork, fork)) {
            Some(kept) if !items.is_empty() => kept.items = items,
            Some(_) => self.favourites.retain(|kept| !same_path(&kept.fork, fork)),
            None if !items.is_empty() => self.favourites.push(ForkFavourites { fork: forward_slashes(Path::new(fork)), items }),
            None => {}
        }
    }

    /// Whether a fork's maps show the Problems overlay.
    pub fn problems_shown(&self, fork: &str) -> bool {
        self.problems_shown.iter().any(|shown| same_path(shown, fork))
    }

    /// Turns the Problems overlay on or off for a fork's maps.
    pub fn show_problems(&mut self, fork: &str, on: bool) {
        self.problems_shown.retain(|shown| !same_path(shown, fork));
        if on {
            self.problems_shown.push(forward_slashes(Path::new(fork)));
        }
    }

    /// How a fork's walls are placed, by its folder. A fork with none kept has the defaults.
    pub fn placing(&self, fork: &str) -> ForkPlacing {
        let kept = self.placing.iter().find(|kept| same_path(&kept.fork, fork)).cloned();
        ForkPlacing { fork: forward_slashes(Path::new(fork)), ..kept.unwrap_or_default() }
    }

    /// Keeps how a fork's walls are placed, in the place of what it had.
    pub fn set_placing(&mut self, placing: ForkPlacing) {
        self.placing.retain(|kept| !same_path(&kept.fork, &placing.fork));
        self.placing.push(placing);
    }

    /// The keys of the packs, textures and engine sides the known forks last used, which the
    /// cache keeps.
    pub fn cached_keys(&self) -> Vec<String> {
        self.forks.iter().flat_map(|fork| [fork.prototypes.clone(), fork.textures.clone(), fork.engine.clone()]).flatten().collect()
    }
}

/// Whether two paths name the same folder or file: with either slash, with or without one at
/// the end, and on Windows in either case.
pub fn same_path(a: &str, b: &str) -> bool {
    let tidy = |path: &str| {
        let path = path.replace('\\', "/");
        let path = path.trim_end_matches('/').to_owned();
        if cfg!(windows) {
            path.to_lowercase()
        } else {
            path
        }
    };
    tidy(a) == tidy(b)
}

#[cfg(test)]
#[path = "../tests/forks/settings.rs"]
mod tests;
