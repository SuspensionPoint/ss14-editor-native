//! The tools as the toolbar offers them, the way an art program has them: a few tools, and
//! options that say what each works on and in what shape.
//!
//! The toolbar's tools (`BarTool`) are not the session's tools (`session::Tool`). The session
//! keeps the tools it had before the toolbar was made this way, each with its own press, drag
//! and keys. A toolbar tool with its options stands for one of them (`Toolbox::tool_for`), and
//! each of them for one toolbar tool (`BarTool::of`):
//!
//! - Paint with a shape of Point is the session's Paint, or Place with Entities as the target.
//!   Line, Rectangle, Circle, Lasso and Polygon are the session's tools of those names.
//! - Erase is the session's Erase, whatever the shape. The eraser reads its shape at the press.
//!   It takes away everything of its target that can be seen, or with Only on just the kind in
//!   hand (`ToolOptions::erase_only`).
//! - Fill is the session's Fill, on any target.
//! - Select with the Tiles target is the session's Area tool, a region of cells with everything
//!   on them. Without it, it is the session's Select tool, for entities and decals.
//! - Every other toolbar tool is the session's tool of the same name.
//!
//! The target is what Paint and Erase work on. It follows the palette: choosing a tile makes it
//! Tiles. Setting it gives back what was last in hand for it (`Toolbox::recall`).
//!
//! The closed shapes of Paint and Erase (Rectangle, Circle, Lasso and Polygon) are filled or
//! just their edge (`ShapeFill`), read at the press.
//!
//! The shapes, Fill or Outline, the select tool's targets and shape, Select same's Contiguous,
//! free placing and Erase's Only are kept in the settings file (`ToolOptions`).

use serde::{Deserialize, Serialize};

use crate::session::{Palette, Tool};

/// A tool as the toolbar offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarTool {
    Select,
    Pan,
    Pick,
    Paint,
    Erase,
    Fill,
    Room,
    Link,
    Cables,
    Pipes,
}

impl BarTool {
    /// The tools in the toolbar's order, in its three groups (`group`).
    pub const ALL: [BarTool; 10] = [BarTool::Select, BarTool::Pan, BarTool::Pick, BarTool::Paint, BarTool::Erase, BarTool::Fill, BarTool::Room, BarTool::Link, BarTool::Cables, BarTool::Pipes];

    /// Its name, as its hover label and the list of controls give it.
    pub fn name(self) -> &'static str {
        match self {
            BarTool::Select => "Select",
            BarTool::Pan => "Pan",
            BarTool::Pick => "Pick",
            BarTool::Paint => "Paint",
            BarTool::Erase => "Erase",
            BarTool::Fill => "Fill",
            BarTool::Room => "Room",
            BarTool::Link => "Link",
            BarTool::Cables => "Cables",
            BarTool::Pipes => "Pipes",
        }
    }

    /// The key that takes it in hand.
    pub fn key(self) -> char {
        match self {
            BarTool::Select => 'V',
            BarTool::Pan => 'H',
            BarTool::Pick => 'I',
            BarTool::Paint => 'B',
            BarTool::Erase => 'E',
            BarTool::Fill => 'G',
            BarTool::Room => 'O',
            BarTool::Link => 'D',
            BarTool::Cables => 'K',
            BarTool::Pipes => 'J',
        }
    }

    /// Its group on the toolbar: choosing and moving about, putting things down, and wiring.
    pub fn group(self) -> usize {
        match self {
            BarTool::Select | BarTool::Pan | BarTool::Pick => 0,
            BarTool::Paint | BarTool::Erase | BarTool::Fill | BarTool::Room => 1,
            BarTool::Link | BarTool::Cables | BarTool::Pipes => 2,
        }
    }

    /// The toolbar tool a session tool belongs to.
    pub fn of(tool: Tool) -> BarTool {
        match tool {
            Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Lasso | Tool::Polygon | Tool::Place => BarTool::Paint,
            Tool::Erase => BarTool::Erase,
            Tool::Select | Tool::Area => BarTool::Select,
            Tool::Pan => BarTool::Pan,
            Tool::Pick => BarTool::Pick,
            Tool::Fill => BarTool::Fill,
            Tool::Room => BarTool::Room,
            Tool::Link => BarTool::Link,
            Tool::Cable => BarTool::Cables,
            Tool::Pipe => BarTool::Pipes,
        }
    }

    /// Whether the options bar offers a target for it: Paint, Erase and Fill work on any of
    /// the three.
    pub fn has_target(self) -> bool {
        matches!(self, BarTool::Paint | BarTool::Erase | BarTool::Fill)
    }
}

