//! How the map looks under the mouse for each tool, as the web editor has it: the colour of the
//! outline of the cell under the cursor, and the mouse pointer.

use super::*;

/// How the cell under the cursor is outlined ([`OutlineStyle::Hover`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverLook {
    /// The edge, as red, green, blue and alpha.
    pub edge: [u8; 4],
    /// A faint fill of the cell, for the tools that have one.
    pub fill: Option<[u8; 4]>,
    /// Whether the edge is dashed.
    pub dashed: bool,
    /// Whether an X goes from corner to corner, as the eraser has.
    pub cross: bool,
}

impl HoverLook {
    const fn edge(edge: [u8; 4]) -> HoverLook {
        HoverLook { edge, fill: None, dashed: false, cross: false }
    }
}

/// The mouse pointer over the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerLook {
    /// The system's arrow.
    Default,
    Crosshair,
    /// An open hand, which pans when pressed.
    Grab,
    /// A closed hand, panning.
    Grabbing,
}

/// The web editor's colour of a disposal pipe, and of a gas pipe laid in the prototype's own colour.
const DISPOSAL: [u8; 4] = [0x88, 0x66, 0x44, 255];
const GAS: [u8; 4] = [0x00, 0x88, 0xff, 255];

impl Session {
    /// How the cell under the cursor is outlined with the tool in hand: Paint green, Erase red
    /// with an X, Pick yellow and dashed, Fill green over a faint green, Rectangle in the
    /// accent, Line gold, Circle cyan, Room orange, the cable and pipe tools in the colour they lay, Area
    /// white and dashed, and Place green and dashed, blue with Shift.
    pub fn hover_look(&self) -> HoverLook {
        match self.tool {
            Tool::Paint => HoverLook::edge([0x00, 0xff, 0x00, 255]),
            Tool::Erase => HoverLook { cross: true, ..HoverLook::edge([0xff, 0x44, 0x44, 255]) },
            Tool::Pick => HoverLook { dashed: true, ..HoverLook::edge([0xff, 0xff, 0x00, 255]) },
            Tool::Fill => HoverLook { fill: Some([0x00, 0xff, 0x00, 38]), ..HoverLook::edge([0x00, 0xff, 0x88, 255]) },
            Tool::Rectangle => HoverLook::edge([0x44, 0x88, 0xff, 255]),
            Tool::Line => HoverLook::edge([0xff, 0xcc, 0x00, 255]),
            Tool::Circle => HoverLook::edge([0x00, 0xcc, 0xff, 255]),
            Tool::Room => HoverLook::edge([0xff, 0x88, 0x44, 255]),
            Tool::Cable => {
                let (red, green, blue) = self.cable_kind().colour();
                HoverLook::edge([red, green, blue, 255])
            }
            Tool::Pipe => HoverLook::edge(match self.pipe_network() {
                PipeNetwork::Disposal => DISPOSAL,
                PipeNetwork::Gas => self.pipe_colour().and_then(decal_colour_bytes).map_or(GAS, |[red, green, blue, _]| [red, green, blue, 255]),
            }),
            Tool::Place if self.held.shift => HoverLook { dashed: true, ..HoverLook::edge([100, 200, 255, 153]) },
            Tool::Place => HoverLook { dashed: true, ..HoverLook::edge([0, 255, 100, 153]) },
            Tool::Area | Tool::Pan | Tool::Select | Tool::Link => HoverLook { dashed: true, ..HoverLook::edge([255, 255, 255, 255]) },
        }
    }

    /// The mouse pointer over the map: a closed hand while panning, an open hand with Pan in
    /// hand or Space held, the arrow with Select, and a crosshair with every other tool.
    pub fn pointer_look(&self) -> PointerLook {
        if matches!(self.drag, Drag::Pan { .. }) {
            PointerLook::Grabbing
        } else if self.tool == Tool::Pan || self.held.space {
            PointerLook::Grab
        } else if self.tool == Tool::Select {
            PointerLook::Default
        } else {
            PointerLook::Crosshair
        }
    }
}
