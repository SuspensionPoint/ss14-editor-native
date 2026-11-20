//! The open document as the interface knows it: a mirror of what the engine reported.
//!
//! The engine owns the map. This mirror holds only what the tools need to decide what to ask for
//! (which tile is under the cursor, where an entity is, which tiles a fill would cover) and what
//! the panels show. It is built from the scene the engine reports when a map opens. After that it
//! is updated from what the engine says each edit changed, never from what the edit meant to do.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Deserializer};

use crate::air::AirMirror;
use crate::drawlist::image_file;
use crate::links::LinkMirror;

/// The id of the tile that is no tile.
pub const SPACE: &str = "Space";

// ---- what the engine sends (src/Editor.Kernel/Contract.cs) ----

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SceneJson {
    pub document: DocumentInfo,
    pub grids: Vec<GridJson>,
    pub entities: Vec<Entity>,
    pub history: History,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub id: i32,
    pub source: String,
    pub category: String,
    pub map_uid: i32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GridJson {
    pub uid: i32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub rotation: f64,
    pub tile_ids: Vec<String>,
    /// Five integers a tile: x, y, index into `tile_ids`, variant, rotation and mirroring.
    pub tiles: Vec<i32>,
    #[serde(default)]
    pub decals: Vec<Decal>,
}

/// A decal on a grid, as the game's decal component holds it.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decal {
    /// Its number on its grid. Two grids may each have a decal of the same number.
    pub id: i32,
    pub prototype: String,
    /// Where its lower left corner is, in the grid's own coordinates.
    pub x: f32,
    pub y: f32,
    /// Its tint as #RRGGBBAA. None leaves the prototype's own colours.
    pub color: Option<String>,
    /// How it is turned, in radians.
    pub angle: f64,
    pub z_index: i32,
    pub cleanable: bool,
    /// Where the game draws it among the decals of its z-index, lowest first. It is the id
    /// where a grid keeps its decals itself, and the decal's chunk and its id there where a
    /// fork keeps them on chunk entities, so it can change when the decal moves.
    #[serde(default)]
    pub order: i64,
}

/// A decal that was added or changed, or with no decal, removed.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DecalChange {
    pub grid: i32,
    pub id: i32,
    pub decal: Option<Decal>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub uid: i32,
    pub yaml_uid: i32,
    pub prototype: String,
    pub parent: i32,
    pub x: f32,
    pub y: f32,
    pub rotation: f64,
    pub anchored: bool,
    pub contained: bool,
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TileChange {
    pub grid: i32,
    pub x: i32,
    pub y: i32,
    pub tile: String,
    /// The variant of its picture, and the engine's byte for how it is turned and mirrored.
    /// An older engine sends neither, and they are 0.
    #[serde(default)]
    pub variant: i32,
    #[serde(default)]
    pub rotation_mirroring: i32,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub can_undo: bool,
    pub can_redo: bool,
    pub revision: i32,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub tiles: Vec<TileChange>,
    pub entities: Vec<Entity>,
    pub removed: Vec<i32>,
    pub grids_changed: Vec<GridJson>,
    pub history: History,
    pub created: Vec<i32>,
    #[serde(default)]
    pub decals: Vec<DecalChange>,
    /// The components of each entity whose components the command changed, as they are now.
    /// A delete changes those of the entities left behind that named what was deleted.
    #[serde(default)]
    pub details: Vec<Details>,
    /// The whole set of outgoing links and listed devices of each entity whose links the
    /// command may have changed. Kept in [`Doc::links`].
    #[serde(default)]
    pub linked: Vec<EntityLinks>,
    /// The grids whose stored air the command changed: a region's air carried, a fix of the
    /// air, or the undo or redo of one.
    #[serde(default)]
    pub air: Vec<i32>,
}

/// What one entity links to as a source, and the devices on its own device list.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EntityLinks {
    pub uid: i32,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub devices: Vec<i32>,
}

/// One device link: a port of a source wired to a port of a sink.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub source: i32,
    pub source_port: String,
    pub sink: i32,
    pub sink_port: String,
}

/// One container of an entity, as the engine's `containers` question tells it.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    /// "Container" holds many, "ContainerSlot" holds one.
    pub kind: String,
    /// What it holds, in file order.
    pub entities: Vec<i32>,
    /// Whether something more can go in: an empty slot, or any container that holds many.
    pub can_insert: bool,
}

/// The containers of one entity, empty ones too.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EntityContainers {
    pub uid: i32,
    pub containers: Vec<Container>,
}

