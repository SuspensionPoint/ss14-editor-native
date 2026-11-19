//! Checks the open map for faults, like the web editor's Validate Map. Each rule is a function of
//! the mirror and of what the engine reported about the prototypes, and none of them asks the
//! engine anything.
//!
//! All five of the web editor's rules are here. Three of them read device links and lists: a link
//! or list entry that points at nothing, and an air alarm or fire alarm with no devices. They use
//! the links mirror (`Doc::links`) and wait until the engine has listed the links. Two rules are
//! new compared to the web editor. One flags a decal whose prototype the fork doesn't define, or
//! that names none. The other flags a room open to space, using the rooms the air overlay finds
//! (`crate::rooms`), once the engine has read the air. Three more read the stored air of sealed
//! rooms and walls (`crate::air_checks`): uneven air in a room, floors with no air, and air in a
//! wall. Another finds a grid of floors with no atmosphere, which Fix grid atmos can give it. The
//! last finds open ends of disposal pipes, where the game dumps trash and tears up the floor.
//!
//! Three more come from mappers. Stacked walls: two walls, windows or doors on one cell. Duplicates:
//! the same thing placed twice on one cell. Walls with no floor: a wall, window or door on a cell
//! with no tile, or off every grid, which the game leaves loose. What counts as a wall, a window
//! or a door is [`blocker`]. These three and the two of floors under walls and doors are cheap,
//! and the Problems overlay runs them again after every edit ([`quick`]).

use std::collections::{HashMap, HashSet};

use crate::air::{room_text, AirMirror, GridAir};
use crate::air_checks::RoomFaults;
use crate::links::{alarm_of, Alarm, LinkMirror};
use crate::model::{Doc, Entity, Prototype, SPACE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    FloorUnderWall,
    DoorWithoutFloor,
    WallWithoutFloor,
    StackedWalls,
    Duplicates,
    DanglingLink,
    UnlinkedAirAlarm,
    UnlinkedFireAlarm,
    UnknownDecal,
    OpenRoom,
    NoAtmosphere,
    UnfilledFloor,
    UnevenAir,
    AirInWall,
    OpenDisposals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    /// Worth knowing, and harmless in the game.
    Notice,
}

impl Rule {
    /// Every rule, in the order the results list them.
    pub const ALL: [Rule; 15] = [
        Rule::FloorUnderWall,
        Rule::DoorWithoutFloor,
        Rule::WallWithoutFloor,
        Rule::StackedWalls,
        Rule::Duplicates,
        Rule::DanglingLink,
        Rule::UnlinkedAirAlarm,
        Rule::UnlinkedFireAlarm,
        Rule::UnknownDecal,
        Rule::OpenRoom,
        Rule::NoAtmosphere,
        Rule::UnfilledFloor,
        Rule::UnevenAir,
        Rule::AirInWall,
        Rule::OpenDisposals,
    ];

    /// Its heading, the web editor's where it has the rule.
    pub fn label(self) -> &'static str {
        match self {
            Rule::FloorUnderWall => "Floor tiles under walls",
            Rule::DoorWithoutFloor => "Doors without floor tiles",
            Rule::WallWithoutFloor => "Walls with no floor",
            Rule::StackedWalls => "Stacked walls",
            Rule::Duplicates => "Duplicates",
            Rule::DanglingLink => "Dangling device references",
            Rule::UnlinkedAirAlarm => "Unlinked air alarms",
            Rule::UnlinkedFireAlarm => "Unlinked fire alarms",
            Rule::UnknownDecal => "Decals with unknown prototypes",
            Rule::OpenRoom => "Rooms open to space",
            Rule::NoAtmosphere => "Grids with no atmosphere",
            Rule::UnfilledFloor => "Floors with no air",
            Rule::UnevenAir => "Uneven air in rooms",
            Rule::AirInWall => "Air in walls",
            Rule::OpenDisposals => "Open disposal ends",
        }
    }

    /// The web editor's: a reference to nothing is an error, the rest are warnings. Air in a
    /// wall is only a notice, since the game clears it.
    pub fn severity(self) -> Severity {
        match self {
            Rule::DanglingLink => Severity::Error,
            Rule::FloorUnderWall
            | Rule::DoorWithoutFloor
            | Rule::WallWithoutFloor
            | Rule::StackedWalls
            | Rule::Duplicates
            | Rule::UnlinkedAirAlarm
            | Rule::UnlinkedFireAlarm
            | Rule::UnknownDecal
            | Rule::OpenRoom
            | Rule::NoAtmosphere
            | Rule::UnfilledFloor
            | Rule::UnevenAir
            | Rule::OpenDisposals => Severity::Warning,
            Rule::AirInWall => Severity::Notice,
        }
    }
}

/// A room open to the outside with fewer tiles than this is not found. Bagel has 34 of one to
/// three tiles: a girder in the hull, a notch in an outer wall, a conveyor's tile outside a
/// blast door. Their air is a few tiles' worth and they matter to no one.
pub const SMALLEST_ROOM: usize = 4;

