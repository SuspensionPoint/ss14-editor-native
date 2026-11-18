//! The order the palettes list things in, matching the web editor. Common tiles come first.
//! Entities sit in a tree of categories taken from the folders their prototypes live in, with the
//! common categories first.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::model::Prototype;
use crate::session::{shown_name, Tile};

/// The tiles the web editor lists first, in this order, when the fork has them.
pub const COMMON_TILES: [&str; 8] = ["Plating", "FloorSteel", "FloorDark", "FloorWhite", "FloorWood", "WallSolid", "WallReinforced", "Lattice"];

/// The categories the web editor lists first, in this order. The rest follow in the order of
/// their names.
pub const COMMON_CATEGORIES: [&str; 10] = [
    "Structures",
    "Structures/Doors",
    "Structures/Machines",
    "Structures/Power",
    "Structures/Piping",
    "Structures/Wallmounts",
    "Structures/Lighting",
    "Structures/Storage",
    "Structures/Windows",
    "Markers",
];

/// The tiles as the palette lists them, as indices into `tiles`: the common ones first, then
/// the rest by their ids.
pub fn tile_order(tiles: &[Tile]) -> Vec<usize> {
    let mut common: Vec<usize> = COMMON_TILES.iter().filter_map(|id| tiles.iter().position(|tile| tile.id == *id)).collect();
    let mut rest: Vec<usize> = (0..tiles.len()).filter(|index| !COMMON_TILES.contains(&tiles[*index].id.as_str())).collect();
    rest.sort_by(|a, b| tiles[*a].id.cmp(&tiles[*b].id));
    common.append(&mut rest);
    common
}

/// The category the web editor gives the entities of a prototype file, from its path inside
/// the fork's Resources folder: the folders under `Prototypes/Entities`, or under
/// `Prototypes/Catalog` with `Catalog/` before them, either of which may be inside a fork's
/// own folder whose name begins with `_`. Any other file is "Other".
pub fn category_of(path: &str) -> String {
    let path = path.replace('\\', "/");
    let Some(at) = path.find("/Prototypes/").map(|at| at + "/Prototypes/".len()).or_else(|| path.strip_prefix("Prototypes/").map(|_| "Prototypes/".len())) else {
        return "Other".to_owned();
    };
    let mut parts: Vec<&str> = path[at..].split('/').collect();
    if parts.len() > 1 && parts[0].starts_with('_') && matches!(parts[1], "Entities" | "Catalog") {
        parts.remove(0);
    }
    // The root, at least one folder, and the file.
    if parts.len() < 3 {
        return "Other".to_owned();
    }
    let folders = parts[1..parts.len() - 1].join("/");
    match parts[0] {
        "Entities" => folders,
        "Catalog" => format!("Catalog/{folders}"),
        _ => "Other".to_owned(),
    }
}

/// The ids of the entity prototypes a prototype file writes, in its order. It reads only the
/// keys of each item of the file's top-level list, which is how the game's files are laid out.
pub fn entity_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let (mut kind, mut id): (Option<String>, Option<String>) = (None, None);
    let mut finish = |kind: &mut Option<String>, id: &mut Option<String>| {
        if let (Some("entity"), Some(found)) = (kind.as_deref(), id.take()) {
            ids.push(found);
        }
        *kind = None;
    };
    for line in text.lines() {
        let key_value = if let Some(rest) = line.strip_prefix("- ") {
            finish(&mut kind, &mut id);
            rest
        } else if let Some(rest) = line.strip_prefix("  ") {
            if rest.starts_with([' ', '-', '#']) {
                continue;
            }
            rest
        } else {
            if !line.is_empty() && !line.starts_with([' ', '#']) {
                finish(&mut kind, &mut id);
            }
            continue;
        };
        let Some((key, value)) = key_value.split_once(':') else { continue };
        let value = value.split(" #").next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'');
        match key.trim() {
            "type" => kind = Some(value.to_owned()),
            "id" if !value.is_empty() => id = Some(value.to_owned()),
            _ => {}
        }
    }
    finish(&mut kind, &mut id);
    ids
}

