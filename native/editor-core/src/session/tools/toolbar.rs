//! The toolbar's tools and options on the session (`crate::toolbox` has the model): which
//! toolbar tool is in hand, its target and shape, what Select selects and how, and where the
//! options are kept.
//!
//! Every change goes through `set_tool` in the end, which notes the session tool in the
//! toolbox (`Toolbox::note_tool`), so the toolbar shows what is in hand however it was taken.
//! A change of the options is written to the settings file when there is one.

use super::*;
use crate::forks::settings::Settings;
use crate::toolbox::{BarTool, KeyAction, SelectMode, SelectTargets, Shape, Target, ToolOptions, Toolbox};

/// The toolbar's state, and the settings file its options are kept in.
#[derive(Default)]
pub(in crate::session) struct Tooling {
    pub(in crate::session) toolbox: Toolbox,
    /// None keeps the options in the session only.
    kept: Option<PathBuf>,
    /// The options as last written, so that only a change writes the file.
    saved: ToolOptions,
}

impl Tooling {
    /// The options a settings file keeps, kept there again at each change.
    pub(in crate::session) fn load(file: PathBuf) -> Tooling {
        let options = Settings::load(&file).tools;
        Tooling { toolbox: Toolbox::with_options(options.clone()), kept: Some(file), saved: options }
    }
}

impl Session {
    /// The toolbar tool in hand.
    pub fn bar_tool(&self) -> BarTool {
        BarTool::of(self.tool)
    }

    /// The toolbar's state: the target, the options and the select mode.
    pub fn toolbox(&self) -> &Toolbox {
        &self.tooling.toolbox
    }

    /// Takes a toolbar tool in hand with its options as they are. Room works with a tile, so
    /// it takes the last tile back in hand where something else is. Fill takes back what was
    /// last in hand for its target where nothing is.
    pub fn take_bar_tool(&mut self, bar: BarTool) {
        if bar == BarTool::Room && self.chosen_tile().is_none() {
            self.recall(Target::Tiles);
        }
        if bar == BarTool::Fill && self.palette == Palette::None {
            self.recall(self.target());
        }
        let tool = self.tooling.toolbox.tool_for(bar);
        self.set_tool(tool);
    }

    /// What Paint and Erase work on.
    pub fn target(&self) -> Target {
        self.tooling.toolbox.target
    }

    /// Sets what Paint, Erase and Fill work on, and takes back in hand what was last in hand
    /// for it. With Paint in hand it may change the session tool, as Entities at a point
    /// places them.
    pub fn set_target(&mut self, target: Target) {
        let bar = self.bar_tool();
        self.recall(target);
        if bar == BarTool::Paint {
            self.take_bar_tool(BarTool::Paint);
        }
    }

    /// Makes `target` the target and puts back in the palette what was last in hand for it.
    fn recall(&mut self, target: Target) {
        self.tooling.toolbox.target = target;
        let palette = self.tooling.toolbox.recall(target);
        if palette != Palette::None && palette != self.palette {
            self.palette = palette;
            if let Palette::Entity(index) = self.palette {
                self.want_turned_previews(index);
            }
            self.reveal = true;
        } else if Target::of(&self.palette) != Some(target) {
            self.palette = Palette::None;
        }
        self.hint_answered();
    }

    /// The shape of the toolbar tool in hand, if it has one.
    pub fn shape(&self) -> Option<Shape> {
        self.tooling.toolbox.shape(self.bar_tool())
    }

    /// Sets the shape of the toolbar tool in hand, Paint's or Erase's, and keeps it.
    pub fn set_shape(&mut self, shape: Shape) {
        let bar = self.bar_tool();
        if self.tooling.toolbox.set_shape(bar, shape) {
            self.take_bar_tool(bar);
            self.save_tool_options();
        }
    }

    /// Switches one of the select tool's targets on or off and keeps it. The last one on stays
    /// on. With Select in hand a change of Tiles changes between a region and entities.
    pub fn toggle_select_target(&mut self, target: Target) {
        if !self.tooling.toolbox.options.select.toggle(target) {
            self.set_status("Select needs at least one target");
            return;
        }
        if self.bar_tool() == BarTool::Select {
            self.take_bar_tool(BarTool::Select);
        }
        self.save_tool_options();
    }