/// The rules that are not checked while the engine has not listed the links, and what each needs.
pub const NOT_CHECKED: &[(&str, &str)] = &[
    ("Dangling device references", "the device lists and links of every entity"),
    ("Unlinked air alarms", "the device list of every air alarm"),
    ("Unlinked fire alarms", "the device list of every fire alarm"),
];

/// A fault one rule found, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: Rule,
    pub message: String,
    /// The grid it is on, or 0 for the map itself, and the cell there.
    pub grid: i32,
    pub cell: (i32, i32),
    /// The entity it is about, if it is about one.
    pub entity: Option<i32>,
    /// The decal it is about, by its id on its grid, if it is about one.
    pub decal: Option<i32>,
}

impl Finding {
    /// The grid a fix of the air would mend this finding on, if it would: floors with no air
    /// get standard air from Fix grid atmos (`Session::fix_grid_air`), and a grid with no
    /// atmosphere is given one first.
    pub fn fixes_air(&self) -> Option<i32> {
        matches!(self.rule, Rule::UnfilledFloor | Rule::NoAtmosphere).then_some(self.grid)
    }

    /// What Validate Map's button on this finding does, if it has one.
    pub fn fix(&self) -> Option<Fix> {
        match self.rule {
            Rule::UnfilledFloor | Rule::NoAtmosphere => Some(Fix::Air(self.grid)),
            Rule::StackedWalls | Rule::Duplicates => Some(Fix::RemoveExtras),
            Rule::WallWithoutFloor if self.grid != 0 => Some(Fix::LayPlating),
            Rule::WallWithoutFloor => self.entity.map(Fix::Select),
            _ => None,
        }
    }
}

/// What a finding's button does in Validate Map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Fix grid atmos on this grid.
    Air(i32),
    /// Delete all but one of what is stacked on the cell (`extras`).
    RemoveExtras,
    /// Lay plating on the cell.
    LayPlating,
    /// Select this entity. Something off every grid has no tile to lay.
    Select(i32),
}

impl Fix {
    /// The button's words.
    pub fn label(self) -> &'static str {
        match self {
            Fix::Air(_) => "Fix",
            Fix::RemoveExtras => "Remove extras",
            Fix::LayPlating => "Lay plating",
            Fix::Select(_) => "Select",
        }
    }

    /// What the button does, for its tooltip.
    pub fn does(self) -> &'static str {
        match self {
            Fix::Air(_) => "Run Fix grid atmos on this grid. Every floor gets standard station air, and one undo reverts it.",
            Fix::RemoveExtras => "Delete the extras and keep one. One undo puts them back.",
            Fix::LayPlating => "Put plating under it and anchor it. One undo takes both away.",
            Fix::Select(_) => "Select it on the map. It is on no grid, so there is no floor to lay.",
        }
    }
}

/// What a wall, window or door is, as the rules of walls tell them ([`blocker`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocker {
    Wall,
    Window,
    Door,
}

/// Whether a prototype is a wall, a window or a door that fills its whole cell, and which.
/// Two of these on one cell are always a mistake. Each is airtight on all four sides in the
/// game.
///
/// - A wall has the Wall tag.
/// - A window has the Window tag and the Airtight component. A directional window is only a
///   pane on one side, and a cell may hold several: it has the DirectionalWindow tag, or
///   Directional in its id (Starlight's brass one has no tag).
/// - A door has the Door and Airtight components. Three kinds are left out, since the game puts
///   them on the cell of a window or a door on purpose, or on one side of a cell. A firelock
///   (the Firelock component) goes on a door's cell. Shutters and blast doors go over windows
///   and doors, and are told by Shutters or BlastDoor in their ids. A windoor is a thin door
///   on one side of its cell, with Windoor in its id.
///
/// Anything with the Diagonal tag fills only half its cell, and is left out. A grille, a girder
/// or a table is none of these, so a window on a grille is no fault.
pub fn blocker(prototype: &Prototype) -> Option<Blocker> {
    let tagged = |tag: &str| prototype.tags.iter().any(|known| &**known == tag);
    let named = |part: &str| prototype.id.contains(part);
    if tagged("Diagonal") {
        return None;
    }
    if prototype.is_wall() {
        return Some(Blocker::Wall);
    }
    if tagged("Window") && !tagged("DirectionalWindow") && !named("Directional") && prototype.has("Airtight") {
        return Some(Blocker::Window);
    }
    let cover = prototype.has("Firelock") || named("Shutters") || named("BlastDoor") || named("Windoor");
    if prototype.has("Door") && prototype.has("Airtight") && !cover {
        return Some(Blocker::Door);
    }
    None
}

