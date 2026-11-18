use super::*;
use crate::air::{GAS_CONSTANT, HAZARD_HIGH_PRESSURE};
use crate::rooms;

fn gases() -> Vec<String> {
    ["Oxygen", "Nitrogen", "CarbonDioxide", "Plasma"].map(String::from).to_vec()
}

/// Station air at a temperature and a share of one atmosphere.
fn air(kelvin: f32, atmospheres: f32) -> Mix {
    let moles = ONE_ATMOSPHERE * 2500.0 / (kelvin * GAS_CONSTANT) * atmospheres;
    Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles: vec![moles * 0.21, moles * 0.79, 0.0, 0.0] }
}

/// The mixes the letters of [`grid`] stand for, in this order: station air, a near copy
/// of it, vacuum, plasma, freezer air, a cold patch, thin air, the map's vacuum.
fn mixes() -> Vec<Mix> {
    vec![
        air(293.15, 1.0),
        air(293.14987, 1.0),
        Mix { temperature: 293.15, volume: 2500.0, immutable: false, moles: vec![0.0; 4] },
        Mix { temperature: 293.15, volume: 2500.0, immutable: false, moles: vec![0.0, 0.0, 0.0, 103.9] },
        air(235.0, 1.0),
        air(283.0, 1.0),
        air(293.15, 0.8),
        Mix { temperature: 2.7, volume: 2500.0, immutable: true, moles: vec![0.0; 4] },
    ]
}

/// A grid drawn in text, the top row first. Floors by the mix they store: `.` station
/// air, `d` its near copy, `v` vacuum, `p` plasma, `f` freezer air, `c` a cold patch, `t`
/// thin air. `n` a floor with no air, `o` an open door with none, `#` a wall with none,
/// `W` a wall that stores air, `D` a closed airlock that stores air, `_` space with the
/// map's vacuum, ` ` no tile.
fn grid(rows: &[&str]) -> GridFaults {
    let (mut tiles, mut cells) = (HashMap::new(), HashMap::new());
    let top = rows.len() as i32 - 1;
    for (row, line) in rows.iter().enumerate() {
        for (x, letter) in line.chars().enumerate() {
            let cell = (x as i32, top - row as i32);
            let (flags, mix) = match letter {
                '.' => (0, Some(0)),
                'd' => (0, Some(1)),
                'v' => (0, Some(2)),
                'p' => (0, Some(3)),
                'f' => (0, Some(4)),
                'c' => (0, Some(5)),
                't' => (0, Some(6)),
                'n' => (0, None),
                'o' => (FIX_VACUUM, None),
                '#' => (ALL | NO_AIR | FIX_VACUUM, None),
                'W' => (ALL | NO_AIR | FIX_VACUUM, Some(0)),
                'D' => (ALL | FIX_VACUUM, Some(0)),
                '_' => (OPEN, Some(7)),
                _ => continue,
            };
            cells.insert(cell, flags);
            if let Some(mix) = mix {
                tiles.insert(cell, mix);
            }
        }
    }
    find(&mixes(), &tiles, &cells, &rooms::find(&cells))
}

#[test]
fn drift_and_a_room_of_one_air_are_no_fault() {
    let faults = grid(&["########", "#..d.d.#", "#d...d.#", "########"]);
    assert_eq!(faults, GridFaults::default());
    // Rooms of another air each are no fault either: a freezer beside the station.
    assert!(grid(&["########", "#...#ff#", "#...#ff#", "########"]).rooms.is_empty());
}

#[test]
fn a_vacuum_a_plasma_and_a_freezer_tile_in_a_room_are_found() {
    let faults = grid(&["#######", "#..v..#", "#.p...#", "#....f#", "#######"]);
    let room = &faults.rooms[&0];
    assert_eq!(room.size, 15);
    assert_eq!(room.uneven, [((2, 2), Difference::Gas(3)), ((3, 3), Difference::Vacuum), ((5, 1), Difference::Colder)]);
    assert_eq!(room.uneven_kinds(&gases()), "vacuum (1), more plasma (1), colder (1)");
    assert!(room.unfilled.is_empty());
}

#[test]
fn a_cold_patch_and_thin_air_are_found_and_named_once_when_all_one_way() {
    let cold = grid(&["######", "#..cc#", "#...c#", "######"]);
    assert_eq!(cold.rooms[&0].uneven.len(), 3);
    assert_eq!(cold.rooms[&0].uneven_kinds(&gases()), "colder");
    let thin = grid(&["#####", "#..t#", "#####"]);
    assert_eq!(thin.rooms[&0].uneven, [((3, 1), Difference::LowerPressure)]);
}