/// What Paint and Erase work on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Tiles,
    Entities,
    Decals,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Tiles, Target::Entities, Target::Decals];

    pub fn name(self) -> &'static str {
        match self {
            Target::Tiles => "Tiles",
            Target::Entities => "Entities",
            Target::Decals => "Decals",
        }
    }

    /// What the palette holds, as a target. None when it holds nothing.
    pub fn of(palette: &Palette) -> Option<Target> {
        match palette {
            Palette::None => None,
            Palette::Tile(_) => Some(Target::Tiles),
            Palette::Entity(_) => Some(Target::Entities),
            Palette::Decal(_) => Some(Target::Decals),
        }
    }
}

/// The shape Paint and Erase put down or take away in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// A brush that works on each cell the cursor crosses.
    #[default]
    Point,
    Line,
    Rectangle,
    Circle,
    /// A freehand line dragged around the cells, closed when the button comes up.
    Lasso,
    /// Corners clicked one at a time, closed on the first corner, by a double-click or Enter.
    Polygon,
}

impl Shape {
    pub const ALL: [Shape; 6] = [Shape::Point, Shape::Line, Shape::Rectangle, Shape::Circle, Shape::Lasso, Shape::Polygon];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Point => "Point",
            Shape::Line => "Line",
            Shape::Rectangle => "Rectangle",
            Shape::Circle => "Circle",
            Shape::Lasso => "Lasso",
            Shape::Polygon => "Polygon",
        }
    }

    /// Whether it has an inside, so that it can be filled or be just its edge (`ShapeFill`).
    pub fn closed(self) -> bool {
        matches!(self, Shape::Rectangle | Shape::Circle | Shape::Lasso | Shape::Polygon)
    }

    /// The session tool that paints in it, Point aside, which depends on the target.
    fn paint_tool(self) -> Option<Tool> {
        match self {
            Shape::Point => None,
            Shape::Line => Some(Tool::Line),
            Shape::Rectangle => Some(Tool::Rectangle),
            Shape::Circle => Some(Tool::Circle),
            Shape::Lasso => Some(Tool::Lasso),
            Shape::Polygon => Some(Tool::Polygon),
        }
    }
}

/// Whether a closed shape of Paint or Erase works on every cell inside it or only on its edge:
/// the cells of it that touch a cell outside it side by side. A rectangle's edge is its border
/// and a circle's its ring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeFill {
    #[default]
    Fill,
    Outline,
}

impl ShapeFill {
    pub const ALL: [ShapeFill; 2] = [ShapeFill::Fill, ShapeFill::Outline];

    pub fn name(self) -> &'static str {
        match self {
            ShapeFill::Fill => "Fill",
            ShapeFill::Outline => "Outline",
        }
    }
}

/// The shape Select selects in, as an art program's marquee, lasso and magic wand. Each works
/// on a region of cells with Tiles on, and on entities and decals with it off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectShape {
    /// A box dragged from corner to corner.
    #[default]
    Rectangle,
    /// An ellipse inside the box dragged from corner to corner.
    Ellipse,
    /// A freehand line dragged around what is wanted, closed when the button comes up.
    Lasso,
    /// Corners clicked one at a time, closed on the first corner, by a double-click or Enter.
    Polygon,
    /// A click on a tile, an entity or a decal takes every one of its kind, touching or on the
    /// whole grid (`ToolOptions::select_contiguous`).
    Same,
}

