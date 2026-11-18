//! The pipe tool: the pieces of each network, the fitting a cell calls for, and laying and
//! removing pipes with the pieces round them refitted.
//!
//! A cell's fitting comes from the sides it is open on. A cell of a path is open towards the
//! cells before and after it in the path, and towards a neighbour whose pipe is open towards
//! it. A pipe of the network that was on the cell keeps the sides that joined a neighbour and
//! loses those that led nowhere, so a lone straight turns to follow the path. A neighbour
//! that only stands beside the path is not joined, so a line drawn alongside another stays
//! apart from it. The mirror does not hold a pipe's colour, so this is how supply and return
//! lines laid side by side stay apart. To branch off a line or turn its end, the drag begins
//! or ends on it. A removed pipe's neighbours lose the sides towards it and keep the rest.

use super::*;

/// The sides of a cell as bits of a mask, in the grid's own directions.
pub(super) const NORTH: u8 = 1;
pub(super) const EAST: u8 = 2;
pub(super) const SOUTH: u8 = 4;
pub(super) const WEST: u8 = 8;

/// Each side with the step to the cell on that side.
const SIDES: [(u8, (i32, i32)); 4] = [(NORTH, (0, 1)), (EAST, (1, 0)), (SOUTH, (0, -1)), (WEST, (-1, 0))];

/// The colours a gas pipe is laid in, as the web editor has them: supply and return.
pub const PIPE_COLOURS: [(&str, &str); 2] = [("Supply", "#0055CCFF"), ("Return", "#990000FF")];

/// The four shapes of pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Piece {
    Straight,
    Bend,
    Junction,
    Fourway,
}

impl Piece {
    const ALL: [Piece; 4] = [Piece::Straight, Piece::Bend, Piece::Junction, Piece::Fourway];

    /// The sides it is open on unturned, as the web editor has them: a straight runs north and
    /// south, a bend joins south and west, and a T is open on all sides but north.
    fn unturned(self) -> u8 {
        match self {
            Piece::Straight => NORTH | SOUTH,
            Piece::Bend => SOUTH | WEST,
            Piece::Junction => EAST | SOUTH | WEST,
            Piece::Fourway => NORTH | EAST | SOUTH | WEST,
        }
    }
}

/// A mask turned a quarter anticlockwise, as a positive angle turns a piece: north goes to
/// west, east to north, south to east and west to south.
fn quarter(mask: u8) -> u8 {
    (mask >> 1) | ((mask & NORTH) << 3)
}

/// The sides a piece is open on, turned by quarter turns anticlockwise.
pub(super) fn open_sides(piece: Piece, quarters: i32) -> u8 {
    (0..quarters.rem_euclid(4)).fold(piece.unturned(), |mask, _| quarter(mask))
}

/// The angle the game is given for quarter turns anticlockwise, as the web editor writes it.
pub(super) fn angle(quarters: i32) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    [0.0, FRAC_PI_2, PI, -FRAC_PI_2][quarters.rem_euclid(4) as usize]
}

/// The quarter turns of an angle, to the nearest.
fn quarters_of(rotation: f64) -> i32 {
    ((rotation / std::f64::consts::FRAC_PI_2).round() as i32).rem_euclid(4)
}

/// The side opposite one.
fn opposite(side: u8) -> u8 {
    quarter(quarter(side))
}

/// The piece and its quarter turns that a cell open on these sides calls for. This is the web
/// editor's `fitPipes`: four sides a four-way, three a T, two a straight or a bend, and one or
/// none a straight along that side, north and south when there is none.
pub(super) fn fit(sides: u8) -> (Piece, i32) {
    let sides = match sides.count_ones() {
        0 | 1 if sides & (EAST | WEST) != 0 => EAST | WEST,
        0 | 1 => NORTH | SOUTH,
        _ => sides,
    };
    for piece in Piece::ALL {
        for quarters in 0..4 {
            if open_sides(piece, quarters) == sides {
                return (piece, quarters);
            }
        }
    }
    unreachable!("every mask of two sides or more is a piece")
}

/// The prototypes of one network's pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipePieces {
    pub straight: &'static str,
    pub bend: &'static str,
    pub junction: &'static str,
    pub fourway: &'static str,
}

impl PipePieces {
    pub fn ids(self) -> [&'static str; 4] {
        [self.straight, self.bend, self.junction, self.fourway]
    }

