//! The cable and pipe tools: what is chosen for them, the path a drag of either lays, and the
//! cables. The pipes' fittings are in `pipes.rs`.
//!
//! A press of the primary button begins a path on the grid under it and a press of the
//! secondary one a path that removes. The path goes from cell to cell by their sides, with
//! no gaps however fast the drag, and is shown filled while it is dragged. Nothing is asked
//! of the engine until the button comes up: then the path is one edit with no stroke. A cell
//! with no tile is left out. A kind the fork does not have is dragged out all the same and
//! sets nothing, with a line on the status bar.

use super::pipes::{PipeNetwork, PIPE_COLOURS};
use super::*;

/// The fill of a path that removes.
const REMOVE_FILL: [f32; 4] = [1.0, 0.25, 0.25, 0.27];

/// The alpha of a path's fill, the web editor's `44`.
const PATH_ALPHA: f32 = 0x44 as f32 / 255.0;

/// A kind of cable, as the web editor offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CableKind {
    High,
    Medium,
    Apc,
}

impl CableKind {
    pub const ALL: [CableKind; 3] = [CableKind::High, CableKind::Medium, CableKind::Apc];

    /// Its prototype, the web editor's name.
    pub fn id(self) -> &'static str {
        match self {
            CableKind::High => "CableHV",
            CableKind::Medium => "CableMV",
            CableKind::Apc => "CableApcExtension",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CableKind::High => "HV Cable",
            CableKind::Medium => "MV Cable",
            CableKind::Apc => "APC Cable",
        }
    }

    /// The colour the web editor marks it with.
    pub fn colour(self) -> (u8, u8, u8) {
        match self {
            CableKind::High => (0xff, 0x88, 0x00),
            CableKind::Medium => (0xff, 0xcc, 0x00),
            CableKind::Apc => (0x00, 0xcc, 0x44),
        }
    }

    /// The names its edits go by in the history.
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            CableKind::High => ("Lay HV cable", "Remove HV cable"),
            CableKind::Medium => ("Lay MV cable", "Remove MV cable"),
            CableKind::Apc => ("Lay APC cable", "Remove APC cable"),
        }
    }
}

/// What the cable and pipe tools lay: a kind of cable, a network of pipe, and the colour of a
/// gas pipe, none for the prototype's own.
pub(in crate::session) struct Laying {
    cable: CableKind,
    network: PipeNetwork,
    colour: Option<String>,
}

impl Default for Laying {
    fn default() -> Self {
        Laying { cable: CableKind::High, network: PipeNetwork::Gas, colour: Some(PIPE_COLOURS[0].1.to_owned()) }
    }
}

impl Laying {
    pub(super) fn network(&self) -> PipeNetwork {
        self.network
    }

    pub(super) fn colour(&self) -> Option<&str> {
        self.colour.as_deref()
    }
}

/// A path being dragged out on one grid by the cable or the pipe tool.
pub(in crate::session) struct PathDrag {
    grid: i32,
    /// The cells in the order the cursor went through them, each next to the one before.
    /// A cell crossed twice is there twice, so that the pipes know which cells follow which.
    steps: Vec<(i32, i32)>,
    /// Whether it removes, from a press of the secondary button.
    remove: bool,
}

/// The cells from one cell to another, both included, each next to the one before by a side:
/// the brush's line, with a corner put in where it steps across one.
pub(super) fn side_steps(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let mut steps = vec![from];
    for cell in cells_between(from, to).into_iter().skip(1) {
        let last = *steps.last().unwrap();
        if last.0 != cell.0 && last.1 != cell.1 {
            steps.push((cell.0, last.1));
        }
        steps.push(cell);
    }
    steps
}

impl Session {
    /// The kind of cable the cable tool lays.
    pub fn cable_kind(&self) -> CableKind {
        self.laying.cable
    }

    /// Chooses the kind of cable, and takes the cable tool in hand.
    pub fn choose_cable(&mut self, kind: CableKind) {
        self.laying.cable = kind;
        self.set_tool(Tool::Cable);
    }

    /// Whether the fork has a kind of cable.
    pub fn cable_available(&self, kind: CableKind) -> bool {
        self.prototype(kind.id()).is_some()
    }

    /// The network the pipe tool lays.
    pub fn pipe_network(&self) -> PipeNetwork {
        self.laying.network
    }

    /// The colour a gas pipe is laid in, as #RRGGBBAA, or None for the prototype's own.
    pub fn pipe_colour(&self) -> Option<&str> {
        self.laying.colour()
    }

    /// Chooses the network and the colour of a gas pipe, and takes the pipe tool in hand.
    pub fn choose_pipe(&mut self, network: PipeNetwork, colour: Option<String>) {
        self.laying.network = network;
        self.laying.colour = colour;
        self.set_tool(Tool::Pipe);
    }

    /// The cable or pipe tool was pressed: begins a path at the cell under it, which removes
    /// when the secondary button was pressed.
    pub(super) fn path_down(&mut self, world: (f32, f32), remove: bool) {
        if let Some((grid, cell)) = self.cell_at(world) {
            self.drag = Drag::Path(PathDrag { grid, steps: vec![cell], remove });
        }
    }

