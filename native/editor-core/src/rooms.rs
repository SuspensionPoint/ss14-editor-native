//! Rooms for the leak check: the tiles of a grid that share their air, and which of them are open
//! to the map's atmosphere. These are pure functions of what blocks air on each tile, as the
//! engine reports it (`cells` in the `air` answer, `Air.cs`).
//!
//! The game has no room finder of its own. It joins two neighbouring tiles unless either one
//! blocks the side they share (`AtmosphereSystem.GridAtmosphere.cs`, `UpdateAdjacentTiles`). A
//! room here is a set of tiles joined that way. A tile blocked on all four sides, like a wall or a
//! closed airlock, joins nothing and belongs to no room. A tile open to the map's atmosphere
//! (space, lattice) and an empty cell next to the grid count as the outside. A room joined to the
//! outside is open, and each of its tiles that touches the outside is a leak.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// The sides of a cell's flags, as the game's `AtmosDirection`.
pub const NORTH: u8 = 1;
pub const SOUTH: u8 = 2;
pub const EAST: u8 = 4;
pub const WEST: u8 = 8;
pub const ALL: u8 = 15;
/// A thing that blocks air there leaves no air when all four sides are blocked.
pub const NO_AIR: u8 = 16;
/// The tile is open to the map's atmosphere.
pub const OPEN: u8 = 32;
/// An airtight thing stands there, blocking or not, as an open door. The game gives such a
/// floor with no air the mean of its neighbours' air and not vacuum.
pub const FIX_VACUUM: u8 = 64;

/// Each side with the step to the neighbour across it and the side that neighbour shares.
pub(crate) const SIDES: [(u8, (i32, i32), u8); 4] = [(NORTH, (0, 1), SOUTH), (SOUTH, (0, -1), NORTH), (EAST, (1, 0), WEST), (WEST, (-1, 0), EAST)];

/// How many soft colours sealed rooms are given. Neighbouring rooms get different ones.
pub const COLOURS: usize = 8;

/// One room of a grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    /// How many tiles it has.
    pub size: usize,
    /// Its tiles joined to the outside, in order. None for a sealed room.
    pub leaks: Vec<(i32, i32)>,
    /// Each way out of a leak: its cell and the step to the outside across one of its sides,
    /// in order. A leak at a corner has two.
    pub ways_out: Vec<((i32, i32), (i32, i32))>,
    /// Which of the soft colours it is drawn in, from 0 to [`COLOURS`].
    pub colour: usize,
}

impl Room {
    pub fn is_open(&self) -> bool {
        !self.leaks.is_empty()
    }

    /// Whether it is the outside and no room: open on most of its edge, as a walkway on a
    /// hull or a planet's ground is. A room of n tiles has an edge of about four times the
    /// square root of n. One with three times that root of leaks or more is open on about
    /// three quarters of it. A small one is the outside when more than half its tiles leak, as
    /// a strip of floor in space does. A room with a hole, an open door or a girder in a wall
    /// has far fewer.
    pub fn is_outside(&self) -> bool {
        let leaks = self.leaks.len();
        self.is_open() && (leaks as f64 >= 3.0 * (self.size as f64).sqrt() || leaks * 2 > self.size)
    }
}

/// The rooms of one grid. A room's number is its place in `rooms`, from 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridRooms {
    pub rooms: Vec<Room>,
    /// The room of each tile that is in one.
    pub room_of: HashMap<(i32, i32), usize>,
    /// How long finding them took, in microseconds.
    pub micros: u64,
}

impl GridRooms {
    pub fn open(&self) -> impl Iterator<Item = (usize, &Room)> {
        self.rooms.iter().enumerate().filter(|(_, room)| room.is_open())
    }
}

/// The flags of each tile of a grid from the engine's three numbers a tile: x, y and flags.
pub fn cells_of(numbers: &[i32]) -> HashMap<(i32, i32), u8> {
    numbers.chunks_exact(3).map(|cell| ((cell[0], cell[1]), cell[2] as u8)).collect()
}