/// Whether the game stacks entities of a prototype on one cell on purpose: a marker or a
/// spawner. Such a prototype has the Marker component, a component whose name ends in Spawner
/// (EntityTableSpawner, RandomSpawner, ConditionalSpawner and the like), or a SpawnPoint. The
/// engine reports only the server's components, and Marker is the client's, so the spawners
/// are told by their own.
fn stacks_on_purpose(prototype: &Prototype) -> bool {
    prototype.components.iter().any(|name| &**name == "Marker" || &**name == "SpawnPoint" || name.ends_with("Spawner"))
}

/// The cells of grids with two or more entities anchored on them: the grid, the cell, and
/// those entities by uid. By grid, then row, then column.
type Cells<'d> = Vec<(i32, (i32, i32), Vec<&'d Entity>)>;

/// The cells of grids with two or more entities anchored on them, of those standing
/// ([`standing`]). A sort and one pass, which is quicker on a station than a map of every cell.
fn anchored_cells<'d>(standing: &[Standing<'d>]) -> Cells<'d> {
    let mut anchored: Vec<&Standing<'d>> = standing.iter().filter(|(entity, grid, _)| *grid != 0 && entity.anchored).collect();
    // Standing is in the order of uids, and a stable sort keeps it within a cell.
    anchored.sort_by_key(|(_, grid, cell)| (*grid, cell.1, cell.0));
    let mut cells: Cells<'d> = Vec::new();
    for group in anchored.chunk_by(|a, b| (a.1, a.2) == (b.1, b.2)) {
        if group.len() >= 2 {
            cells.push((group[0].1, group[0].2, group.iter().map(|(entity, ..)| *entity).collect()));
        }
    }
    cells
}

/// A list of names as a sentence has them: "a", "a and b", "a, b and c".
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The groups of each cell that the stacked walls rule finds: the walls, windows and doors
/// anchored on one cell of a grid, by uid, where there are two or more. By grid, then cell.
fn stacked_groups<'a, 'd>(cells: &Cells<'d>, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Cells<'d> {
    cells
        .iter()
        .filter_map(|(grid, cell, entities)| {
            let walls: Vec<&Entity> = entities.iter().copied().filter(|entity| prototype(&entity.prototype).and_then(blocker).is_some()).collect();
            (walls.len() >= 2).then_some((*grid, *cell, walls))
        })
        .collect()
}

/// Two or more walls, windows or doors ([`blocker`]) anchored on one cell of a grid. A wall
/// on a wall, a window in a wall or a door in a window. Found at the cell, about the first of
/// them by uid, which a fix keeps. By grid, then row, then column.
pub fn stacked_walls<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    stacked_walls_in(&anchored_cells(&standing(doc)), prototype)
}

fn stacked_walls_in<'a>(cells: &Cells, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    stacked_groups(cells, prototype)
        .into_iter()
        .map(|(grid, cell, walls)| {
            let names: Vec<&str> = walls.iter().map(|entity| entity.prototype.as_str()).collect();
            let message = format!("{} are stacked at ({}, {}). One cell holds one wall, window or door.", and_list(&names), cell.0, cell.1);
            Finding { rule: Rule::StackedWalls, message, grid, cell, entity: Some(walls[0].uid), decal: None }
        })
        .collect()
}

/// How an entity is turned, in whole quarter turns from 0 to 3.
fn quarters(rotation: f64) -> i64 {
    (rotation / std::f64::consts::FRAC_PI_2).round().rem_euclid(4.0) as i64
}

/// The groups the duplicates rule finds: entities of one prototype anchored on one cell of a
/// grid, turned the same way, two or more, by uid. By grid, cell, then the first uid.
fn duplicate_groups<'a, 'd>(cells: &Cells<'d>, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Cells<'d> {
    let mut groups: Cells<'d> = Vec::new();
    for (grid, cell, entities) in cells {
        let mut same: Vec<((&str, i64), Vec<&Entity>)> = Vec::new();
        for &entity in entities {
            if prototype(&entity.prototype).is_some_and(stacks_on_purpose) {
                continue;
            }
            let key = (entity.prototype.as_str(), quarters(entity.rotation));
            match same.iter_mut().find(|(known, _)| *known == key) {
                Some((_, list)) => list.push(entity),
                None => same.push((key, vec![entity])),
            }
        }
        // In the order of their first uids, as the entities of a cell are.
        groups.extend(same.into_iter().filter(|(_, list)| list.len() >= 2).map(|(_, list)| (*grid, *cell, list)));
    }
    groups
}

/// The same prototype anchored two or more times on one cell of a grid, turned the same way:
/// two cables, two pipes, two lights on one wall. Markers and spawners are left out
/// (`stacks_on_purpose`), as is the same thing turned another way, such as a light on each
/// wall of a corner or two pipes that cross. Found at the cell, about the first by uid, which a
/// fix keeps.
pub fn duplicates<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    duplicates_in(&anchored_cells(&standing(doc)), prototype)
}