/// An entity's component data: a JSON array, as a map file would store it.
#[derive(Deserialize, Debug)]
pub struct Details {
    pub uid: i32,
    pub components: String,
}

/// How long the engine took over a command, in milliseconds.
#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineTiming {
    pub document_ms: f64,
    pub view_ms: f64,
    pub record_ms: f64,
    pub total_ms: f64,
}

/// An entity prototype, for the palette and for what is known of an entity on the map. An
/// engine build from before the kernel told a prototype's description, categories, components
/// or tags leaves them empty.
///
/// Thousands of prototypes have a component, a tag or a category of the same name, so each name
/// is held as a copy that can be shared. [`share_names`] makes it one copy for a whole list.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prototype {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub suffix: Option<String>,
    #[serde(default, deserialize_with = "names")]
    pub categories: Vec<Arc<str>>,
    #[serde(rename = "abstract")]
    pub is_abstract: bool,
    pub hide_spawn_menu: bool,
    pub placement_mode: String,
    /// The names of its components, as map files write them (Sprite, not SpriteComponent).
    #[serde(default, deserialize_with = "names")]
    pub components: Vec<Arc<str>>,
    /// The tags of its Tag component.
    #[serde(default, deserialize_with = "names")]
    pub tags: Vec<Arc<str>>,
    /// Whether its Transform is anchored by default. The engine anchors such a thing as it
    /// spawns, on the middle of its cell. A kernel that does not tell it leaves it false.
    #[serde(default)]
    pub anchored: bool,
    /// The exits of its DisposalTube in the game's order, as the engine's numbers of
    /// directions (0 south, 2 east, 4 north, 6 west), before the entity is turned. None if it is
    /// no disposal tube or the kernel does not tell.
    #[serde(default)]
    pub tube_exits: Option<Vec<u8>>,
    /// The id and the name in lower case, for searching.
    #[serde(skip)]
    pub search: String,
}

impl Prototype {
    /// Whether the game puts this prototype on the middle of a tile when a mapper places it.
    pub fn snaps(&self) -> bool {
        self.placement_mode.starts_with("Snapgrid") || self.placement_mode.starts_with("AlignTile")
    }

    /// Whether the prototype has a component of this name.
    pub fn has(&self, component: &str) -> bool {
        self.components.iter().any(|name| &**name == component)
    }

    /// Whether it is a wall. Forks mark walls one of two ways: the Wall tag, as Starlight and
    /// older forks do, or the Wall component that Space Wizards' master uses in its place.
    pub fn is_wall(&self) -> bool {
        self.tags.iter().any(|tag| &**tag == "Wall") || self.has("Wall")
    }

    /// Whether a mapper can place it: the game's spawn menu lists it.
    pub fn placeable(&self) -> bool {
        !self.is_abstract && !self.hide_spawn_menu
    }
}

/// Reads a list of names, each as a copy of its own.
fn names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Arc<str>>, D::Error> {
    Ok(Vec::<String>::deserialize(deserializer)?.into_iter().map(Arc::from).collect())
}

/// Makes the prototypes that have a component, a tag or a category of the same name hold one
/// copy of that name between them.
pub fn share_names(prototypes: &mut [Prototype]) {
    let mut known: HashSet<Arc<str>> = HashSet::new();
    for prototype in prototypes {
        for name in prototype.categories.iter_mut().chain(&mut prototype.components).chain(&mut prototype.tags) {
            match known.get(&**name) {
                Some(shared) => *name = shared.clone(),
                None => {
                    known.insert(name.clone());
                }
            }
        }
    }
}

/// A decal prototype, for the palette.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecalPrototype {
    pub id: String,
    /// Resource path of the RSI folder, or of the image itself when there is no state. None
    /// if the prototype names no picture.
    pub sprite_path: Option<String>,
    pub sprite_state: Option<String>,
    /// In the prototype's own order: the first is the group a palette shows it under.
    pub tags: Vec<String>,
    /// Whether the game's own decal palette lists it.
    pub show_menu: bool,
    /// Whether it stays upright however its grid is turned.
    pub snap_cardinals: bool,
    /// What the game's decal palette starts with for this decal: taking a colour, going on the
    /// middle of a tile, and being cleanable.
    pub default_custom_color: bool,
    pub default_snap: bool,
    pub default_cleanable: bool,
}

