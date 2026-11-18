//! Device links and device lists: the session's mirror of them, what the engine reports about
//! ports, and the web editor's Auto-link Room.
//!
//! The mirror is filled once from the engine's answer to `links` when a map opens. After that,
//! each delta keeps it current by one rule. A removed uid takes its own links and its own list
//! with it, and each entry of `linked` is the complete new set of that entity's links and list.
//! The engine lists in `linked` every source whose links to a removed entity it took away, so
//! nothing else is needed.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Deserialize;

use crate::model::{Doc, EntityLinks, Link, Prototype, SPACE};

/// The engine's answer to `links`: every link, and every device on every device list.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct LinksAnswer {
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub lists: Vec<ListEntry>,
}

/// One device on one device list.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListEntry {
    pub list: i32,
    pub device: i32,
}

/// Every link and list entry of the open map, as the engine last told them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkMirror {
    /// The links of each source, in the engine's order.
    links: HashMap<i32, Vec<Link>>,
    /// The devices on each entity's device list, in the engine's order.
    lists: HashMap<i32, Vec<i32>>,
}

impl LinkMirror {
    pub fn from_answer(answer: LinksAnswer) -> LinkMirror {
        let mut mirror = LinkMirror::default();
        for link in answer.links {
            mirror.links.entry(link.source).or_default().push(link);
        }
        for entry in answer.lists {
            mirror.lists.entry(entry.list).or_default().push(entry.device);
        }
        mirror
    }

    /// Takes in what a delta says of links: the uids it removed, then its `linked`.
    pub fn apply(&mut self, removed: &[i32], linked: &[EntityLinks]) {
        for uid in removed {
            self.links.remove(uid);
            self.lists.remove(uid);
        }
        for entity in linked {
            if entity.links.is_empty() {
                self.links.remove(&entity.uid);
            } else {
                self.links.insert(entity.uid, entity.links.clone());
            }
            if entity.devices.is_empty() {
                self.lists.remove(&entity.uid);
            } else {
                self.lists.insert(entity.uid, entity.devices.clone());
            }
        }
    }

    /// The links an entity makes as a source.
    pub fn links_from(&self, source: i32) -> &[Link] {
        self.links.get(&source).map_or(&[], Vec::as_slice)
    }

    /// The links that end at an entity, by source and then in each source's order.
    pub fn links_to(&self, sink: i32) -> Vec<&Link> {
        let mut found: Vec<&Link> = self.links.values().flatten().filter(|link| link.sink == sink).collect();
        found.sort_by_key(|link| link.source);
        found
    }

    /// The devices on an entity's device list.
    pub fn devices(&self, list: i32) -> &[i32] {
        self.lists.get(&list).map_or(&[], Vec::as_slice)
    }

    /// The entities whose device lists have this device on them, in the order of their uids.
    pub fn lists_with(&self, device: i32) -> Vec<i32> {
        let mut found: Vec<i32> = self.lists.iter().filter(|(_, devices)| devices.contains(&device)).map(|(&list, _)| list).collect();
        found.sort_unstable();
        found
    }

    /// Every link, by source in the order of their uids.
    pub fn all_links(&self) -> Vec<&Link> {
        let mut sources: Vec<&i32> = self.links.keys().collect();
        sources.sort_unstable();
        sources.into_iter().flat_map(|source| &self.links[source]).collect()
    }

    /// Every device list that has a device on it, with its devices, in the order of their uids.
    pub fn all_lists(&self) -> Vec<(i32, &[i32])> {
        let mut lists: Vec<(i32, &[i32])> = self.lists.iter().map(|(&list, devices)| (list, devices.as_slice())).collect();
        lists.sort_unstable_by_key(|(list, _)| *list);
        lists
    }
}

// ---- ports ----

/// A port as the engine's `ports` lists it.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PortJson {
    pub id: String,
    /// The game's localisation key of its name.
    #[serde(default)]
    pub name: String,
    /// The sink ports the game's "link defaults" wires this source port to. None for a sink.
    #[serde(default)]
    pub defaults: Option<Vec<String>>,
}

/// The ports a prototype has, as the engine's `ports` lists it.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PrototypePortsJson {
    pub id: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub sinks: Vec<String>,
}

/// The engine's answer to `ports`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortsJson {
    #[serde(default)]
    pub source_ports: Vec<PortJson>,
    #[serde(default)]
    pub sink_ports: Vec<PortJson>,
    #[serde(default)]
    pub prototypes: Vec<PrototypePortsJson>,
}