fn duplicates_in<'a>(cells: &Cells, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    duplicate_groups(cells, prototype)
        .into_iter()
        .map(|(grid, cell, same)| {
            let message = format!("{} is on ({}, {}) {} times.", same[0].prototype, cell.0, cell.1, same.len());
            Finding { rule: Rule::Duplicates, message, grid, cell, entity: Some(same[0].uid), decal: None }
        })
        .collect()
}

/// What a fix of a stacked walls or duplicates finding deletes: all on its cell but the one it
/// keeps, as the map is now. Nothing for a finding of another rule, or one no longer there.
pub fn extras<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>, finding: &Finding) -> Vec<i32> {
    let cells = anchored_cells(&standing(doc));
    let groups = match finding.rule {
        Rule::StackedWalls => stacked_groups(&cells, prototype),
        Rule::Duplicates => duplicate_groups(&cells, prototype),
        _ => return Vec::new(),
    };
    groups
        .into_iter()
        .find(|(grid, cell, entities)| *grid == finding.grid && *cell == finding.cell && Some(entities[0].uid) == finding.entity)
        .map_or_else(Vec::new, |(.., entities)| entities[1..].iter().map(|entity| entity.uid).collect())
}

/// A wall, window or door ([`blocker`]) on a cell of a grid with no tile, or on the map off
/// every grid. The game takes anything anchored off a cell with no tile, so it loads loose: it
/// can drift and be pulled, and seals no room. One on the map is found at the cell of the
/// map it is in. By uid.
pub fn walls_without_floor<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    walls_without_floor_in(doc, &standing(doc), prototype)
}

fn walls_without_floor_in<'a>(doc: &Doc, standing: &[Standing], prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for &(entity, grid, cell) in standing {
        // The tile first: it is cheaper to read than the prototype.
        let off = grid == 0 || doc.tile_at(grid, cell) == SPACE;
        let uid = entity.uid;
        if !off || prototype(&entity.prototype).and_then(blocker).is_none() {
            continue;
        }
        let message = if grid == 0 {
            format!("{} at ({}, {}) is on no grid. In the game it floats loose in space.", entity.prototype, cell.0, cell.1)
        } else {
            format!("{} at ({}, {}) has no tile under it. In the game it comes loose, can drift, and doesn't seal the room.", entity.prototype, cell.0, cell.1)
        };
        findings.push(Finding { rule: Rule::WallWithoutFloor, message, grid, cell, entity: Some(uid), decal: None });
    }
    findings
}

/// Walls that may stand on any tile, though they have the Wall tag. The web editor's list,
/// which is the game's `AllowedWalls` in `MapWallFloorTests.cs`.
pub(crate) const ALLOWED_WALLS: &[&str] = &[
    "AsteroidRock",
    "AsteroidRockArtifactFragment",
    "AsteroidRockBananium",
    "AsteroidRockBananiumCrab",
    "AsteroidRockBluespace",
    "AsteroidRockCoal",
    "AsteroidRockCoalCrab",
    "AsteroidRockDiamond",
    "AsteroidRockGibtonite",
    "AsteroidRockGold",
    "AsteroidRockGoldCrab",
    "AsteroidRockTin",
    "AsteroidRockTinCrab",
    "AsteroidRockPlasma",
    "AsteroidRockQuartz",
    "AsteroidRockQuartzCrab",
    "AsteroidRockSalt",
    "AsteroidRockSilver",
    "AsteroidRockSilverCrab",
    "AsteroidRockUranium",
    "AsteroidRockUraniumCrab",
    "AsteroidRockMining",
    "WoodenSupportWall",
    "WoodenSupportWallBroken",
    "SolidSecretDoor",
];

/// The tiles a wall may stand on.
const WALL_TILES: &[&str] = &["Plating", "Lattice", SPACE];

/// The tiles that are no floor under a door.
const NO_FLOOR: &[&str] = &[SPACE, "Lattice"];

/// An entity standing on a grid or on the map, the grid (0 for the map) and the cell.
type Standing<'d> = (&'d Entity, i32, (i32, i32));

/// The entities that stand on a grid or on the map, inside nothing, in the order of their
/// uids, with the grid (0 for the map) and the cell.
fn standing(doc: &Doc) -> Vec<Standing<'_>> {
    let mut found: Vec<Standing> = doc
        .entities
        .values()
        .filter(|entity| !entity.contained && (doc.grid(entity.parent).is_some() || entity.parent == doc.map_uid))
        .map(|entity| {
            let grid = if entity.parent == doc.map_uid { 0 } else { entity.parent };
            (entity, grid, (entity.x.floor() as i32, entity.y.floor() as i32))
        })
        .collect();
    found.sort_unstable_by_key(|(entity, ..)| entity.uid);
    found
}

/// A wall on a tile that is not plating, lattice or space. A wall is an entity whose prototype
/// has the Wall tag and not the Diagonal tag, and is not one of the walls the game allows
/// anywhere. Tags an entity sets for itself are not in the mirror, so the prototype's count.
pub fn floor_under_wall<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    floor_under_wall_in(doc, &standing(doc), prototype)
}