impl DecalPrototype {
    /// The image file its picture is in, as a texture is looked up, if it has a picture.
    pub fn file(&self) -> Option<String> {
        self.sprite_path.as_deref().map(|path| image_file(path, self.sprite_state.as_deref()))
    }
}

// ---- the mirror ----

/// A tile on a grid: its id, as an index into [`Doc::tile_names`], the variant of its
/// picture, and the engine's byte for how it is turned and mirrored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridTile {
    pub id: u32,
    pub variant: u8,
    pub rotation_mirroring: u8,
}

pub struct Grid {
    pub uid: i32,
    pub name: String,
    /// Where the grid's origin is on the map, and how it is turned (radians).
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    /// The tiles by cell. Cells of space are absent.
    pub tiles: HashMap<(i32, i32), GridTile>,
    /// The grid's decals, by id.
    pub decals: HashMap<i32, Decal>,
}

impl Grid {
    /// A map position in the grid's own coordinates.
    pub fn to_local(&self, world: (f32, f32)) -> (f32, f32) {
        let (dx, dy) = (world.0 - self.x, world.1 - self.y);
        let (sin, cos) = (-self.rotation).sin_cos();
        (dx * cos - dy * sin, dx * sin + dy * cos)
    }

    /// A position in the grid's own coordinates, on the map.
    pub fn to_world(&self, local: (f32, f32)) -> (f32, f32) {
        let (sin, cos) = self.rotation.sin_cos();
        (self.x + local.0 * cos - local.1 * sin, self.y + local.0 * sin + local.1 * cos)
    }

    /// A direction on the map, in the grid's own coordinates.
    pub fn direction_to_local(&self, delta: (f32, f32)) -> (f32, f32) {
        let (sin, cos) = (-self.rotation).sin_cos();
        (delta.0 * cos - delta.1 * sin, delta.0 * sin + delta.1 * cos)
    }

    pub fn cell_at(&self, world: (f32, f32)) -> (i32, i32) {
        let local = self.to_local(world);
        (local.0.floor() as i32, local.1.floor() as i32)
    }

    /// The four corners of a rectangle of cells on the map: bottom left, bottom right, top
    /// right, top left of the grid's own axes. `to` is inclusive.
    pub fn cells_outline(&self, from: (i32, i32), to: (i32, i32)) -> [(f32, f32); 4] {
        let (left, right) = (from.0.min(to.0) as f32, from.0.max(to.0) as f32 + 1.0);
        let (bottom, top) = (from.1.min(to.1) as f32, from.1.max(to.1) as f32 + 1.0);
        [self.to_world((left, bottom)), self.to_world((right, bottom)), self.to_world((right, top)), self.to_world((left, top))]
    }
}

pub struct Doc {
    pub id: i32,
    pub source: String,
    /// The file it was opened from or last saved to.
    pub path: Option<PathBuf>,
    /// What kind of file it is: "Map", or "Grid" for a file of exactly one grid.
    pub category: String,
    pub map_uid: i32,
    pub grids: Vec<Grid>,
    pub entities: HashMap<i32, Entity>,
    pub history: History,
    /// The history revision that is on disk.
    pub saved_revision: i32,
    pub tile_names: Vec<String>,
    tile_indices: HashMap<String, u32>,
    /// The grid the tile tools use where no grid has a tile: the last one edited.
    pub active_grid: i32,
    /// Every device link and device list entry, once the engine has listed them. None until
    /// then, or if it would not.
    pub links: Option<LinkMirror>,
    /// The air each grid stores, once the engine has read it. None until then, or if it would not.
    pub air: Option<AirMirror>,
}

impl Doc {
    pub fn from_scene(scene: SceneJson, path: Option<PathBuf>) -> Doc {
        let mut doc = Doc {
            id: scene.document.id,
            source: scene.document.source,
            path,
            category: scene.document.category,
            map_uid: scene.document.map_uid,
            grids: Vec::new(),
            entities: scene.entities.into_iter().map(|entity| (entity.uid, entity)).collect(),
            saved_revision: scene.history.revision,
            history: scene.history,
            tile_names: vec![SPACE.to_owned()],
            tile_indices: HashMap::from([(SPACE.to_owned(), 0)]),
            active_grid: 0,
            links: None,
            air: None,
        };
        for grid in scene.grids {
            doc.put_grid(grid);
        }
        doc.active_grid = doc.grids.iter().max_by_key(|grid| grid.tiles.len()).map_or(0, |grid| grid.uid);
        doc
    }

    pub fn is_dirty(&self) -> bool {
        self.history.revision != self.saved_revision
    }