/// What the engine tells of ports once a session: each source port's defaults, the
/// localisation key of each port's name, and the ports of each prototype that has any.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ports {
    defaults: HashMap<String, Vec<String>>,
    /// The keys of the names, of source ports and of sink ports, by the port's id.
    names: (HashMap<String, String>, HashMap<String, String>),
    prototypes: HashMap<String, (Vec<String>, Vec<String>)>,
}

impl Ports {
    pub fn from_json(json: PortsJson) -> Ports {
        let named = |ports: &[PortJson]| ports.iter().filter(|port| !port.name.is_empty()).map(|port| (port.id.clone(), port.name.clone())).collect();
        let names = (named(&json.source_ports), named(&json.sink_ports));
        let defaults = json.source_ports.into_iter().filter_map(|port| Some((port.id, port.defaults?))).collect();
        let prototypes = json.prototypes.into_iter().map(|prototype| (prototype.id, (prototype.sources, prototype.sinks))).collect();
        Ports { defaults, names, prototypes }
    }

    /// The localisation key of a port's name, a sink port's or a source port's, if the engine
    /// told one.
    pub fn name_key(&self, port: &str, sink: bool) -> Option<&str> {
        let names = if sink { &self.names.1 } else { &self.names.0 };
        names.get(port).map(String::as_str)
    }

    /// The sink ports the game links a source port to by default.
    pub fn defaults(&self, source_port: &str) -> &[String] {
        self.defaults.get(source_port).map_or(&[], Vec::as_slice)
    }

    /// Whether a prototype has a source port.
    pub fn is_source(&self, prototype: &str) -> bool {
        self.prototypes.get(prototype).is_some_and(|(sources, _)| !sources.is_empty())
    }
}

/// The ports an entity has now, as the engine's `ports.of` tells them.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityPorts {
    pub uid: i32,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub sinks: Vec<String>,
    /// The source's reach in tiles, 0 if it is no source.
    #[serde(default)]
    pub range: f32,
    #[serde(default)]
    pub has_device_list: bool,
    #[serde(default)]
    pub is_network_device: bool,
}

/// A source's ports, those whose defaults name a port of the sink first. Each part keeps the
/// engine's order.
pub fn source_ports_ordered(ports: &Ports, sources: &[String], sinks: &[String]) -> Vec<String> {
    let fits = |port: &String| ports.defaults(port).iter().any(|default| sinks.contains(default));
    sources.iter().filter(|port| fits(port)).chain(sources.iter().filter(|port| !fits(port))).cloned().collect()
}

/// A sink's ports, those the source port links to by default first, in the order of the
/// defaults, and then the rest in the engine's order.
pub fn sink_ports_ordered(ports: &Ports, source_port: Option<&str>, sinks: &[String]) -> Vec<String> {
    let defaults = source_port.map_or(&[][..], |port| ports.defaults(port));
    let first = defaults.iter().filter(|port| sinks.contains(port));
    first.chain(sinks.iter().filter(|port| !defaults.contains(port))).cloned().collect()
}

// ---- Auto-link Room ----

/// How many tiles a room may have at most, and how far it reaches from its alarm in steps,
/// the web editor's.
pub const ROOM_MOST_TILES: usize = 200;
pub const ROOM_REACH: i32 = 8;