impl SelectShape {
    pub const ALL: [SelectShape; 5] = [SelectShape::Rectangle, SelectShape::Ellipse, SelectShape::Lasso, SelectShape::Polygon, SelectShape::Same];

    pub fn name(self) -> &'static str {
        match self {
            SelectShape::Rectangle => "Rectangle",
            SelectShape::Ellipse => "Ellipse",
            SelectShape::Lasso => "Lasso",
            SelectShape::Polygon => "Polygon",
            SelectShape::Same => "Select same",
        }
    }
}

/// How a box of the select tool changes what is selected. Shift and Ctrl give Add and
/// Subtract for one press whatever the mode is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Subtract,
}

impl SelectMode {
    pub const ALL: [SelectMode; 3] = [SelectMode::Replace, SelectMode::Add, SelectMode::Subtract];

    pub fn name(self) -> &'static str {
        match self {
            SelectMode::Replace => "Replace",
            SelectMode::Add => "Add",
            SelectMode::Subtract => "Subtract",
        }
    }

    /// The mode a press goes by: Ctrl subtracts and Shift adds, Ctrl first, and with neither
    /// held it is the mode chosen.
    pub fn with_keys(self, shift: bool, ctrl: bool) -> SelectMode {
        match (ctrl, shift) {
            (true, _) => SelectMode::Subtract,
            (false, true) => SelectMode::Add,
            (false, false) => self,
        }
    }
}

/// What the select tool selects. With Tiles on it selects a region of cells, which takes
/// everything on them along. With Tiles off it selects entities, decals or both. At least one
/// of the three is always on, and no switch is ever dead (`toggle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectTargets {
    pub entities: bool,
    pub decals: bool,
    pub tiles: bool,
}

impl Default for SelectTargets {
    fn default() -> Self {
        SelectTargets { entities: true, decals: true, tiles: false }
    }
}

impl SelectTargets {
    /// Whether a target is on.
    pub fn has(self, target: Target) -> bool {
        match target {
            Target::Tiles => self.tiles,
            Target::Entities => self.entities,
            Target::Decals => self.decals,
        }
    }

    /// Whether the options bar shows a target as on. With Tiles on the region takes
    /// everything, so Entities and Decals show off, and pressing one leaves the region for it.
    pub fn lit(self, target: Target) -> bool {
        match target {
            Target::Tiles => self.tiles,
            Target::Entities | Target::Decals => !self.tiles && self.has(target),
        }
    }

    /// A press of a target's switch. Tiles goes on or off. Off, it gives back Entities and
    /// Decals as they were, or both if neither was on. With Tiles on, Entities or Decals turns
    /// Tiles off and selects that one alone. With Tiles off they go on and off, and the last
    /// one on stays on, which gives back false.
    pub fn toggle(&mut self, target: Target) -> bool {
        match target {
            Target::Tiles => {
                self.tiles ^= true;
                if !(self.tiles || self.entities || self.decals) {
                    (self.entities, self.decals) = (true, true);
                }
            }
            Target::Entities | Target::Decals if self.tiles => {
                *self = SelectTargets { entities: target == Target::Entities, decals: target == Target::Decals, tiles: false };
            }
            Target::Entities | Target::Decals => {
                let mut next = *self;
                match target {
                    Target::Entities => next.entities ^= true,
                    _ => next.decals ^= true,
                }
                if !(next.entities || next.decals) {
                    return false;
                }
                *self = next;
            }
        }
        true
    }
}

/// The tool options kept in the settings file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolOptions {
    pub paint_shape: Shape,
    pub erase_shape: Shape,
    /// Whether Paint's closed shapes are filled or just their edge.
    pub paint_fill: ShapeFill,
    /// Whether Erase's closed shapes are filled or just their edge.
    pub erase_fill: ShapeFill,
    pub select: SelectTargets,
    /// Whether Paint at a point puts an entity or a decal where the cursor is, and not on its
    /// cell. Shift does the other for one press.
    #[serde(alias = "entity_free")]
    pub place_free: bool,
    /// Whether Erase takes away only the kind in hand, and not everything of its target.
    pub erase_only: bool,
    /// The shape Select selects in.
    pub select_shape: SelectShape,
    /// Whether Select same takes only what touches the thing clicked, cell by cell, and not
    /// every one of its kind on the grid.
    pub select_contiguous: bool,
}