    pub fn tile_index(&mut self, name: &str) -> u32 {
        if let Some(&index) = self.tile_indices.get(name) {
            return index;
        }
        let index = self.tile_names.len() as u32;
        self.tile_names.push(name.to_owned());
        self.tile_indices.insert(name.to_owned(), index);
        index
    }

    fn put_grid(&mut self, json: GridJson) {
        let indices: Vec<u32> = json.tile_ids.iter().map(|name| self.tile_index(name)).collect();
        let mut tiles = HashMap::with_capacity(json.tiles.len() / 5);
        for tile in json.tiles.chunks_exact(5) {
            let id = indices.get(tile[2] as usize).copied().unwrap_or(0);
            if id != 0 {
                tiles.insert((tile[0], tile[1]), GridTile { id, variant: tile[3] as u8, rotation_mirroring: tile[4] as u8 });
            }
        }
        let decals = json.decals.into_iter().map(|decal| (decal.id, decal)).collect();
        let grid = Grid { uid: json.uid, name: json.name, x: json.x, y: json.y, rotation: json.rotation as f32, tiles, decals };
        match self.grids.iter_mut().find(|known| known.uid == grid.uid) {
            Some(known) => *known = grid,
            None => self.grids.push(grid),
        }
    }

    /// Takes in what the engine says an edit, an undo or a redo changed. Nothing here depends
    /// on the order of `removed`: each uid in it is simply dropped.
    pub fn apply(&mut self, delta: Delta) {
        // A grid is an entity too, and one that is gone takes its tiles and decals with it.
        let removed: HashSet<i32> = delta.removed.iter().copied().collect();
        self.grids.retain(|grid| !removed.contains(&grid.uid));
        for grid in delta.grids_changed {
            self.put_grid(grid);
        }
        for change in delta.tiles {
            let id = self.tile_index(&change.tile);
            if let Some(grid) = self.grids.iter_mut().find(|grid| grid.uid == change.grid) {
                if id == 0 {
                    grid.tiles.remove(&(change.x, change.y));
                } else {
                    let tile = GridTile { id, variant: change.variant as u8, rotation_mirroring: change.rotation_mirroring as u8 };
                    grid.tiles.insert((change.x, change.y), tile);
                }
            }
        }
        for change in delta.decals {
            if let Some(grid) = self.grids.iter_mut().find(|grid| grid.uid == change.grid) {
                match change.decal {
                    Some(decal) => grid.decals.insert(change.id, decal),
                    None => grid.decals.remove(&change.id),
                };
            }
        }
        if let Some(links) = self.links.as_mut() {
            links.apply(&delta.removed, &delta.linked);
        }
        for uid in delta.removed {
            self.entities.remove(&uid);
        }
        for entity in delta.entities {
            self.entities.insert(entity.uid, entity);
        }
        self.history = delta.history;
        // The tile tools' grid went with a removed grid: the largest left takes its place.
        if self.grid(self.active_grid).is_none() {
            self.active_grid = self.grids.iter().max_by_key(|grid| grid.tiles.len()).map_or(0, |grid| grid.uid);
        }
    }

    pub fn grid(&self, uid: i32) -> Option<&Grid> {
        self.grids.iter().find(|grid| grid.uid == uid)
    }

    /// Whether the document is a grid file, which holds exactly one grid.
    pub fn is_grid_file(&self) -> bool {
        self.category == "Grid"
    }

    /// How many entities stand on a grid, through whatever holds them.
    pub fn entities_on(&self, grid: i32) -> usize {
        self.entities.keys().filter(|&&uid| self.place_of(uid).is_some_and(|(on, _)| on == grid)).count()
    }

    /// The middle of the box around a grid's tiles, on the map. A grid with no tiles has its
    /// origin. None for a grid the mirror does not have.
    pub fn grid_centre(&self, uid: i32) -> Option<(f32, f32)> {
        let grid = self.grid(uid)?;
        let mut cells = grid.tiles.keys();
        let Some(&first) = cells.next() else { return Some((grid.x, grid.y)) };
        let (l, b, r, t) = cells.fold((first.0, first.1, first.0, first.1), |(l, b, r, t), &(x, y)| (l.min(x), b.min(y), r.max(x), t.max(y)));
        Some(grid.to_world(((l + r + 1) as f32 / 2.0, (b + t + 1) as f32 / 2.0)))
    }