    /// A path was dragged on: it goes on to the cursor's cell.
    pub(super) fn path_dragged(&self, world: (f32, f32), mut drag: PathDrag) -> Drag {
        let last = *drag.steps.last().unwrap();
        let cell = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)).map_or(last, |grid| grid.cell_at(world));
        if cell != last {
            drag.steps.extend(side_steps(last, cell).into_iter().skip(1));
        }
        Drag::Path(drag)
    }

    /// A path was let go: lays or removes along it, as one edit.
    pub(super) fn path_up(&mut self, drag: PathDrag) {
        match (self.tool, drag.remove) {
            (Tool::Cable, false) => self.lay_cables(drag.grid, &drag.steps),
            (Tool::Cable, true) => self.remove_cables(drag.grid, &drag.steps),
            (Tool::Pipe, false) => self.lay_pipes(drag.grid, &drag.steps),
            (Tool::Pipe, true) => self.remove_pipes(drag.grid, &drag.steps),
            _ => {}
        }
    }

    /// Lays a cable of the chosen kind on each cell of a path that has a tile and has none of
    /// that kind, as one edit.
    fn lay_cables(&mut self, grid: i32, steps: &[(i32, i32)]) {
        let kind = self.laying.cable;
        if !self.cable_available(kind) {
            self.set_status(format!("Not laid: the fork has no {}", kind.id()));
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let holding = tiles::cells_holding(doc, grid, kind.id());
        let mut seen = HashSet::new();
        let ops = steps
            .iter()
            .filter(|&&cell| doc.tile_at(grid, cell) != SPACE && !holding.contains(&cell) && seen.insert(cell))
            .map(|cell| json!({ "kind": "spawn", "prototype": kind.id(), "parent": grid, "x": cell.0 as f32 + 0.5, "y": cell.1 as f32 + 0.5, "rotation": 0.0 }))
            .collect();
        self.edit(ops, kind.labels().0, None, false);
    }

    /// Removes the cables of the chosen kind on the cells of a path, as one edit.
    fn remove_cables(&mut self, grid: i32, steps: &[(i32, i32)]) {
        let kind = self.laying.cable;
        let Some(doc) = self.doc.as_ref() else { return };
        let cells: HashSet<(i32, i32)> = steps.iter().copied().collect();
        let mut uids: Vec<i32> = doc
            .entities
            .values()
            .filter(|entity| entity.parent == grid && !entity.contained && entity.prototype == kind.id() && cells.contains(&(entity.x.floor() as i32, entity.y.floor() as i32)))
            .map(|entity| entity.uid)
            .collect();
        uids.sort_unstable();
        let ops = uids.into_iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        self.edit(ops, kind.labels().1, None, false);
    }

    /// The label of a path being dragged, for `drag_label`: its length, the number of cells of
    /// it that have a tile, each counted once, over the cursor's cell.
    pub(super) fn path_label(&self, drag: &PathDrag) -> Option<((f32, f32), String)> {
        let doc = self.doc.as_ref()?;
        let grid = doc.grid(drag.grid)?;
        let cells: HashSet<(i32, i32)> = drag.steps.iter().copied().filter(|&cell| doc.tile_at(drag.grid, cell) != SPACE).collect();
        let last = *drag.steps.last()?;
        Some((grid.to_world((last.0 as f32 + 0.5, last.1 as f32 + 1.0)), cells.len().to_string()))
    }

    /// The colour a path is filled with while it is dragged: the kind's, or red for removing.
    fn path_fill(&self, remove: bool) -> [f32; 4] {
        if remove {
            return REMOVE_FILL;
        }
        let rgb = match self.tool {
            Tool::Pipe => match (self.laying.network, self.laying.colour()) {
                // The web editor's brown for disposal.
                (PipeNetwork::Disposal, _) => (0x88, 0x66, 0x44),
                (PipeNetwork::Gas, Some(colour)) => decal_colour_bytes(colour).map_or((0xaa, 0xaa, 0xaa), |bytes| (bytes[0], bytes[1], bytes[2])),
                (PipeNetwork::Gas, None) => (0xaa, 0xaa, 0xaa),
            },
            _ => self.laying.cable.colour(),
        };
        [rgb.0 as f32 / 255.0, rgb.1 as f32 / 255.0, rgb.2 as f32 / 255.0, PATH_ALPHA]
    }

    /// The cells of a path being dragged, filled, for `overlay_quads`.
    pub(super) fn path_overlay(&self, over: &mut Vec<f32>) {
        let Drag::Path(drag) = &self.drag else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)) else { return };
        let fill = self.path_fill(drag.remove);
        let mut seen = HashSet::new();
        for &cell in drag.steps.iter().filter(|&&cell| seen.insert(cell)) {
            for corner in grid.cells_outline(cell, cell) {
                over.extend_from_slice(&[corner.0, corner.1]);
            }
            over.extend_from_slice(&fill);
            over.push(WHITE_TEXTURE as f32);
            over.extend_from_slice(&WHITE_SOURCE);
        }
    }
}
