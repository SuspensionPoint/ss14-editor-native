//! Which layer switch an entity falls under by the game's draw depth. Forks number their
//! draw depths differently (Content.Shared/DrawDepth/DrawDepth.cs): Starlight draws walls at
//! -2 and Space Wizards at -8, below their small mobs. So the engine tells the fork's names and
//! numbers (`drawdepths`), and the layers are found by name, the same on any fork.
//!
//! By name:
//! - SubFloor: LowFloors, BelowFloor, every pipe and wire, and any depth at or below BelowFloor.
//! - Floor Obj: FloorTiles, FloorObjects, Puddles, HighFloorObjects.
//! - Structures: DeadMobs, SmallMobs, Walls, WallTops.
//! - Doors: Doors, BlastDoors, Overdoors.
//! - Objects: every other name.
//!
//! A name the rule does not know goes with the known names on either side of it when both are
//! on one layer, as a fork's new kind of wall between its walls and wall tops would. Else it is
//! SubFloor at or below BelowFloor, and Objects above.
//!
//! A number between two names belongs with them when both are on one layer, as the gap a fork
//! leaves between its walls and wall tops, and with Objects when they are not. A number below
//! every name is SubFloor, and one above every name Objects. On Starlight this gives the very
//! ranges the web editor has always used (`DepthGroups::default`).

use crate::renderer::group;

/// The layer of each draw depth: the fork's named depths with their layers, lowest first.
/// A depth between two of them, or beyond them, takes its layer by the rule above.
#[derive(Clone, Debug, PartialEq)]
pub struct DepthGroups {
    named: Vec<(i32, u32)>,
}

impl Default for DepthGroups {
    /// Starlight's, the web editor's ranges: -13 and below SubFloor, -12 to -5 Floor Obj, -4 to
    /// -1 Structures, 8 to 10 Doors, the rest Objects. Used until the engine tells the fork's.
    fn default() -> Self {
        let named = [(-13, group::SUBFLOOR), (-12, group::FLOOR_OBJECTS), (-5, group::FLOOR_OBJECTS), (-4, group::STRUCTURES), (-1, group::STRUCTURES), (0, group::OBJECTS), (8, group::DOORS), (10, group::DOORS), (11, group::OBJECTS)];
        DepthGroups { named: named.to_vec() }
    }
}

/// A draw depth's name and number, as the engine tells them.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct DrawDepth {
    pub name: String,
    pub value: i32,
}

/// The layer a draw depth's name puts it on, if the name says.
fn named(name: &str) -> Option<u32> {
    match name {
        "LowFloors" | "BelowFloor" => Some(group::SUBFLOOR),
        "FloorTiles" | "FloorObjects" | "Puddles" | "HighFloorObjects" => Some(group::FLOOR_OBJECTS),
        "DeadMobs" | "SmallMobs" | "Walls" | "WallTops" => Some(group::STRUCTURES),
        "Doors" | "BlastDoors" | "Overdoors" => Some(group::DOORS),
        _ if name.contains("Pipe") || name.contains("Wire") => Some(group::SUBFLOOR),
        _ => None,
    }
}

impl DepthGroups {
    /// The layers of a fork's draw depths, from their names. With none told, Starlight's.
    pub fn from_depths(depths: &[DrawDepth]) -> DepthGroups {
        if depths.is_empty() {
            return DepthGroups::default();
        }
        let below_floor = depths.iter().find(|depth| depth.name == "BelowFloor").map(|depth| depth.value);
        // Each number once. When two names share a number, one the rule knows wins.
        let mut named: Vec<(i32, u32, bool)> = Vec::new();
        for depth in depths {
            let known = self::named(&depth.name);
            let layer = known.unwrap_or(if below_floor.is_some_and(|floor| depth.value <= floor) { group::SUBFLOOR } else { group::OBJECTS });
            match named.iter_mut().find(|(value, _, _)| *value == depth.value) {
                Some(entry) if !entry.2 && known.is_some() => *entry = (depth.value, layer, true),
                Some(_) => {}
                None => named.push((depth.value, layer, known.is_some())),
            }
        }
        named.sort_unstable_by_key(|(value, _, _)| *value);
        // A name the rule does not know, above BelowFloor, between two known names of one layer.
        let known: Vec<(usize, u32)> = named.iter().enumerate().filter(|(_, entry)| entry.2).map(|(at, entry)| (at, entry.1)).collect();
        for (at, (value, layer, is_known)) in named.iter_mut().enumerate() {
            if *is_known || below_floor.is_some_and(|floor| *value <= floor) {
                continue;
            }
            let below = known.iter().rev().find(|(index, _)| *index < at).map(|(_, layer)| *layer);
            let above = known.iter().find(|(index, _)| *index > at).map(|(_, layer)| *layer);
            if let (Some(below), Some(above)) = (below, above) {
                if below == above {
                    *layer = below;
                }
            }
        }
        DepthGroups { named: named.into_iter().map(|(value, layer, _)| (value, layer)).collect() }
    }

    /// The layers from the engine's answer to `drawdepths`. When it gave none, or one that
    /// does not read, Starlight's, with a line that says so: on another fork some things may
    /// then be under the wrong layer switch.
    pub fn from_answer(answer: Result<Vec<DrawDepth>, String>) -> (DepthGroups, Option<String>) {
        match answer {
            Ok(depths) if !depths.is_empty() => (DepthGroups::from_depths(&depths), None),
            Ok(_) => (DepthGroups::default(), Some("This fork reported no draw depths, so the layer switches use Starlight's grouping".into())),
            Err(error) => (DepthGroups::default(), Some(format!("Couldn't read this fork's draw depths ({}), so the layer switches use Starlight's grouping", error.lines().next().unwrap_or("")))),
        }
    }

    /// The layer of an entity drawn at a depth.
    pub fn group(&self, depth: i32) -> u32 {
        let at = self.named.partition_point(|&(value, _)| value < depth);
        match (at.checked_sub(1).and_then(|below| self.named.get(below)), self.named.get(at)) {
            (_, Some(&(value, layer))) if value == depth => layer,
            (Some(&(_, below)), Some(&(_, above))) if below == above => below,
            (None, _) => group::SUBFLOOR,
            _ => group::OBJECTS,
        }
    }
}

#[cfg(test)]
#[path = "tests/draw_depths.rs"]
mod tests;
