//! Stored-air faults, listed by Validate Map and reported by the Rooms view's hover box. There are
//! three: a sealed room whose air isn't the same everywhere, floors of a sealed room that store no
//! air, and walls that store air. These are pure functions of a grid's stored air, what blocks air
//! on its tiles, and its rooms (`crate::rooms`). They run on the engine's thread along with the
//! rooms.
//!
//! Only sealed rooms are checked. A room open to the outside already shows up in Validate Map's
//! "Rooms Open To Space", and its air will change whatever it stores.

use std::collections::{BTreeMap, HashMap};

use crate::air::{gas_name, Mix, HAZARD_LOW_PRESSURE, ONE_ATMOSPHERE, OTHER_GAS};
use crate::rooms::{GridRooms, ALL, FIX_VACUUM, NO_AIR, OPEN};

// What counts as a difference between two tiles of a room. Simulation drift leaves near
// copies of a mix (Bagel stores station air at 293.14987 K and at 293.1495 K), so a tolerance
// must pass those and still catch a vacuum tile, a plasma tile, freezer air or a cold patch.
// Each is a difference the game itself would act on. Paths are under upstream/starlight.

/// A tenth of an atmosphere. The game lets a group of tiles rest once less than a tenth of a
/// cell's standard moles moves between them (Atmospherics.MinimumAirToSuspend, used by
/// AtmosphereSystem.LINDA.cs), which is this much pressure at station temperature.
pub const PRESSURE_TOLERANCE: f32 = 0.1 * ONE_ATMOSPHERE;
/// Kelvin. Two tiles closer than this exchange no more heat as a group
/// (Atmospherics.MinimumTemperatureDeltaToSuspend, used by AtmosphereSystem.Gases.cs CompareExchange).
pub const TEMPERATURE_TOLERANCE: f32 = 4.0;
/// A share of the mix, from 0 to 1. The smallest share of a gas a station air alarm calls
/// dangerous is tritium's 0.4% (airalarms.yml). A smaller difference cannot make one tile
/// alarm where the rest of its room does not.
pub const SHARE_TOLERANCE: f32 = OTHER_GAS;

/// How a tile's air differs from the most common air of its room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Difference {
    /// Vacuum, or near it, in a room of air.
    Vacuum,
    /// Air in a room of vacuum.
    Air,
    /// Another mix: the gas whose share rose most, by its place in the gases.
    Gas(usize),
    Colder,
    Warmer,
    LowerPressure,
    HigherPressure,
}

impl Difference {
    /// As Validate Map and the box by the mouse say it.
    pub fn text(self, gases: &[String]) -> String {
        match self {
            Difference::Vacuum => "vacuum".to_owned(),
            Difference::Air => "air".to_owned(),
            Difference::Gas(gas) => format!("more {}", gases.get(gas).map_or_else(|| "of another gas".to_owned(), |name| gas_name(name).to_lowercase())),
            Difference::Colder => "colder".to_owned(),
            Difference::Warmer => "warmer".to_owned(),
            Difference::LowerPressure => "lower pressure".to_owned(),
            Difference::HigherPressure => "higher pressure".to_owned(),
        }
    }
}

/// How a mix differs from a room's usual one beyond the tolerances, if it does. Vacuum first,
/// then what the mix is made of, then temperature, then pressure. Two mixes that are both
/// vacuum, at or below the pressure a body takes vacuum damage at, do not differ: the
/// temperature and the traces of a vacuum mean nothing.
pub fn difference(mix: &Mix, usual: &Mix) -> Option<Difference> {
    let (pressure, usual_pressure) = (mix.pressure(), usual.pressure());
    match (pressure <= HAZARD_LOW_PRESSURE, usual_pressure <= HAZARD_LOW_PRESSURE) {
        (true, true) => return None,
        (true, false) => return Some(Difference::Vacuum),
        (false, true) => return Some(Difference::Air),
        (false, false) => {}
    }
    let (total, usual_total) = (mix.total_moles(), usual.total_moles());
    let share = |mix: &Mix, total: f32, gas: usize| mix.moles.get(gas).copied().unwrap_or(0.0) / total;
    let mut rose_most = (f32::MIN, 0);
    let mut differs = false;
    for gas in 0..mix.moles.len().max(usual.moles.len()) {
        let change = share(mix, total, gas) - share(usual, usual_total, gas);
        differs |= change.abs() > SHARE_TOLERANCE;
        if change > rose_most.0 {
            rose_most = (change, gas);
        }
    }
    if differs {
        return Some(Difference::Gas(rose_most.1));
    }
    let warmer = mix.temperature - usual.temperature;
    if warmer.abs() > TEMPERATURE_TOLERANCE {
        return Some(if warmer > 0.0 { Difference::Warmer } else { Difference::Colder });
    }
    let higher = pressure - usual_pressure;
    if higher.abs() > PRESSURE_TOLERANCE {
        return Some(if higher > 0.0 { Difference::HigherPressure } else { Difference::LowerPressure });
    }
    None
}