fn floor_under_wall_in<'a>(doc: &Doc, standing: &[Standing], prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for &(entity, grid, cell) in standing {
        let uid = entity.uid;
        if ALLOWED_WALLS.contains(&entity.prototype.as_str()) {
            continue;
        }
        let Some(prototype) = prototype(&entity.prototype) else { continue };
        let tagged = |tag: &str| prototype.tags.iter().any(|known| &**known == tag);
        if !prototype.is_wall() || tagged("Diagonal") {
            continue;
        }
        let tile = doc.tile_at(grid, cell);
        if WALL_TILES.contains(&tile) {
            continue;
        }
        findings.push(Finding {
            rule: Rule::FloorUnderWall,
            message: format!("Floor tile ({tile}) under wall at ({}, {}). Walls should be on Plating.", cell.0, cell.1),
            grid,
            cell,
            entity: Some(uid),
            decal: None,
        });
    }
    findings
}

/// A door on space or lattice. A door is an entity whose prototype id has Airlock or Firelock
/// in it, as the web editor tells them.
pub fn door_without_floor(doc: &Doc) -> Vec<Finding> {
    door_without_floor_in(doc, &standing(doc))
}

fn door_without_floor_in(doc: &Doc, standing: &[Standing]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for &(entity, grid, cell) in standing {
        let uid = entity.uid;
        if !entity.prototype.contains("Airlock") && !entity.prototype.contains("Firelock") {
            continue;
        }
        if !NO_FLOOR.contains(&doc.tile_at(grid, cell)) {
            continue;
        }
        findings.push(Finding {
            rule: Rule::DoorWithoutFloor,
            message: format!("Door ({}) has no floor tile at ({}, {}).", entity.prototype, cell.0, cell.1),
            grid,
            cell,
            entity: Some(uid),
            decal: None,
        });
    }
    findings
}

/// A decal of a prototype the fork does not define, or that names none. `known` is the ids
/// of the fork's decal prototypes. Its cell is the one its middle is in. They come by grid
/// and then by id.
pub fn unknown_decals(doc: &Doc, known: &HashSet<&str>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut grids: Vec<_> = doc.grids.iter().collect();
    grids.sort_unstable_by_key(|grid| grid.uid);
    for grid in grids {
        let mut decals: Vec<_> = grid.decals.values().collect();
        decals.sort_unstable_by_key(|decal| decal.id);
        for decal in decals {
            if known.contains(decal.prototype.as_str()) {
                continue;
            }
            let cell = ((decal.x + 0.5).floor() as i32, (decal.y + 0.5).floor() as i32);
            let message = if decal.prototype.trim().is_empty() {
                format!("Decal {} at ({}, {}) has no prototype.", decal.id, cell.0, cell.1)
            } else {
                format!("Decal {} ({}) at ({}, {}) isn't a decal this fork has.", decal.id, decal.prototype, cell.0, cell.1)
            };
            findings.push(Finding { rule: Rule::UnknownDecal, message, grid: grid.uid, cell, entity: None, decal: Some(decal.id) });
        }
    }
    findings
}

/// Where an entity is for a finding: the grid it stands on through whatever holds it, or 0
/// for the map, and the cell there.
fn cell_of(doc: &Doc, uid: i32) -> Option<(i32, (i32, i32))> {
    let (grid, local) = doc.place_of(uid)?;
    Some((grid, (local.0.floor() as i32, local.1.floor() as i32)))
}

/// A link or a list entry that names an entity the map does not have, as the web editor words
/// it. A link to nothing is found on its source and a list entry of nothing on its list. A
/// device listed by a list that is gone is found on the device, as the web editor finds the
/// `deviceLists` of its `DeviceNetwork`. By the uid of where it is found.
pub fn dangling_links(doc: &Doc, links: &LinkMirror) -> Vec<Finding> {
    let mut found: Vec<(i32, i32, &str)> = Vec::new();
    for link in links.all_links() {
        if doc.entities.contains_key(&link.source) && !doc.entities.contains_key(&link.sink) {
            found.push((link.source, link.sink, "DeviceLinkSource"));
        }
    }
    for (list, devices) in links.all_lists() {
        for &device in devices {
            match (doc.entities.contains_key(&list), doc.entities.contains_key(&device)) {
                (true, false) => found.push((list, device, "DeviceList")),
                (false, true) => found.push((device, list, "DeviceNetwork")),
                _ => {}
            }
        }
    }
    found.sort_by_key(|&(at, missing, _)| (at, missing));
    found.dedup();
    found
        .into_iter()
        .filter_map(|(at, missing, component)| {
            let (grid, cell) = cell_of(doc, at)?;
            let message = format!("{} (UID {at}) points to UID {missing} in {component}, but no entity has that UID.", doc.entities[&at].prototype);
            Some(Finding { rule: Rule::DanglingLink, message, grid, cell, entity: Some(at), decal: None })
        })
        .collect()
}