    fn id(self, piece: Piece) -> &'static str {
        match piece {
            Piece::Straight => self.straight,
            Piece::Bend => self.bend,
            Piece::Junction => self.junction,
            Piece::Fourway => self.fourway,
        }
    }

    /// The piece a prototype is, the first in the order straight, bend, T, four-way where one
    /// prototype is two pieces.
    fn piece_of(self, prototype: &str) -> Option<Piece> {
        Piece::ALL.into_iter().find(|&piece| self.id(piece) == prototype)
    }
}

/// A network of pipes the tool lays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeNetwork {
    Gas,
    Disposal,
}

impl PipeNetwork {
    pub const ALL: [PipeNetwork; 2] = [PipeNetwork::Gas, PipeNetwork::Disposal];

    /// Its pieces. These are the web editor's names, which the fork's prototype list is
    /// checked against (`Session::pipe_network_available`). Disposal has no four-way, and a
    /// junction stands in for one as in the web editor.
    pub fn pieces(self) -> PipePieces {
        match self {
            PipeNetwork::Gas => PipePieces { straight: "GasPipeStraight", bend: "GasPipeBend", junction: "GasPipeTJunction", fourway: "GasPipeFourway" },
            PipeNetwork::Disposal => PipePieces { straight: "DisposalPipe", bend: "DisposalBend", junction: "DisposalJunction", fourway: "DisposalJunction" },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PipeNetwork::Gas => "Gas pipe",
            PipeNetwork::Disposal => "Disposal pipe",
        }
    }
}

/// A pipe of the network on a cell, as the mirror holds it.
#[derive(Clone, Copy, Debug)]
struct Laid {
    uid: i32,
    piece: Piece,
    quarters: i32,
    sides: u8,
}

/// The pipes of a network on a grid by cell: children of the grid inside nothing. Of two on
/// one cell, the one of the lower uid.
fn pipes_on(doc: &Doc, grid: i32, pieces: PipePieces) -> HashMap<(i32, i32), Laid> {
    let mut found: Vec<(&Entity, Piece)> = doc
        .entities
        .values()
        .filter(|entity| entity.parent == grid && !entity.contained)
        .filter_map(|entity| pieces.piece_of(&entity.prototype).map(|piece| (entity, piece)))
        .collect();
    found.sort_unstable_by_key(|(entity, _)| std::cmp::Reverse(entity.uid));
    found
        .into_iter()
        .map(|(entity, piece)| {
            let quarters = quarters_of(entity.rotation);
            ((entity.x.floor() as i32, entity.y.floor() as i32), Laid { uid: entity.uid, piece, quarters, sides: open_sides(piece, quarters) })
        })
        .collect()
}

/// The side of a cell that another next to it is on.
fn side_towards(from: (i32, i32), to: (i32, i32)) -> Option<u8> {
    SIDES.iter().find(|(_, step)| (from.0 + step.0, from.1 + step.1) == to).map(|(side, _)| *side)
}

/// Each cell once, in the order first reached.
fn distinct(steps: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut seen = HashSet::new();
    steps.iter().copied().filter(|&cell| seen.insert(cell)).collect()
}

impl Session {
    /// Whether the fork has every piece of a network.
    pub fn pipe_network_available(&self, network: PipeNetwork) -> bool {
        network.pieces().ids().iter().all(|id| self.prototype(id).is_some())
    }

    /// The ops that give a cell the piece its sides call for: none where the pipe there is
    /// that piece already turned that way, a turn where it is of the same prototype, and else
    /// a new pipe in place of the old one. A pipe turned keeps its uid and its data.
    fn refit_ops(&self, grid: i32, cell: (i32, i32), sides: u8, laid: Option<Laid>, pieces: PipePieces, ops: &mut Vec<Value>) {
        let (piece, quarters) = fit(sides);
        match laid {
            Some(laid) if laid.piece == piece && laid.sides == open_sides(piece, quarters) => {}
            // Disposal's junction stands in for a four-way, and may be fitted already as one.
            Some(laid) if pieces.id(laid.piece) == pieces.id(piece) && laid.quarters == quarters => {}
            Some(laid) if pieces.id(laid.piece) == pieces.id(piece) => ops.push(json!({ "kind": "rotate", "uid": laid.uid, "rotation": angle(quarters) })),
            Some(laid) => {
                ops.push(json!({ "kind": "delete", "uid": laid.uid }));
                ops.push(self.pipe_spawn(grid, cell, pieces.id(piece), quarters));
            }
            None => ops.push(self.pipe_spawn(grid, cell, pieces.id(piece), quarters)),
        }
    }