#[test]
fn the_usual_air_is_the_most_common_so_a_room_mostly_of_vacuum_finds_its_air() {
    let faults = grid(&["######", "#vvv.#", "######"]);
    assert_eq!(faults.rooms[&0].uneven, [((4, 1), Difference::Air)]);
    // A tie takes the grid's first mix, station air, as usual.
    assert_eq!(grid(&["####", "#.v#", "####"]).rooms[&0].uneven, [((2, 1), Difference::Vacuum)]);
}

#[test]
fn floors_with_no_air_are_found_and_a_room_of_none_has_no_usual_air() {
    let faults = grid(&["######", "#.n.n#", "######", "#nnnn#", "######"]);
    assert_eq!(faults.rooms[&0].unfilled, [(1, 1), (2, 1), (3, 1), (4, 1)]);
    assert!(faults.rooms[&0].uneven.is_empty());
    assert_eq!(faults.rooms[&1].unfilled, [(2, 3), (4, 3)]);
    assert_eq!(faults.rooms[&1].hover_line(&gases()).unwrap(), "Air: 2 of 4 tiles store no air");
    // An open door with no air between two rooms is filled from its neighbours in the game.
    assert!(grid(&["#######", "#..o..#", "#######"]).rooms.is_empty());
}

#[test]
fn rooms_open_to_space_and_the_outside_are_not_checked() {
    // A room open to space with a vacuum tile and a floor with no air.
    let faults = grid(&["##_###", "#..vn#", "######", "______", "nv..n.", "######"]);
    assert!(faults.rooms.is_empty(), "{faults:?}");
}

#[test]
fn walls_that_store_air_are_found_and_airlocks_and_walls_on_lattice_are_not() {
    let faults = grid(&["#W###D#", "#.....#", "###W###"]);
    assert_eq!(faults.walls_with_air, [(1, 2), (3, 0)]);
    // The airlock keeps its air, and the room beside them is no fault.
    assert!(faults.rooms.is_empty());
    // A wall on lattice keeps the map's air in the game.
    let mut cells = HashMap::from([((0, 0), ALL | NO_AIR | OPEN)]);
    let tiles = HashMap::from([((0, 0), 7)]);
    assert!(find(&mixes(), &tiles, &cells, &rooms::find(&cells)).walls_with_air.is_empty());
    cells.insert((0, 0), ALL | NO_AIR);
    assert_eq!(find(&mixes(), &tiles, &cells, &rooms::find(&cells)).walls_with_air, [(0, 0)]);
}

#[test]
fn the_hover_line_tells_both_faults() {
    let faults = grid(&["######", "#.nv.#", "######"]);
    assert_eq!(faults.rooms[&0].hover_line(&gases()).unwrap(), "Air: 1 of 4 tiles stores no air, 1 tile differs (vacuum)");
    assert_eq!(RoomFaults::default().hover_line(&gases()), None);
}

#[test]
fn the_tolerances_pass_drift_and_catch_what_the_game_acts_on() {
    let station = air(293.15, 1.0);
    let differs = |mix: Mix| difference(&mix, &station);
    assert_eq!(differs(air(293.1495, 1.0)), None);
    assert_eq!(differs(air(296.0, 1.0)), None);
    assert_eq!(differs(air(298.0, 1.0)), Some(Difference::Warmer));
    // Cork's outside air at 288.15 K is 5 K colder: a tile of it in a room is found.
    assert_eq!(differs(air(288.15, 1.0)), Some(Difference::Colder));
    assert_eq!(differs(air(293.15, 0.95)), None);
    assert_eq!(differs(air(293.15, HAZARD_HIGH_PRESSURE / ONE_ATMOSPHERE)), Some(Difference::HigherPressure));
    // A trace of plasma under the alarms' smallest danger is none, one over it is.
    let traced = |plasma: f32| Mix { moles: vec![21.8, 82.1, 0.0, plasma], ..station.clone() };
    assert_eq!(differs(traced(0.3)), None);
    assert_eq!(differs(traced(0.5)), Some(Difference::Gas(3)));
    // Vacuum against vacuum, whatever its temperature, is none.
    let vacuum = Mix { temperature: 2.7, volume: 2500.0, immutable: true, moles: vec![0.0; 4] };
    assert_eq!(difference(&mixes()[2], &vacuum), None);
    assert_eq!(Difference::Gas(9).text(&gases()), "more of another gas");
    assert_eq!(Difference::Gas(2).text(&gases()), "more carbon dioxide");
}