/// An air alarm or a fire alarm, by its id as the web editor tells them, that keeps a device
/// list and has no device on it. One whose prototype has no `DeviceList` and has nothing
/// listed keeps none, and is not found. By uid.
pub fn unlinked_alarms<'a>(doc: &Doc, links: &LinkMirror, kind: Alarm, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Vec<Finding> {
    let mut alarms: Vec<i32> = doc
        .entities
        .values()
        .filter(|entity| alarm_of(&entity.prototype) == Some(kind) && links.devices(entity.uid).is_empty())
        .filter(|entity| prototype(&entity.prototype).is_some_and(|prototype| prototype.has("DeviceList")))
        .map(|entity| entity.uid)
        .collect();
    alarms.sort_unstable();
    let rule = match kind {
        Alarm::Air => Rule::UnlinkedAirAlarm,
        Alarm::Fire => Rule::UnlinkedFireAlarm,
    };
    alarms
        .into_iter()
        .filter_map(|uid| {
            let (grid, cell) = cell_of(doc, uid)?;
            let message = format!("{} at ({}, {}) has no linked devices.", doc.entities[&uid].prototype, cell.0, cell.1);
            Some(Finding { rule, message, grid, cell, entity: Some(uid), decal: None })
        })
        .collect()
}

/// A room open to space, or to the map's atmosphere where that is air, with its size and how
/// many leaks it has, found at its first leak. The outside itself is not found
/// (`Room::is_outside`), nor a room of fewer than [`SMALLEST_ROOM`] tiles. By grid and then
/// by the room's number. Nothing before the engine has read the air.
pub fn open_rooms(doc: &Doc) -> Vec<Finding> {
    let Some(mirror) = doc.air.as_ref() else { return Vec::new() };
    let mut grids: Vec<i32> = doc.grids.iter().map(|grid| grid.uid).filter(|uid| mirror.grids.contains_key(uid)).collect();
    grids.sort_unstable();
    let mut findings = Vec::new();
    for grid in grids {
        for (number, room) in mirror.grids[&grid].rooms.open() {
            if room.is_outside() || room.size < SMALLEST_ROOM {
                continue;
            }
            let cell = room.leaks[0];
            let message = format!("{}. The first leak is at ({}, {}).", room_text(number + 1, room, mirror.map_has_air), cell.0, cell.1);
            findings.push(Finding { rule: Rule::OpenRoom, message, grid, cell, entity: None, decal: None });
        }
    }
    findings
}

/// The air of each grid of the doc the engine has read, by grid uid. None before it has.
fn aired_grids(doc: &Doc) -> Option<(&AirMirror, Vec<(i32, &GridAir)>)> {
    let mirror = doc.air.as_ref()?;
    let mut grids: Vec<(i32, &GridAir)> = doc.grids.iter().filter_map(|grid| Some((grid.uid, mirror.grids.get(&grid.uid)?))).collect();
    grids.sort_unstable_by_key(|&(uid, _)| uid);
    Some((mirror, grids))
}

/// A grid with floor tiles and no atmosphere that can be given one, found at its first floor
/// (the lowest row, then the leftmost). Its floors keep no air in the game. A planet's own
/// ground has none and is not found: its air is the map's. By grid.
pub fn grids_without_atmosphere(doc: &Doc) -> Vec<Finding> {
    let Some((_, grids)) = aired_grids(doc) else { return Vec::new() };
    grids
        .into_iter()
        .filter(|(_, air)| !air.atmosphere && air.addable)
        .filter_map(|(uid, _)| {
            let grid = doc.grid(uid)?;
            let floors: Vec<(i32, i32)> = grid.tiles.keys().copied().filter(|&cell| !NO_FLOOR.contains(&doc.tile_at(uid, cell))).collect();
            let &cell = floors.iter().min_by_key(|&&(x, y)| (y, x))?;
            let tiles = if floors.len() == 1 { "its 1 floor tile stores".to_owned() } else { format!("its {} floor tiles store", floors.len()) };
            let message = format!("This grid has no atmosphere, so {tiles} no air. Fix adds one, the way the game's atmos:add does, and fills its floors with station air.");
            Some(Finding { rule: Rule::NoAtmosphere, message, grid: uid, cell, entity: None, decal: None })
        })
        .collect()
}

/// One finding for each sealed room of each grid that a fault of its air (`air_checks`) is
/// told of, by grid and then by the room's number.
fn room_findings(doc: &Doc, rule: Rule, tell: impl Fn(usize, &RoomFaults, &[String]) -> Option<(String, (i32, i32))>) -> Vec<Finding> {
    let Some((mirror, grids)) = aired_grids(doc) else { return Vec::new() };
    let mut findings = Vec::new();
    for (grid, air) in grids {
        for (&number, faults) in &air.faults.rooms {
            if let Some((message, cell)) = tell(number + 1, faults, &mirror.gases) {
                findings.push(Finding { rule, message, grid, cell, entity: None, decal: None });
            }
        }
    }
    findings
}