    /// What the select tool selects.
    pub fn select_targets(&self) -> SelectTargets {
        self.tooling.toolbox.options.select
    }

    /// How a press of the select tool changes what is selected, before Shift and Ctrl.
    pub fn select_mode(&self) -> SelectMode {
        self.tooling.toolbox.mode
    }

    pub fn set_select_mode(&mut self, mode: SelectMode) {
        self.tooling.toolbox.mode = mode;
    }

    /// The mode a press of the select tool goes by now: the mode chosen, or Add with Shift held,
    /// or Subtract with Ctrl.
    pub(super) fn press_mode(&self) -> SelectMode {
        self.tooling.toolbox.mode.with_keys(self.held.shift, self.held.ctrl)
    }

    /// Whether Paint at a point puts entities and decals where the cursor is, and not on their
    /// cell.
    pub fn place_free(&self) -> bool {
        self.tooling.toolbox.options.place_free
    }

    /// Sets whether Paint at a point puts entities and decals where the cursor is, and keeps it.
    pub fn set_place_free(&mut self, free: bool) {
        self.tooling.toolbox.options.place_free = free;
        self.save_tool_options();
    }

    /// Whether a press of Paint at a point puts the entity or the decal where the cursor is:
    /// the option, or the other way while Shift is held.
    pub(super) fn places_free(&self) -> bool {
        self.held.shift != self.place_free()
    }

    /// Whether Erase takes away only the kind in hand, and not everything of its target.
    pub fn erase_only(&self) -> bool {
        self.tooling.toolbox.options.erase_only
    }

    /// Sets whether Erase takes away only the kind in hand, and keeps it.
    pub fn set_erase_only(&mut self, only: bool) {
        self.tooling.toolbox.options.erase_only = only;
        self.save_tool_options();
    }

    /// What Erase with Only on would take away, by its id: the tile, the entity's prototype or
    /// the decal in hand, if it is of the target. None with nothing of the target in hand.
    pub fn erase_only_id(&self) -> Option<String> {
        match (self.target(), &self.palette) {
            (Target::Tiles, Palette::Tile(id)) | (Target::Decals, Palette::Decal(id)) => Some(id.clone()),
            (Target::Entities, Palette::Entity(index)) => self.prototypes.get(*index).map(|prototype| prototype.id.clone()),
            _ => None,
        }
    }

    /// A tool key that the tool in hand did not take: takes the tool it stands for.
    pub(super) fn tool_key(&mut self, action: KeyAction) {
        let options = &mut self.tooling.toolbox.options;
        let bar = match action {
            KeyAction::Take(bar) => bar,
            KeyAction::PaintShape(shape) => {
                options.paint_shape = shape;
                BarTool::Paint
            }
            KeyAction::PlaceEntities => {
                options.paint_shape = Shape::Point;
                self.recall(Target::Entities);
                BarTool::Paint
            }
            KeyAction::SelectRegion => {
                options.select.tiles = true;
                BarTool::Select
            }
            KeyAction::SelectThings => {
                options.select.tiles = false;
                BarTool::Select
            }
        };
        self.take_bar_tool(bar);
    }

    /// The tool in hand changed through `set_tool`: the toolbox notes it, and the options are
    /// kept where they changed.
    pub(super) fn tool_taken(&mut self, tool: Tool) {
        self.tooling.toolbox.note_tool(tool);
        self.save_tool_options();
    }

    /// A choice in a palette, or a pick: the toolbox notes it as last in hand for its target,
    /// which becomes the target.
    pub(in crate::session) fn chosen(&mut self) {
        let palette = self.palette.clone();
        self.tooling.toolbox.remember(&palette);
    }

    /// Writes the options into the settings file if they changed since they were last written.
    fn save_tool_options(&mut self) {
        let options = self.tooling.toolbox.options.clone();
        if options == self.tooling.saved {
            return;
        }
        self.tooling.saved = options.clone();
        let Some(file) = self.tooling.kept.clone() else { return };
        if let Err(error) = Settings::update(&file, |settings| settings.tools = options) {
            self.set_status(format!("Tool options not saved: {error}"));
        }
    }
}