/// The faults of one sealed room.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomFaults {
    /// How many tiles the room has.
    pub size: usize,
    /// Its tiles that store no air and would be vacuum in the game, in order by x and then y.
    /// A tile under an airtight thing, as an open door, is not one: the game fills it with
    /// the mean of its neighbours (AtmosphereSystem.GridAtmosphere.cs GridFixTileVacuum).
    pub unfilled: Vec<(i32, i32)>,
    /// Its tiles whose air differs from the room's most common, each with how, in order.
    pub uneven: Vec<((i32, i32), Difference)>,
}

impl RoomFaults {
    /// How the uneven tiles differ: "vacuum" when all one way, or each way with its count,
    /// "vacuum (2), colder (1)", in the order of [`Difference`].
    pub fn uneven_kinds(&self, gases: &[String]) -> String {
        let mut kinds: BTreeMap<Difference, usize> = BTreeMap::new();
        for &(_, kind) in &self.uneven {
            *kinds.entry(kind).or_default() += 1;
        }
        if kinds.len() == 1 {
            return kinds.keys().next().map(|kind| kind.text(gases)).unwrap_or_default();
        }
        kinds.iter().map(|(kind, count)| format!("{} ({count})", kind.text(gases))).collect::<Vec<_>>().join(", ")
    }

    /// The one line the Rooms view's box by the mouse adds for the room. None for a room with
    /// no fault.
    pub fn hover_line(&self, gases: &[String]) -> Option<String> {
        let mut parts = Vec::new();
        if !self.unfilled.is_empty() {
            parts.push(format!("{} of {} tiles {} no air", self.unfilled.len(), self.size, if self.unfilled.len() == 1 { "stores" } else { "store" }));
        }
        if !self.uneven.is_empty() {
            parts.push(format!("{} {} ({})", self.uneven.len(), if self.uneven.len() == 1 { "tile differs" } else { "tiles differ" }, self.uneven_kinds(gases)));
        }
        (!parts.is_empty()).then(|| format!("Air: {}", parts.join(", ")))
    }
}

/// The faults of a grid's air.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridFaults {
    /// By room number, the sealed rooms with a fault.
    pub rooms: BTreeMap<usize, RoomFaults>,
    /// Tiles that hold no air by the game's rule and still store some, in order: a wall placed
    /// after the air was last fixed. The game clears it as the round starts
    /// (AtmosphereSystem.Processing.cs UpdateTileAir), so it is harmless.
    pub walls_with_air: Vec<(i32, i32)>,
}

/// The faults of a grid from its stored mixes, the mix of each tile, the flags of each tile
/// (`crate::rooms`) and its rooms. A room's usual air is the mix most of its tiles store, the
/// first of the grid's mixes if two are as common.
pub fn find(mixes: &[Mix], tiles: &HashMap<(i32, i32), u32>, cells: &HashMap<(i32, i32), u8>, rooms: &GridRooms) -> GridFaults {
    let mut of_room: Vec<Vec<(i32, i32)>> = vec![Vec::new(); rooms.rooms.len()];
    for (&cell, &room) in &rooms.room_of {
        if !rooms.rooms[room].is_open() {
            of_room[room].push(cell);
        }
    }

    let mut found = BTreeMap::new();
    for (number, mut members) in of_room.into_iter().enumerate() {
        if members.is_empty() {
            continue;
        }
        members.sort_unstable();
        let mut counts: HashMap<u32, usize> = HashMap::new();
        let mut unfilled = Vec::new();
        for cell in &members {
            match tiles.get(cell).filter(|&&index| (index as usize) < mixes.len()) {
                Some(&index) => *counts.entry(index).or_default() += 1,
                None if cells.get(cell).is_some_and(|&flags| flags & FIX_VACUUM != 0) => {}
                None => unfilled.push(*cell),
            }
        }
        let mut uneven = Vec::new();
        if let Some((&usual, _)) = counts.iter().max_by_key(|&(&index, &count)| (count, std::cmp::Reverse(index))) {
            let mut known: HashMap<u32, Option<Difference>> = HashMap::new();
            for cell in &members {
                let Some(&index) = tiles.get(cell).filter(|&&index| index != usual && (index as usize) < mixes.len()) else { continue };
                if let Some(kind) = *known.entry(index).or_insert_with(|| difference(&mixes[index as usize], &mixes[usual as usize])) {
                    uneven.push((*cell, kind));
                }
            }
        }
        if !unfilled.is_empty() || !uneven.is_empty() {
            found.insert(number, RoomFaults { size: members.len(), unfilled, uneven });
        }
    }

    let mut walls_with_air: Vec<(i32, i32)> = cells
        .iter()
        .filter(|&(cell, &flags)| flags & ALL == ALL && flags & NO_AIR != 0 && flags & OPEN == 0 && tiles.contains_key(cell))
        .map(|(&cell, _)| cell)
        .collect();
    walls_with_air.sort_unstable();
    GridFaults { rooms: found, walls_with_air }
}

#[cfg(test)]
#[path = "tests/air_checks.rs"]
mod tests;