    /// What an entity holds: the entities inside it, in the order of their uids. It looks
    /// through every entity of the map, so it is for one container at a time.
    pub fn contents(&self, uid: i32) -> Vec<&Entity> {
        let mut held: Vec<&Entity> = self.entities.values().filter(|entity| entity.contained && entity.parent == uid).collect();
        held.sort_unstable_by_key(|entity| entity.uid);
        held
    }

    /// Where an entity is, through whatever holds it: the grid it stands on, or 0 for the map
    /// itself, and its place in that grid's own coordinates. Something inside a locker is
    /// where the locker is. None for an entity the mirror does not have.
    pub fn place_of(&self, uid: i32) -> Option<(i32, (f32, f32))> {
        let mut entity = self.entities.get(&uid)?;
        // A chain of parents is short. The bound keeps a mirror that is wrong from looping.
        for _ in 0..64 {
            if self.grid(entity.parent).is_some() {
                return Some((entity.parent, (entity.x, entity.y)));
            }
            if entity.parent == self.map_uid {
                return Some((0, (entity.x, entity.y)));
            }
            entity = self.entities.get(&entity.parent)?;
        }
        None
    }

    /// Where an entity is on the map, through whatever holds it.
    pub fn world_of(&self, uid: i32) -> Option<(f32, f32)> {
        let (grid, local) = self.place_of(uid)?;
        Some(self.grid(grid).map_or(local, |grid| grid.to_world(local)))
    }

    /// The grid a tool at this map position works on: one with a tile there, or else the
    /// active grid.
    pub fn grid_at(&self, world: (f32, f32)) -> Option<&Grid> {
        self.grids.iter().find(|grid| grid.tiles.contains_key(&grid.cell_at(world))).or_else(|| self.grid(self.active_grid)).or(self.grids.first())
    }

    /// The grid that has a tile at this map position, if any does.
    pub fn grid_with_tile_at(&self, world: (f32, f32)) -> Option<&Grid> {
        self.grids.iter().find(|grid| grid.tiles.contains_key(&grid.cell_at(world)))
    }

    pub fn tile_at(&self, grid: i32, cell: (i32, i32)) -> &str {
        self.grid(grid).and_then(|grid| grid.tiles.get(&cell)).map_or(SPACE, |tile| &self.tile_names[tile.id as usize])
    }

    /// The cells a fill from `start` would change: the connected cells that hold the same tile
    /// as it. A fill of space stays inside the box around the grid's tiles, one cell out, since
    /// space has no edge of its own. None if that is more than `limit` cells.
    pub fn fill_cells(&self, grid: i32, start: (i32, i32), limit: usize) -> Option<Vec<(i32, i32)>> {
        let grid = self.grid(grid)?;
        let wanted = grid.tiles.get(&start).map_or(0, |tile| tile.id);
        let bounds = grid.tiles.keys().fold(None, |bounds: Option<(i32, i32, i32, i32)>, &(x, y)| match bounds {
            None => Some((x, y, x, y)),
            Some((l, b, r, t)) => Some((l.min(x), b.min(y), r.max(x), t.max(y))),
        });
        let (left, bottom, right, top) = bounds.map_or((start.0, start.1, start.0, start.1), |(l, b, r, t)| (l - 1, b - 1, r + 1, t + 1));
        let inside = |cell: (i32, i32)| wanted != 0 || (cell.0 >= left && cell.0 <= right && cell.1 >= bottom && cell.1 <= top);
        if !inside(start) {
            return Some(vec![start]);
        }

        let mut seen = HashSet::from([start]);
        let mut queue = VecDeque::from([start]);
        let mut cells = Vec::new();
        while let Some(cell) = queue.pop_front() {
            cells.push(cell);
            if cells.len() > limit {
                return None;
            }
            for next in [(cell.0 + 1, cell.1), (cell.0 - 1, cell.1), (cell.0, cell.1 + 1), (cell.0, cell.1 - 1)] {
                if inside(next) && grid.tiles.get(&next).map_or(0, |tile| tile.id) == wanted && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        Some(cells)
    }
}

/// The cells on a straight line between two cells, both included, so that a fast stroke of a
/// brush leaves no gaps.
pub fn cells_between(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((to.0 - from.0).abs(), -(to.1 - from.1).abs());
    let (sx, sy) = (if from.0 < to.0 { 1 } else { -1 }, if from.1 < to.1 { 1 } else { -1 });
    let (mut x, mut y, mut error) = (from.0, from.1, dx + dy);
    let mut cells = Vec::new();
    loop {
        cells.push((x, y));
        if (x, y) == to {
            return cells;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;