/// Reads the fork's prototype files for the category of each entity prototype, by its id. Only
/// files under an `Entities` or a `Catalog` folder are read, since any other is "Other". The
/// first file to write an id gives its category.
pub fn scan_categories(fork_repo: &Path) -> HashMap<String, String> {
    let resources = fork_repo.join("Resources");
    let mut categories = HashMap::new();
    let mut pending = vec![resources.join("Prototypes")];
    let mut files = Vec::new();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("yml")) {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        let Ok(relative) = path.strip_prefix(&resources) else { continue };
        let category = category_of(&relative.to_string_lossy());
        if category == "Other" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for id in entity_ids(&text) {
            categories.entry(id).or_insert_with(|| category.clone());
        }
    }
    categories
}

/// One category of the entity palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    /// Its path, as "Structures/Power".
    pub path: String,
    /// Its prototypes, as indices into the session's list, by name.
    pub entries: Vec<usize>,
}

impl Category {
    /// The last part of its path, which the web editor shows it by.
    pub fn short_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// One row of the entity palette as it is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TreeRow {
    /// The header of a category, by its index in the tree, and whether it is open.
    Category(usize, bool),
    /// A prototype, by its index in the session's list.
    Entity(usize),
}

/// The entity palette's tree: every prototype a mapper can place, in the category of the
/// folder it is written in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityTree {
    pub categories: Vec<Category>,
}

/// Puts prototypes in the order of their shown names, in either case, and of their ids.
fn by_name(prototypes: &[Prototype], indices: &mut [usize]) {
    indices.sort_by_cached_key(|&index| (shown_name(&prototypes[index]).to_lowercase(), prototypes[index].id.clone()));
}

impl EntityTree {
    /// The tree of the prototypes that can be placed, given the category of each by its id.
    /// One whose file was not read is "Other". The common categories come first, then the
    /// rest by their paths.
    pub fn build(prototypes: &[Prototype], categories: &HashMap<String, String>) -> EntityTree {
        let mut grouped: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, prototype) in prototypes.iter().enumerate().filter(|(_, prototype)| prototype.placeable()) {
            let category = categories.get(&prototype.id).map_or("Other", String::as_str);
            grouped.entry(category).or_default().push(index);
        }
        let mut paths: Vec<&str> = grouped.keys().copied().collect();
        paths.sort_by_key(|path| (COMMON_CATEGORIES.iter().position(|common| common == path).unwrap_or(COMMON_CATEGORIES.len()), *path));
        let categories = paths
            .into_iter()
            .map(|path| {
                let mut entries = grouped.remove(path).unwrap_or_default();
                by_name(prototypes, &mut entries);
                Category { path: path.to_owned(), entries }
            })
            .collect();
        EntityTree { categories }
    }

    /// The category a prototype is in, by its index in the tree.
    pub fn category_holding(&self, prototype: usize) -> Option<usize> {
        self.categories.iter().position(|category| category.entries.contains(&prototype))
    }

    /// The rows shown with these categories open, by their paths.
    pub fn rows(&self, open: &HashSet<String>) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        for (index, category) in self.categories.iter().enumerate() {
            let expanded = open.contains(&category.path);
            rows.push(TreeRow::Category(index, expanded));
            if expanded {
                rows.extend(category.entries.iter().map(|&entry| TreeRow::Entity(entry)));
            }
        }
        rows
    }

    /// A search over the whole tree: the prototypes whose id, name or suffix has every one of
    /// the words in it, in either case, in the order of their names, as one list.
    pub fn search(&self, prototypes: &[Prototype], words: &[&str]) -> Vec<usize> {
        let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
        let mut found: Vec<usize> = self
            .categories
            .iter()
            .flat_map(|category| category.entries.iter().copied())
            .filter(|&index| {
                let prototype = &prototypes[index];
                let text = format!("{} {} {}", prototype.id, prototype.name, prototype.suffix.as_deref().unwrap_or("")).to_lowercase();
                words.iter().all(|word| text.contains(word.as_str()))
            })
            .collect();
        by_name(prototypes, &mut found);
        found
    }
}

#[cfg(test)]
#[path = "tests/palette.rs"]
mod tests;