impl Default for ToolOptions {
    fn default() -> Self {
        ToolOptions {
            paint_shape: Shape::default(),
            erase_shape: Shape::default(),
            paint_fill: ShapeFill::default(),
            erase_fill: ShapeFill::default(),
            select: SelectTargets::default(),
            place_free: false,
            erase_only: false,
            select_shape: SelectShape::default(),
            select_contiguous: true,
        }
    }
}

/// What a tool key does when the tool in hand has not taken it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Takes a toolbar tool in hand with its options as they are.
    Take(BarTool),
    /// Takes Paint in hand with this shape.
    PaintShape(Shape),
    /// Takes Paint in hand to place entities one at a time: Entities, Point.
    PlaceEntities,
    /// Takes Select in hand with the Tiles target on, for a region.
    SelectRegion,
    /// Takes Select in hand with the Tiles target off, for entities and decals.
    SelectThings,
}

/// Every key that a tool takes or that takes a tool, as the interface hands them on.
pub const TOOL_KEYS: [char; 15] = ['V', 'H', 'I', 'B', 'E', 'G', 'O', 'D', 'K', 'J', 'P', 'L', 'C', 'R', 'S'];

/// What a tool key does. The keys are the ones mappers knew before the toolbar was made
/// this way: P places entities, R, L and C paint a rectangle, a line and a circle, S selects a
/// region and V selects entities and decals. R turns things first where the tool in hand
/// takes it.
pub fn key_action(key: char) -> Option<KeyAction> {
    let key = key.to_ascii_uppercase();
    Some(match key {
        'V' => KeyAction::SelectThings,
        'S' => KeyAction::SelectRegion,
        'P' => KeyAction::PlaceEntities,
        'R' => KeyAction::PaintShape(Shape::Rectangle),
        'L' => KeyAction::PaintShape(Shape::Line),
        'C' => KeyAction::PaintShape(Shape::Circle),
        _ => KeyAction::Take(BarTool::ALL.into_iter().find(|tool| tool.key() == key)?),
    })
}

/// The toolbar's state beside the session's tool: the target, the options, the select mode,
/// and what was last in hand for each target.
#[derive(Clone, Debug, PartialEq)]
pub struct Toolbox {
    pub target: Target,
    pub options: ToolOptions,
    pub mode: SelectMode,
    last_tile: Option<String>,
    last_entity: Option<usize>,
    last_decal: Option<String>,
}

impl Default for Toolbox {
    fn default() -> Self {
        Toolbox { target: Target::Tiles, options: ToolOptions::default(), mode: SelectMode::Replace, last_tile: None, last_entity: None, last_decal: None }
    }
}

impl Toolbox {
    /// A toolbox with these options kept from before.
    pub fn with_options(options: ToolOptions) -> Toolbox {
        Toolbox { options, ..Toolbox::default() }
    }

    /// The session tool a toolbar tool stands for with the options as they are.
    pub fn tool_for(&self, bar: BarTool) -> Tool {
        match bar {
            BarTool::Paint => match (self.options.paint_shape.paint_tool(), self.target) {
                (Some(tool), _) => tool,
                (None, Target::Entities) => Tool::Place,
                (None, Target::Tiles | Target::Decals) => Tool::Paint,
            },
            BarTool::Erase => Tool::Erase,
            BarTool::Select if self.options.select.tiles => Tool::Area,
            BarTool::Select => Tool::Select,
            BarTool::Pan => Tool::Pan,
            BarTool::Pick => Tool::Pick,
            BarTool::Fill => Tool::Fill,
            BarTool::Room => Tool::Room,
            BarTool::Link => Tool::Link,
            BarTool::Cables => Tool::Cable,
            BarTool::Pipes => Tool::Pipe,
        }
    }

