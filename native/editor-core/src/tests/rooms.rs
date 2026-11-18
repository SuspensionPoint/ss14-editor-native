use super::*;

/// A grid drawn in text, the top row first, one character a cell:
/// `.` floor, `#` wall, `D` closed airlock, `_` space or lattice, ` ` no tile,
/// `v` a thin window blocking south, `>` one blocking east.
fn grid(rows: &[&str]) -> HashMap<(i32, i32), u8> {
    let mut cells = HashMap::new();
    let top = rows.len() as i32 - 1;
    for (row, line) in rows.iter().enumerate() {
        for (x, letter) in line.chars().enumerate() {
            let flags = match letter {
                '.' => 0,
                '#' => ALL | NO_AIR,
                'D' => ALL,
                '_' => OPEN,
                'v' => SOUTH,
                '>' => EAST,
                _ => continue,
            };
            cells.insert((x as i32, top - row as i32), flags);
        }
    }
    cells
}

fn sizes(rooms: &GridRooms) -> Vec<(usize, usize)> {
    rooms.rooms.iter().map(|room| (room.size, room.leaks.len())).collect()
}

#[test]
fn a_room_walled_all_round_is_sealed() {
    let rooms = find(&grid(&["#####", "#...#", "#...#", "#####"]));
    assert_eq!(sizes(&rooms), [(6, 0)]);
    assert_eq!(rooms.room_of[&(1, 1)], 0);
    // Walls are in no room.
    assert!(!rooms.room_of.contains_key(&(0, 0)));
}

#[test]
fn a_closed_airlock_parts_two_rooms_and_is_in_neither() {
    let rooms = find(&grid(&["#######", "#..D..#", "#######"]));
    assert_eq!(sizes(&rooms), [(2, 0), (2, 0)]);
    assert!(!rooms.room_of.contains_key(&(3, 1)));
    // Neighbours across the airlock have different colours.
    assert_ne!(rooms.rooms[0].colour, rooms.rooms[1].colour);
}

#[test]
fn a_gap_in_a_wall_to_an_empty_cell_is_a_leak() {
    // The gap is at the room's top right: the floor at (3, 2) has nothing above it.
    let rooms = find(&grid(&["###. ", "#...#", "#####"]));
    assert_eq!(sizes(&rooms), [(4, 1)]);
    assert_eq!(rooms.rooms[0].leaks, [(3, 2)]);
}

#[test]
fn space_and_lattice_tiles_are_outside_and_several_leaks_are_kept() {
    let rooms = find(&grid(&["#_###", "#...#", "#..._", "#####"]));
    assert_eq!(rooms.rooms[0].leaks, [(1, 2), (3, 1)]);
    // Each leak's way out: up through the lattice, and right into space.
    assert_eq!(rooms.rooms[0].ways_out, [((1, 2), (0, 1)), ((3, 1), (1, 0))]);
    assert_eq!(rooms.open().count(), 1);
}

#[test]
fn a_thin_window_blocks_only_its_side_and_either_tile_s_side_counts() {
    // A window on the floor below the gap blocks south, so not north: the gap leaks.
    let below = find(&grid(&["## ##", "#.v.#", "#...#", "#####"]));
    assert_eq!(below.rooms[0].leaks, [(2, 2)]);
    // A window blocking south on a floor in the gap itself, with space above: the room
    // below is sealed from it, and the window's own tile is a room open to space.
    let in_gap = find(&grid(&["##_##", "##v##", "#...#", "#####"]));
    assert_eq!(sizes(&in_gap), [(3, 0), (1, 1)]);
    // A window blocking east between two floors parts them, from either side.
    let parted = find(&grid(&["####", "#>.#", "####"]));
    assert_eq!(sizes(&parted), [(1, 0), (1, 0)]);
}

#[test]
fn a_wall_on_lattice_blocks() {
    // The wall's tile is open, but a wall blocks all sides.
    let mut cells = grid(&["#####", "#...#", "#####"]);
    cells.insert((2, 2), ALL | NO_AIR | OPEN);
    assert_eq!(sizes(&find(&cells)), [(3, 0)]);
}

#[test]
fn rooms_are_numbered_by_their_lowest_tile_and_the_same_each_time() {
    let cells = grid(&["#####", "#.#.#", "#####", "#.###", "#####"]);
    let rooms = find(&cells);
    assert_eq!(rooms.room_of[&(1, 1)], 0);
    assert_eq!(rooms.room_of[&(1, 3)], 1);
    assert_eq!(rooms.room_of[&(3, 3)], 2);
    assert_eq!(find(&cells).room_of, rooms.room_of);
}

#[test]
fn neighbouring_rooms_take_different_colours() {
    // A row of six rooms of one tile, each beside the next across a wall.
    let rooms = find(&grid(&["#############", "#.#.#.#.#.#.#", "#############"]));
    assert_eq!(rooms.rooms.len(), 6);
    for pair in rooms.rooms.windows(2) {
        assert_ne!(pair[0].colour, pair[1].colour);
    }
    assert!(rooms.rooms.iter().all(|room| room.colour < COLOURS));
}

#[test]
fn a_room_open_on_most_of_its_edge_is_the_outside() {
    // A walkway along a hull, open on its whole length: every tile leaks.
    let walkway = find(&grid(&["___________", ".........._", "###########"]));
    assert!(walkway.rooms[0].is_outside());
    // A room of twenty-five tiles with one gap is a room open to space.
    let room = find(&grid(&["##_####", "#.....#", "#.....#", "#.....#", "#.....#", "#.....#", "#######"]));
    assert_eq!(sizes(&room), [(25, 1)]);
    assert!(room.rooms[0].is_open() && !room.rooms[0].is_outside());
    // Open on most of its edge it is the outside, as a planet's ground is.
    let ground = find(&grid(&["_______", "_....._", "_....._", "_....._", "_....._", "_....._", "_______"]));
    assert!(ground.rooms[0].is_outside());
    // A strip of five floors in space, all leaking, is the outside. A small room with half
    // its tiles open is still a room.
    assert!(find(&grid(&[".", ".", ".", ".", "."])).rooms[0].is_outside());
    let half = find(&grid(&["#__#", "#..#", "#..#", "####"]));
    assert_eq!(sizes(&half), [(4, 2)]);
    assert!(!half.rooms[0].is_outside());
    // A sealed room is never the outside.
    assert!(!find(&grid(&["###", "#.#", "###"])).rooms[0].is_outside());
}

#[test]
fn the_engine_s_numbers_read_as_cells() {
    assert_eq!(cells_of(&[1, 2, 31, -1, 0, 32]), HashMap::from([((1, 2), 31), ((-1, 0), 32)]));
}