    /// A new pipe in the middle of a cell. A gas pipe takes the chosen colour.
    fn pipe_spawn(&self, grid: i32, cell: (i32, i32), prototype: &str, quarters: i32) -> Value {
        let mut op = json!({ "kind": "spawn", "prototype": prototype, "parent": grid, "x": cell.0 as f32 + 0.5, "y": cell.1 as f32 + 0.5, "rotation": angle(quarters) });
        if let (PipeNetwork::Gas, Some(colour)) = (self.laying.network(), self.laying.colour()) {
            op["components"] = json!(json!([{ "type": "AtmosPipeColor", "color": colour }]).to_string());
        }
        op
    }

    /// Lays a pipe along the steps of a path on a grid, as one edit. Each cell with a tile
    /// gets the piece its sides call for (the module's rule), and a pipe of the network that
    /// was there is refitted.
    pub(super) fn lay_pipes(&mut self, grid: i32, steps: &[(i32, i32)]) {
        let network = self.laying.network();
        if !self.pipe_network_available(network) {
            self.set_status(format!("Not laid: the fork has no {}", network.pieces().ids().iter().find(|id| self.prototype(id).is_none()).unwrap_or(&"")));
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let pieces = network.pieces();
        let laid = pipes_on(doc, grid, pieces);
        let cells: Vec<(i32, i32)> = distinct(steps).into_iter().filter(|&cell| doc.tile_at(grid, cell) != SPACE).collect();
        let mut sides: HashMap<(i32, i32), u8> = cells.iter().map(|cell| (*cell, 0)).collect();
        for pair in steps.windows(2) {
            if let (Some(side), true) = (side_towards(pair[0], pair[1]), sides.contains_key(&pair[0]) && sides.contains_key(&pair[1])) {
                *sides.get_mut(&pair[0]).unwrap() |= side;
                *sides.get_mut(&pair[1]).unwrap() |= opposite(side);
            }
        }
        for cell in &cells {
            for (side, step) in SIDES {
                if laid.get(&(cell.0 + step.0, cell.1 + step.1)).is_some_and(|next| next.sides & opposite(side) != 0) {
                    *sides.get_mut(cell).unwrap() |= side;
                }
            }
        }
        let mut ops = Vec::new();
        for cell in &cells {
            self.refit_ops(grid, *cell, sides[cell], laid.get(cell).copied(), pieces, &mut ops);
        }
        self.edit(ops, "Lay pipes", None, false);
    }

    /// Removes the pipes of the network on the cells of a path on a grid, and refits the
    /// pipes next to them that were open towards them, as one edit.
    pub(super) fn remove_pipes(&mut self, grid: i32, steps: &[(i32, i32)]) {
        let Some(doc) = self.doc.as_ref() else { return };
        let pieces = self.laying.network().pieces();
        let cells: HashSet<(i32, i32)> = steps.iter().copied().collect();
        let mut gone: Vec<i32> = doc
            .entities
            .values()
            .filter(|entity| entity.parent == grid && !entity.contained && pieces.piece_of(&entity.prototype).is_some() && cells.contains(&(entity.x.floor() as i32, entity.y.floor() as i32)))
            .map(|entity| entity.uid)
            .collect();
        gone.sort_unstable();
        let removed: HashSet<(i32, i32)> = gone.iter().filter_map(|uid| doc.entities.get(uid)).map(|entity| (entity.x.floor() as i32, entity.y.floor() as i32)).collect();
        let laid = pipes_on(doc, grid, pieces);
        let mut ops: Vec<Value> = gone.iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        let mut refitted = HashSet::new();
        for cell in distinct(steps) {
            if !removed.contains(&cell) {
                continue;
            }
            for (side, step) in SIDES {
                let next = (cell.0 + step.0, cell.1 + step.1);
                let Some(pipe) = laid.get(&next).copied() else { continue };
                if removed.contains(&next) || pipe.sides & opposite(side) == 0 || !refitted.insert(next) {
                    continue;
                }
                // Every side it loses at once, so that a pipe between two removed cells is
                // refitted once.
                let lost = SIDES.iter().filter(|(_, step)| removed.contains(&(next.0 + step.0, next.1 + step.1))).fold(0, |lost, (side, _)| lost | side);
                self.refit_ops(grid, next, pipe.sides & !lost, Some(pipe), pieces, &mut ops);
            }
        }
        self.edit(ops, "Remove pipes", None, false);
    }
}
