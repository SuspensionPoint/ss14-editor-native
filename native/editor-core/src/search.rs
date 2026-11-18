//! Finds entities on the open map by what they are, like the web editor's search box (Ctrl+F).
//! It reads only the mirror and never asks the engine.

use crate::model::Doc;

/// The most results a search gives, as in the web editor.
pub const MOST_FOUND: usize = 200;

/// An entity a search found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub uid: i32,
    /// Its number in the map file, or 0 for one made since the map was opened.
    pub yaml_uid: i32,
    pub prototype: String,
    /// The name it shows: its own, or its prototype's, or else the prototype's id.
    pub name: String,
    /// The grid it stands on, or 0 for the map itself, and the cell there.
    pub grid: i32,
    pub cell: (i32, i32),
    /// Where it is on the map, which a jump centres on.
    pub world: (f32, f32),
}

/// The entities whose prototype id or name has the query in it, in either case, as the web
/// editor matches them. A query that is a number, with or without a "#" before it, also finds
/// the entity of that uid, in the engine or in the file. The entity's own name counts as well
/// as its prototype's. `name_of` gives a prototype's name by its id. They come in the order of
/// their uids, at most [`MOST_FOUND`]. An empty query finds nothing. Something inside a locker
/// is found where the locker is.
pub fn find_entities<'a>(doc: &Doc, query: &str, name_of: impl Fn(&str) -> Option<&'a str>) -> Vec<Found> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let wanted = query.to_lowercase();
    let number: Option<i32> = query.strip_prefix('#').unwrap_or(query).parse().ok();

    let mut uids: Vec<i32> = doc.entities.keys().copied().collect();
    uids.sort_unstable();
    let mut found = Vec::new();
    for uid in uids {
        if found.len() >= MOST_FOUND {
            break;
        }
        let entity = &doc.entities[&uid];
        let own = entity.name.as_deref().filter(|name| !name.is_empty());
        let prototype_name = name_of(&entity.prototype).filter(|name| !name.is_empty());
        let matches = entity.prototype.to_lowercase().contains(&wanted)
            || own.is_some_and(|name| name.to_lowercase().contains(&wanted))
            || prototype_name.is_some_and(|name| name.to_lowercase().contains(&wanted))
            || number.is_some_and(|number| number == entity.uid || (number != 0 && number == entity.yaml_uid));
        if !matches {
            continue;
        }
        let Some((grid, local)) = doc.place_of(uid) else { continue };
        let Some(world) = doc.world_of(uid) else { continue };
        found.push(Found {
            uid,
            yaml_uid: entity.yaml_uid,
            prototype: entity.prototype.clone(),
            name: own.or(prototype_name).unwrap_or(&entity.prototype).to_owned(),
            grid,
            cell: (local.0.floor() as i32, local.1.floor() as i32),
            world,
        });
    }
    found
}

#[cfg(test)]
#[path = "tests/search.rs"]
mod tests;