    /// The shape of a toolbar tool that has one: Paint's or Erase's.
    pub fn shape(&self, bar: BarTool) -> Option<Shape> {
        match bar {
            BarTool::Paint => Some(self.options.paint_shape),
            BarTool::Erase => Some(self.options.erase_shape),
            _ => None,
        }
    }

    /// Sets the shape of Paint or Erase. Gives back whether it was one of those.
    pub fn set_shape(&mut self, bar: BarTool, shape: Shape) -> bool {
        match bar {
            BarTool::Paint => self.options.paint_shape = shape,
            BarTool::Erase => self.options.erase_shape = shape,
            _ => return false,
        }
        true
    }

    /// Whether the closed shapes of Paint or Erase are filled or just their edge.
    pub fn fill(&self, bar: BarTool) -> Option<ShapeFill> {
        match bar {
            BarTool::Paint => Some(self.options.paint_fill),
            BarTool::Erase => Some(self.options.erase_fill),
            _ => None,
        }
    }

    /// Sets whether the closed shapes of Paint or Erase are filled or just their edge. Gives
    /// back whether it was one of those.
    pub fn set_fill(&mut self, bar: BarTool, fill: ShapeFill) -> bool {
        match bar {
            BarTool::Paint => self.options.paint_fill = fill,
            BarTool::Erase => self.options.erase_fill = fill,
            _ => return false,
        }
        true
    }

    /// Takes in a session tool that was taken in hand some other way, as by a key or a choice
    /// in a palette, so that the toolbar shows what it stands for. A shape tool is Paint's
    /// shape, the place tool is Paint at a point on entities, the area tool is Select with
    /// Tiles on, and the select tool is Select with Tiles off.
    pub fn note_tool(&mut self, tool: Tool) {
        match tool {
            Tool::Line => self.options.paint_shape = Shape::Line,
            Tool::Rectangle => self.options.paint_shape = Shape::Rectangle,
            Tool::Circle => self.options.paint_shape = Shape::Circle,
            Tool::Lasso => self.options.paint_shape = Shape::Lasso,
            Tool::Polygon => self.options.paint_shape = Shape::Polygon,
            Tool::Place => {
                self.options.paint_shape = Shape::Point;
                self.target = Target::Entities;
            }
            Tool::Paint => self.options.paint_shape = Shape::Point,
            Tool::Area => self.options.select.tiles = true,
            Tool::Select => {
                self.options.select.tiles = false;
                if !(self.options.select.entities || self.options.select.decals) {
                    self.options.select.entities = true;
                    self.options.select.decals = true;
                }
            }
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Room | Tool::Link | Tool::Cable | Tool::Pipe => {}
        }
    }

    /// Notes what the palette holds as last in hand for its target, and makes that the target.
    pub fn remember(&mut self, palette: &Palette) {
        match palette {
            Palette::None => return,
            Palette::Tile(id) => self.last_tile = Some(id.clone()),
            Palette::Entity(index) => self.last_entity = Some(*index),
            Palette::Decal(id) => self.last_decal = Some(id.clone()),
        }
        if let Some(target) = Target::of(palette) {
            self.target = target;
        }
    }

    /// Notes a tile as last in hand for the Tiles target, and leaves the target as it is.
    pub fn remember_tile(&mut self, id: &str) {
        self.last_tile = Some(id.to_owned());
    }

    /// What was last in hand for a target, as the palette holds it. Nothing if none was.
    pub fn recall(&self, target: Target) -> Palette {
        let recalled = match target {
            Target::Tiles => self.last_tile.clone().map(Palette::Tile),
            Target::Entities => self.last_entity.map(Palette::Entity),
            Target::Decals => self.last_decal.clone().map(Palette::Decal),
        };
        recalled.unwrap_or(Palette::None)
    }
}

#[cfg(test)]
#[path = "tests/toolbox.rs"]
mod tests;