/// "5 of 48 tiles store" or "1 of 48 tiles stores": a count of a room's tiles with its verb.
fn of_tiles(count: usize, size: usize, one: &str, many: &str) -> String {
    format!("{count} of {size} tiles {}", if count == 1 { one } else { many })
}

/// A sealed room with floors that store no air, found at the first. The game makes them
/// vacuum as the round starts, and the room's air spreads into them. Most often floor laid
/// after the air was last fixed.
pub fn unfilled_floors(doc: &Doc) -> Vec<Finding> {
    room_findings(doc, Rule::UnfilledFloor, |number, faults, _| {
        let &first = faults.unfilled.first()?;
        let (x, y) = first;
        let message = if faults.unfilled.len() == faults.size {
            let size = if faults.size == 1 { "1 tile".to_owned() } else { format!("{} tiles", faults.size) };
            format!("Room {number} ({size}) has no air at all, so the game will start it as vacuum. Run Fix grid atmos to fill it. Its first tile is at ({x}, {y}).")
        } else {
            format!(
                "Room {number}: {} no air. When the round starts the game makes them vacuum, and the room's air spreads thin into them. Run Fix grid atmos to fill them. The first one is at ({x}, {y}).",
                of_tiles(faults.unfilled.len(), faults.size, "stores", "store")
            )
        };
        Some((message, first))
    })
}

/// A sealed room whose stored air is not the same everywhere, beyond the tolerances of
/// `air_checks`, with how many tiles differ from its most common air and how, found at the first.
pub fn uneven_air(doc: &Doc) -> Vec<Finding> {
    room_findings(doc, Rule::UnevenAir, |number, faults, gases| {
        let &((x, y), _) = faults.uneven.first()?;
        let message = format!("Room {number}: {} ({}). The first one is at ({x}, {y}).", of_tiles(faults.uneven.len(), faults.size, "differs", "differ"), faults.uneven_kinds(gases));
        Some((message, (x, y)))
    })
}

/// Tiles of a grid that hold no air by the game's rule and still store some, one finding a
/// grid at the first. The game clears it as the round starts.
pub fn air_in_walls(doc: &Doc) -> Vec<Finding> {
    let Some((_, grids)) = aired_grids(doc) else { return Vec::new() };
    grids
        .into_iter()
        .filter_map(|(grid, air)| {
            let &cell = air.faults.walls_with_air.first()?;
            let count = air.faults.walls_with_air.len();
            let tiles = if count == 1 { "1 tile fully blocked by a wall still stores".to_owned() } else { format!("{count} tiles fully blocked by walls still store") };
            let message = format!("{tiles} air. The game clears it when the round starts. The first one is at ({}, {}).", cell.0, cell.1);
            Some(Finding { rule: Rule::AirInWall, message, grid, cell, entity: None, decal: None })
        })
        .collect()
}

/// An engine direction turned by an entity's rotation, as DisposalTubeSystem turns a tube's
/// exits: to the nearest eighth of a turn.
fn turned(direction: u8, rotation: f64) -> u8 {
    let eighths = (rotation / std::f64::consts::FRAC_PI_4).round() as i64;
    (direction as i64 + eighths).rem_euclid(8) as u8
}

/// The cell beside one in an engine direction, if it is a side and not a corner.
fn beside(cell: (i32, i32), direction: u8) -> Option<(i32, i32)> {
    let (x, y) = match direction {
        0 => (0, -1),
        2 => (1, 0),
        4 => (0, 1),
        6 => (-1, 0),
        _ => return None,
    };
    Some((cell.0 + x, cell.1 + y))
}

const SIDES: [&str; 8] = ["south", "south-east", "east", "north-east", "north", "north-west", "west", "south-west"];