/// Whether air crosses from a cell to its neighbour on a side: neither blocks the side they
/// share. An empty cell blocks nothing.
fn crosses(from: u8, side: u8, to: u8, opposite: u8) -> bool {
    from & side == 0 && to & opposite == 0
}

/// Whether a tile can be in a room: it is no outside and it is not blocked on all sides.
fn inside(flags: u8) -> bool {
    flags & OPEN == 0 && flags & ALL != ALL
}

/// The rooms of a grid, from the flags of each of its tiles. Rooms are numbered in the order
/// of their lowest tile, by x and then y, so the same grid gives the same numbers.
pub fn find(cells: &HashMap<(i32, i32), u8>) -> GridRooms {
    let begun = Instant::now();
    let mut tiles: Vec<(i32, i32)> = cells.iter().filter(|(_, &flags)| inside(flags)).map(|(&cell, _)| cell).collect();
    tiles.sort_unstable();

    let mut room_of: HashMap<(i32, i32), usize> = HashMap::with_capacity(tiles.len());
    let mut rooms = Vec::new();
    let mut queue = VecDeque::new();
    for &first in &tiles {
        if room_of.contains_key(&first) {
            continue;
        }
        let number = rooms.len();
        let (mut size, mut leaks, mut ways_out) = (0, Vec::new(), Vec::new());
        room_of.insert(first, number);
        queue.push_back(first);
        while let Some(cell) = queue.pop_front() {
            size += 1;
            let flags = cells[&cell];
            let mut leaking = false;
            for (side, (dx, dy), opposite) in SIDES {
                let next = (cell.0 + dx, cell.1 + dy);
                let to = cells.get(&next).copied();
                if !crosses(flags, side, to.unwrap_or(0), opposite) {
                    continue;
                }
                match to {
                    Some(to) if inside(to) => {
                        if room_of.insert(next, number).is_none() {
                            queue.push_back(next);
                        }
                    }
                    // A tile blocked on all sides blocks this side too, so it is not reached.
                    _ => {
                        leaking = true;
                        ways_out.push((cell, (dx, dy)));
                    }
                }
            }
            if leaking {
                leaks.push(cell);
            }
        }
        leaks.sort_unstable();
        ways_out.sort_unstable();
        rooms.push(Room { size, leaks, ways_out, colour: 0 });
    }

    colour(&mut rooms, &room_of);
    GridRooms { rooms, room_of, micros: begun.elapsed().as_micros() as u64 }
}

/// Gives each room a colour none of its neighbours has, where the colours allow: a room meets
/// another across a wall or a blocked side, so the rooms two steps from a tile are its
/// neighbours. Larger rooms choose first. A room with neighbours of every colour takes the one
/// fewest of them have.
fn colour(rooms: &mut [Room], room_of: &HashMap<(i32, i32), usize>) {
    let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); rooms.len()];
    // Each pair once: the cells beside, across a corner and across a wall, to one side.
    for (&(x, y), &room) in room_of {
        for (dx, dy) in [(2, 0), (0, 2), (1, 1), (1, -1), (1, 0), (0, 1)] {
            let Some(&other) = room_of.get(&(x + dx, y + dy)) else { continue };
            if other != room && !neighbours[room].contains(&other) {
                neighbours[room].push(other);
                neighbours[other].push(room);
            }
        }
    }
    let mut order: Vec<usize> = (0..rooms.len()).collect();
    order.sort_by_key(|&room| (std::cmp::Reverse(rooms[room].size), room));
    let mut chosen: Vec<Option<usize>> = vec![None; rooms.len()];
    for room in order {
        let mut used = [0usize; COLOURS];
        for &other in &neighbours[room] {
            if let Some(colour) = chosen[other] {
                used[colour] += 1;
            }
        }
        // The first colour no neighbour has, or the one the fewest have.
        let colour = (0..COLOURS).min_by_key(|&colour| (used[colour], colour)).unwrap_or(0);
        chosen[room] = Some(colour);
        rooms[room].colour = colour;
    }
}

#[cfg(test)]
#[path = "tests/rooms.rs"]
mod tests;