/// The prototypes an air alarm takes into its list, and a fire alarm, by a part of their ids,
/// as the web editor finds them.
const AIR_ALARM_DEVICES: [&str; 3] = ["GasVentPump", "GasVentScrubber", "AirSensor"];
const FIRE_ALARM_DEVICES: [&str; 1] = ["Firelock"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarm {
    Air,
    Fire,
}

/// The kind of alarm a prototype is, by its id, as the web editor tells them.
pub fn alarm_of(prototype: &str) -> Option<Alarm> {
    if prototype.contains("AirAlarm") {
        Some(Alarm::Air)
    } else if prototype.contains("FireAlarm") {
        Some(Alarm::Fire)
    } else {
        None
    }
}

/// A wall stops a room. It is one by its id, or by an `Occluder` in its prototype.
fn is_wall(id: &str, prototype: Option<&Prototype>) -> bool {
    id.contains("Wall") || prototype.is_some_and(|prototype| prototype.has("Occluder"))
}

/// A door is the edge of a room.
fn is_door(id: &str) -> bool {
    id.contains("Airlock") || id.contains("Firelock")
}

/// The cells of a room and the cells round its edge, on one grid.
#[derive(Debug, Default, PartialEq)]
pub struct Room {
    pub tiles: HashSet<(i32, i32)>,
    pub boundary: HashSet<(i32, i32)>,
}

/// The ids of what stands on each cell of a grid, inside nothing.
fn standing_on(doc: &Doc, grid: i32) -> HashMap<(i32, i32), Vec<&str>> {
    let mut cells: HashMap<(i32, i32), Vec<&str>> = HashMap::new();
    for entity in doc.entities.values().filter(|entity| entity.parent == grid && !entity.contained) {
        cells.entry((entity.x.floor() as i32, entity.y.floor() as i32)).or_default().push(entity.prototype.as_str());
    }
    cells
}

/// The web editor's flood fill from a cell. The cell itself is always in the room, since an
/// alarm hangs on a wall, and the fill begins from it and its four neighbours. It goes by the
/// sides of cells over tiles that are not space, and stops at walls, doors, space and cells
/// more than [`ROOM_REACH`] steps from the start, which make the boundary. It stops growing
/// at [`ROOM_MOST_TILES`].
pub fn flood_room<'a>(doc: &Doc, grid: i32, start: (i32, i32), prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Room {
    let standing = standing_on(doc, grid);
    let mut room = Room::default();
    room.tiles.insert(start);
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::new();
    for next in [(start.0 + 1, start.1), (start.0 - 1, start.1), (start.0, start.1 + 1), (start.0, start.1 - 1)] {
        seen.insert(next);
        queue.push_back(next);
    }
    while room.tiles.len() < ROOM_MOST_TILES {
        let Some(cell) = queue.pop_front() else { break };
        if (cell.0 - start.0).abs() + (cell.1 - start.1).abs() > ROOM_REACH || doc.tile_at(grid, cell) == SPACE {
            room.boundary.insert(cell);
            continue;
        }
        let ids = standing.get(&cell).map_or(&[][..], Vec::as_slice);
        if ids.iter().any(|id| is_wall(id, prototype(id)) || is_door(id)) {
            room.boundary.insert(cell);
            continue;
        }
        room.tiles.insert(cell);
        for next in [(cell.0, cell.1 + 1), (cell.0, cell.1 - 1), (cell.0 + 1, cell.1), (cell.0 - 1, cell.1)] {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    room
}

/// The devices Auto-link Room adds to an alarm's list: for an air alarm the vents, scrubbers
/// and air sensors in its room, and for a fire alarm the firelocks round the room's edge. Those
/// on its list already are left out. In the order of their uids. Refused with why for an
/// entity that is no alarm or keeps no device list.
pub fn room_devices<'a>(doc: &Doc, links: &LinkMirror, alarm: i32, prototype: impl Fn(&str) -> Option<&'a Prototype>) -> Result<Vec<i32>, String> {
    let entity = doc.entities.get(&alarm).ok_or("The alarm isn't on the map")?;
    let Some(kind) = alarm_of(&entity.prototype) else { return Err(format!("{} isn't an air alarm or a fire alarm", entity.prototype)) };
    let listed = links.devices(alarm);
    if listed.is_empty() && !prototype(&entity.prototype).is_some_and(|prototype| prototype.has("DeviceList")) {
        return Err(format!("{} has no device list", entity.prototype));
    }
    let Some((grid, local)) = doc.place_of(alarm) else { return Err("The alarm isn't on the map".into()) };
    let room = flood_room(doc, grid, (local.0.floor() as i32, local.1.floor() as i32), &prototype);
    let (cells, wanted) = match kind {
        Alarm::Air => (&room.tiles, &AIR_ALARM_DEVICES[..]),
        Alarm::Fire => (&room.boundary, &FIRE_ALARM_DEVICES[..]),
    };
    let mut found: Vec<i32> = doc
        .entities
        .values()
        .filter(|other| other.uid != alarm && other.parent == grid && !other.contained && !listed.contains(&other.uid))
        .filter(|other| wanted.iter().any(|part| other.prototype.contains(part)))
        .filter(|other| cells.contains(&(other.x.floor() as i32, other.y.floor() as i32)))
        .map(|other| other.uid)
        .collect();
    found.sort_unstable();
    Ok(found)
}

#[cfg(test)]
#[path = "tests/links.rs"]
mod tests;