/// Where a disposal tube lets trash out with nothing to take it. A holder leaves the pipes on the
/// last tube it can reach (DisposalHolderSystem.Exit): into a disposal unit anchored on that
/// cell, or else onto the floor, which it pries up with the tile in front. Tubes join when
/// both are anchored and each has an exit facing the other (DisposalTubeSystem.CanConnect).
///
/// Found: a side trash can leave by that joins no tube, and a tube of one exit (a trunk) with
/// no unit on it, which is where trash coming down to it leaves. A plain junction never sends
/// anything out of its second exit (Starlight's SelectNextDirection), so that side may be open;
/// a router may use any of its exits. Nothing is found on a cell with a unit, nor a trunk on a
/// conveyor: that is how stations make their outlet to the recycler. Only a tile a crowbar
/// lifts is torn up (TileSystem.PryTile); `pryable` is their ids.
pub fn open_disposals<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype>, pryable: &HashSet<&str>) -> Vec<Finding> {
    let mut tubes: HashMap<(i32, (i32, i32)), Vec<(i32, &'a Prototype, Vec<u8>)>> = HashMap::new();
    let mut units = HashSet::new();
    let mut conveyors = HashSet::new();
    let mut order = Vec::new();
    for (entity, grid, cell) in standing(doc) {
        let uid = entity.uid;
        if grid == 0 || !entity.anchored {
            continue;
        }
        let Some(prototype) = prototype(&entity.prototype) else { continue };
        if prototype.has("DisposalUnit") {
            units.insert((grid, cell));
        }
        if prototype.has("Conveyor") {
            conveyors.insert((grid, cell));
        }
        if let Some(exits) = prototype.tube_exits.as_ref().filter(|exits| !exits.is_empty()) {
            tubes.entry((grid, cell)).or_default().push((uid, prototype, exits.iter().map(|&exit| turned(exit, entity.rotation)).collect()));
            order.push((uid, grid, cell));
        }
    }

    let joined = |grid: i32, cell: (i32, i32), side: u8| {
        beside(cell, side)
            .and_then(|next| tubes.get(&(grid, next)))
            .is_some_and(|there| there.iter().any(|(_, _, exits)| exits.contains(&((side + 4) % 8))))
    };

    let mut findings = Vec::new();
    for (uid, grid, cell) in order {
        if units.contains(&(grid, cell)) {
            continue;
        }
        let (_, prototype, exits) = tubes[&(grid, cell)].iter().find(|(tube, ..)| *tube == uid).unwrap();
        let routes = prototype.has("DisposalRouter") || prototype.has("DisposalSignalRouter");
        let open: Vec<&str> = exits
            .iter()
            .enumerate()
            .filter(|&(index, _)| routes || exits.len() < 3 || index != 1)
            .filter(|&(_, &side)| !joined(grid, cell, side))
            .map(|(_, &side)| SIDES[side as usize])
            .collect();
        let tile = doc.tile_at(grid, cell);
        let dumped = if tile == SPACE {
            "gets thrown out into space"
        } else if pryable.contains(tile) {
            "gets dumped there and tears up the floor"
        } else {
            "gets dumped there"
        };
        let message = if !open.is_empty() {
            let sides = match open.as_slice() {
                [one] => one.to_string(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                [] => unreachable!(),
            };
            format!("{} at ({}, {}) is open to the {sides}, so trash leaving that way {dumped}.", prototype.id, cell.0, cell.1)
        } else if exits.len() == 1 && !conveyors.contains(&(grid, cell)) {
            format!("{} at ({}, {}) has no disposal unit on it, so trash that reaches it {dumped}.", prototype.id, cell.0, cell.1)
        } else {
            continue;
        };
        findings.push(Finding { rule: Rule::OpenDisposals, message, grid, cell, entity: Some(uid), decal: None });
    }
    findings
}

/// Every rule's findings, in the order of [`Rule::ALL`]. The rules of links are checked only
/// once the engine has listed the links, and the rooms and their air once it has read the air.
pub fn validate<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype> + Copy, decal_ids: &HashSet<&str>, pryable: &HashSet<&str>) -> Vec<Finding> {
    let mut findings = quick(doc, prototype);
    if let Some(links) = doc.links.as_ref() {
        findings.extend(dangling_links(doc, links));
        findings.extend(unlinked_alarms(doc, links, Alarm::Air, prototype));
        findings.extend(unlinked_alarms(doc, links, Alarm::Fire, prototype));
    }
    findings.extend(unknown_decals(doc, decal_ids));
    findings.extend(open_rooms(doc));
    findings.extend(grids_without_atmosphere(doc));
    findings.extend(unfilled_floors(doc));
    findings.extend(uneven_air(doc));
    findings.extend(air_in_walls(doc));
    findings.extend(open_disposals(doc, prototype, pryable));
    findings
}

/// The findings of the rules that read only the entities and tiles of the mirror, in the order
/// of [`Rule::ALL`]: floors under walls, doors without floors, walls with no floor, stacked walls
/// and duplicates. They are cheap enough to run again after every edit, as the Problems overlay
/// does.
pub fn quick<'a>(doc: &Doc, prototype: impl Fn(&str) -> Option<&'a Prototype> + Copy) -> Vec<Finding> {
    // What stands where is found once for all five.
    let standing = standing(doc);
    let cells = anchored_cells(&standing);
    let mut findings = floor_under_wall_in(doc, &standing, prototype);
    findings.extend(door_without_floor_in(doc, &standing));
    findings.extend(walls_without_floor_in(doc, &standing, prototype));
    findings.extend(stacked_walls_in(&cells, prototype));
    findings.extend(duplicates_in(&cells, prototype));
    findings
}

/// Findings grouped by rule, in the order of [`Rule::ALL`]. A rule that found nothing has no group.
pub fn grouped(findings: &[Finding]) -> Vec<(Rule, Vec<&Finding>)> {
    Rule::ALL
        .iter()
        .map(|&rule| (rule, findings.iter().filter(|finding| finding.rule == rule).collect::<Vec<_>>()))
        .filter(|(_, found)| !found.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "tests/validate.rs"]
mod tests;
