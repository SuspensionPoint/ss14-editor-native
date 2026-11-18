//! The tools: what the mouse does on the map, and what the interface draws over the map for them.
//!
//! **Presses.** `pointer_down` dispatches on the tool and calls `<tool>_down` (Paint and Erase
//! share `stroke_down`), which may begin a drag by setting `self.drag`. The middle button, the
//! Pan tool and a primary press with Space held pan before any tool is asked, with or without a
//! document. The secondary button has a match of its own on the tool, which calls
//! `<tool>_secondary_down`. Select, Area and Pick have one. The interface hands on one button at a time.
//!
//! **Drags.** Moves and releases go to the drag that was begun, which belongs to the tool in
//! hand or is a pan, since a change of tool ends it. `pointer_move` takes the drag out, gives it
//! to `<drag>_dragged` and keeps the `Drag` that comes back, so a drag goes on only when its
//! method returns one. `pointer_up` takes whatever drag is live out and gives it to `<drag>_up`,
//! which ends it. `set_tool`, the one way the tool changes, ends the drag too. So does
//! `pointer_left`, which drops it with nothing more asked. `outlines` lends the drag to
//! `<drag>_outlines`, and a drag with no outline of its own leaves the tool to draw its
//! `<tool>_hover_outlines`.
//!
//! **The wheel and the keys.** They are dispatched as the buttons are, and a taker returns true.
//! `wheel` asks the live drag first and then the tool in hand, each in a match that names every
//! one. It zooms when neither takes the wheel. A tool's key reaches the session as it goes
//! down (`key_down`) and as it comes up (`key_up`), once each however long it is held, and
//! `key_lost` says that a key is not down any more and was not seen to come up. Each asks the
//! tool in hand in the same way, and `key_down` does so before it looks the key up in
//! `Tool::ALL`. That is how R turns things with Select, Place or Area in hand, and is the
//! rectangle tool with any other. Those three take R going down as the beginning of a hold
//! (`select::Turning`). While the hold lasts the entity tools take the wheel, which turns by
//! five degrees a notch, and R coming up with the wheel not turned is a quarter turn
//! (`rotate`). The area tool leaves the wheel to zoom, and R coming up turns its region. The
//! hold ends when R comes up or is lost, and `set_tool` ends it as it ends a drag. The modifiers and
//! Space are in `held`, which the interface sets each frame before it hands on that frame's
//! presses. It is their only carrier, so a method that is not given an event can read it as
//! well. R is not in it: the key events are its only carrier.
//!
//! **The wheel's list.** With Select in hand, R not held and exactly one entity selected, the
//! wheel over the cell that entity is on, where another visible one is on it too, steps
//! through them, a notch a step, and does not zoom (`select::Stepping`, `step_wheel`). With
//! several selected the wheel zooms. What is there is the cell's stack, as the web editor
//! lists it: every visible entity whose picture's middle is in the cell, and any whose
//! picture is under the cursor, topmost first (`stack_at`). A click finds the topmost by the
//! pictures' own pixels. The list stays open while the cursor is in the cell it opened over.
//! `pointer_move` closes it when the cursor is in another, `pointer_left` when it leaves the
//! map, and `set_tool` with the tool. A press of the primary button closes it and goes on as
//! any press, on the entity the list is on before the topmost. That is the one place a press
//! does not take the topmost entity. A press of the secondary button that finds it open
//! closes it and does nothing more. The interface draws it from `stack`.
//!
//! **Pick.** What can be picked at a place, top first, is the visible entities there, then
//! the decals, and then the tile of the cell if it is not space (`pick::Pickable`). For the
//! wheel's list the entities are the cell's stack (`stack_at`). For a press they are found as
//! a click of the select tool finds them (`entities_at`). An entity of a prototype the engine
//! does not know is left out. A press picks the first: an entity through `choose_entity`, which takes Place in
//! hand, and a tile through `choose_tile`, which takes Paint. A prototype with no place in the
//! palette is picked all the same. The status line says "Picked X". With Pick in hand the
//! wheel over a place with two or more opens a list of them on the first and steps through
//! it as the select tool's does, and does not zoom (`pick::Picking`, `pick_wheel`). With fewer
//! it zooms. The list closes as the select tool's does, when the cursor leaves its cell, when
//! the pointer leaves the map and with the tool, and on a press. A primary press picks the
//! row the list is on. A secondary press picks nothing. The interface draws it from
//! `pick_list`, each row marked as an entity or a tile.
//!
//! **The palette.** What a tool puts down is `palette`, the one thing chosen in the palettes.
//! The choice is made with `choose_tile`, `choose_entity` or `choose_decal`, by the interface
//! and by a tool alike, and read through `chosen_tile`, `chosen_entity` (the index),
//! `chosen_prototype` and `chosen_decal`. Each may change the tool, which it does through
//! `set_tool`. A decal keeps a brush that takes an entity in hand and else takes Paint. The fields `tool` and `palette`
//! are private, read with `tool()` and `palette()`. A drag of a brush reads the palette once, at
//! the press, into a `tiles::Brush` (`brush_for_press`), a tile's id or an entity's index, and
//! carries it. A change of the palette during the drag changes nothing of it.
//!
//! **Entities from the brushes.** Paint, Erase, Rectangle, Line and Circle work with an entity
//! chosen as well as a tile. They put one down in the middle of each cell, unturned, as the place tool sends it.
//! A cell with no tile is skipped, as is a cell that holds an entity of that prototype or that
//! the same drag has put one on, since the mirror is behind the engine. The eraser deletes
//! the entities of that prototype on the cells it crosses and leaves the tiles: children of the
//! grid that are inside nothing. A shape of entities is refused past 10,000 cells. Fill knows
//! tiles alone.
//!
//! **Decals from the brushes.** The same brushes work with a decal chosen (`decals.rs`). They
//! put one on the lower left corner of each cell with a tile, with the placement settings of
//! the moment (`DecalSettings`). Decals stack, so a cell that holds one gets another, and only
//! the same drag skips a cell. A press of Paint with snap off in the settings, or with Shift,
//! puts one decal with its middle on the cursor and begins no stroke. The shapes always go by
//! cells. The eraser removes the decals of that prototype whose corner is in a cell it crosses.
//! The engine refuses a decal with no tile under its corner, and a refusal takes the whole
//! edit back, so no such op is sent. Paint, Rectangle, Line and Circle draw a ghost of the
//! decal where a press would put it (`decal_overlay`).
//!
//! **Decals in the selection.** The select tool selects decals beside entities, into
//! `selected_decals` by grid and id, while the decal layer is shown. A decal is found by the
//! cell-sized square from its lower left corner, top first by z-index and then id
//! (`decals_at`), and by a box where its middle is in it (`decals_in`). A press where no
//! entity is takes the topmost decal as a press takes an entity. A move, a turn, a delete and
//! a hold of R with the wheel take the selected decals along with the selected entities, in
//! the same edits, each decal stated whole with `decal.update`. A decal turns about its middle.
//! Selecting an entity alone drops the decals, and selecting a decal alone drops the entities.
//! Selected decals are outlined (`OutlineStyle::Decal`), since they are drawn in one piece
//! for each grid. Pick takes a decal after the entities and before the tile, with its colour,
//! angle, z-index and cleanable as the placement settings. The decal panel reads
//! `decal_summary` and sets a field of every selected decal with `edit_selected_decals`. The
//! changes of one gesture on one field are one step to undo, until the interface calls
//! `end_decal_edit`. Select All and Recolor All work on the grids of the selected decals.
//!
//! **Shapes.** Rectangle, Line and Circle drag out a shape from the cell pressed to the cell the
//! cursor is on, with the brush of the press. Let go, the shape is one edit with no stroke,
//! through `tiles::put_shape`, which names the shape when it refuses. The cells of a line and
//! a circle are the web editor's, step for step (`shapes::line_cells`, `shapes::disc_cells`).
//! A circle's radius is the distance from the cell pressed to the cursor's cell, rounded. Line
//! and Circle share one drag, `shapes::ShapeDrag`, which says which shape it is. While it is
//! dragged its cells are filled through `overlay_quads`, the cell pressed is outlined, and so
//! is a line's last cell.
//!
//! **Ghosts.** Before a press, the tool in hand shows half seen through what the press would
//! put down. The chosen entity's ghost is the engine's picture of it with Place, Paint,
//! Rectangle, Line and Circle (`entity_ghost_quads`), and `place_preview` gives the interface
//! its label, its arrow and a dashed square while the picture has yet to come, as the web
//! editor draws them. The chosen tile's ghost is its picture on the cell under the cursor with
//! the tools that put tiles down (`tile_ghost`). Neither shows during a shape's drag, while
//! Space is held, while a dialog is open (`dialog_open`) or off the map (`ghost_hover`). The
//! tile's ghost shows during no drag at all.
//!
//! **Hints.** Fill only knows a tile, the other brushes a tile or an entity, and the place tool
//! only an entity. A press that finds nothing its tool can use chosen sets `hint`, which is a
//! `Hint` and not a text. `status_line`
//! shows a hint that stands, and `status` stays as it was underneath. A choice of either kind
//! takes the hint away. So does a new message, and every message goes through `set_status`.
//!
//! **Facts about a tool.** Each is stated once, as a method of `Tool` whose match names every
//! tool: `puts_tiles` for where `choose_tile` goes, `keeps_entity_choice` for where
//! `choose_entity` goes, `brushes_entities` for whether a brush takes an entity chosen, and
//! `is_entity_tool` for the toolbar's groups.
//!
//! **Controls.** `CONTROLS` is the one list of the keys and gestures and what each does. The
//! interface's list of controls shows it, and the toolbar's tooltips take a tool's row from it
//! by the tool's key (`ToolEntry::control`). A change to what a key or a gesture does changes
//! its row as well.
//!
//! **State.** A drag carries its own in its struct. What outlives a drag is one struct in the
//! tool's file and one field on `Session`, as `place::Placing`, `tiles::Painting` and
//! `pick::Picking` are. Its
//! fields are private to that file, and others go through its methods. `set_tool` clears none
//! of it but the area tool's region and the paste mode, when the tool changes to another: choosing an entity takes Place in hand from most tools, and
//! the turn has to stay. What it ends beside the drag is a hold of R, which is to a key what a drag
//! is to a button, and the wheel's lists, which no other tool would step through or close.
//!
//! **The region.** The area tool's `area::Region` is cells of one grid. It is the one state
//! that goes when its tool does: `set_tool` drops it when the tool changes to another, and not
//! when the same tool is taken again. It is dropped too when a map is opened or made, when its
//! grid leaves the mirror, on undo and redo, by Escape, by a secondary press off it and when
//! a region is pasted or cut. The interface
//! draws it from `region_cells` (through `overlay_quads`), `region_edges` and `region_label`.
//! A drag of it asks nothing of the engine until the button comes up: the three say where it
//! would go and `overlay_quads` gives a ghost of it there. Let go, it is one edit with no
//! stroke, whose ops `area::move_ops` builds from the mirror in the order the engine check
//! proved, and the region is then the cells it went to. Delete and Backspace reach the session
//! as `delete_key`, whatever tool is in hand. With the area tool and a region they delete the
//! region with what stands on it (`area::delete_ops`), as one edit with no stroke, and the
//! region is dropped. With any other tool, or no region, they delete the selected entities.
//! A secondary press on a cell of the region opens its menu there (`region_menu`), whose
//! state and actions are the session's and which the interface only draws: Copy, Cut,
//! Delete, Paste while the clipboard holds something, and Save as Prefab. Any press closes
//! it, as do Escape and anything that drops the region. A secondary press off the region
//! drops it.
//! R turns the region a quarter (`turn_region`, `area::turn_ops`) about a pivot that is
//! chosen at the first turn and kept with the region. A move takes the pivot along, and a
//! change of the region's cells forgets it. The ops of a move, a delete and a turn are built
//! from the mirror, so a turn is not asked for while the engine has yet to answer the last
//! move or turn of the region, and a region whose move or turn the engine refused is dropped.
//!
//! **Nudging.** The arrow keys reach the session as `nudge`, as each goes down and as the
//! keyboard repeats it, when they reach the map at all (`arrows_reach_map`, Escape's rule).
//! With the select tool the selected entities and decals move one cell in their grid's own
//! directions, or with Shift one pixel when nothing selected is anchored. With the area tool
//! the region moves one cell through `move_region`, as a drag moves it. Each press is one
//! edit with no stroke. A press is dropped while the engine has yet to answer the last edit,
//! undo or redo (`last_edit`, `last_answered`), since the moves are built from the mirror. So
//! a held key never stacks up moves on a mirror that is behind (`nudge.rs`).
//!
//! **Copy, cut and paste.** Ctrl+C asks the engine for a copy of the selected entities
//! (`copy_selection`), which is a question and not an edit. With nothing selected nothing is
//! asked. The answer fills `clipboard` (`paste::Clipboard`): the game's own text, which the
//! core never reads, the counts, the origin cell, and for the ghost the rectangles the roots
//! were drawn with as the copy was asked for, measured from the origin cell in the grid's own
//! coordinates. Only the answer to the last copy asked for counts. The clipboard outlives the
//! map, and the system clipboard is not used. Ctrl+X (`cut_selection`) is a copy whose answer
//! deletes what was copied that is inside nothing else copied, as one edit. A refused copy
//! deletes nothing, and a map opened before the answer makes the cut a copy alone. Ctrl+V
//! (`begin_paste`) takes Select in hand and begins pasting (`paste::Pasting`, the quarters the
//! ghost is turned, counted against the clock as the engine counts them). While pasting, the
//! select tool's presses, its wheel's list and its hover label do not act and the wheel zooms.
//! A primary press stamps one edit with one paste op and no stroke, at the cursor's cell on the
//! grid with a tile under the cursor, or with Shift with the middle of the origin cell on the
//! cursor. Off every grid nothing is asked. R takes the mode's quarter turns through `rotate`,
//! clockwise, and Shift+R the other way. The ghost goes through `overlay_quads`, turned rigidly.
//! A refused paste leaves the mode on. A secondary press, Escape (`escape`) and a change of
//! tool end it.
//!
//! **A region on the clipboard.** `copy` and `cut` copy the area tool's region when that
//! tool is in hand with one, and else the selected entities (`copy_region`). The region's
//! cells, tiles and decals are read from the mirror (`area::region_copy`), measured from the
//! lowest cell of the box around it, and the engine copies what stands on it. With nothing
//! standing on it the engine is not asked. One clipboard holds either kind, and a copy of
//! one takes the place of the other. A cut drops the region at once and deletes it with what
//! stands on it when the clipboard is filled. Ctrl+V with a region on the clipboard takes the
//! area tool in hand, drops its region and pastes as the select tool does, with whole cells
//! only, on the grid the tile tools would work on under the cursor. A stamp is one edit
//! (`paste::stamp_ops`): the tiles, then one paste op for the entities, so that none lands
//! on a cell with no tile, then the decals as new ones. The ghost is the cells, the tiles'
//! pictures and the entities' rectangles (`region_paste_cells`).
//!
//! **Prefabs.** A prefab is a region of the clipboard in a file (`crate::prefab`). Save as
//! Prefab in the region's menu sets a flag the interface takes with `wants_prefab_name`, and
//! the name it asks for goes to `save_region_as_prefab`, which copies the region as Ctrl+C
//! does and writes the file when the clipboard is filled (`prefabs.rs`). `choose_prefab` puts
//! a prefab on the clipboard and calls `begin_paste`, so a prefab stamps as a copied region
//! does, through the same paste mode. Its ghost has no entity pictures.
//!
//! **Cables and pipes.** The cable and pipe tools share one drag, `infra::PathDrag`, begun by
//! either button: the primary lays and the secondary removes. It goes from cell to cell by
//! their sides and asks nothing until the button comes up, when the path is one edit with
//! no stroke. What they lay is chosen with `choose_cable` and `choose_pipe`, which take the
//! tool in hand, and is kept in `infra::Laying`. A pipe's fitting comes from the sides its
//! cell is open on (`pipes::fit`), and a pipe already there is turned in place where its
//! prototype stays the same, so its uid and data survive (`pipes.rs` has the rule). Which
//! prototypes a kind lays is a table of the web editor's names, and a kind the fork lacks
//! sets nothing.
//!
//! **Device links.** The link tool (`link.rs`) takes a source with a press and works on it
//! and the entity pressed next: a device goes on a list at once, and a pair to link opens a
//! chooser of ports that the interface draws from `link_chooser` and answers through the
//! session's methods. It begins no drag. Its outlines are drawn whether or not the cursor is
//! over the map, and its dashed line comes from `link_line`. What is linked is the mirror's
//! (`crate::links`, kept in `Doc::links`), which the deltas keep. The connections view
//! (`show_connections`) draws every link as a line through `overlay_quads`, which the renderer
//! puts under the selection border (`connections.rs`).
//!
//! **Names and visibility.** `world` is a position on the map and `point` is in pixels of the
//! map area. Both are `(f32, f32)`, so take care not to pass one for the other. An item is
//! private by default, `pub(super)` for what this file or another tool's file calls,
//! `pub(in crate::session)` for what `session/mod.rs` names, and `pub` for the interface.
//!
//! **A new tool** has a file of its own for its methods, the struct its drag carries and its
//! state struct, which is one field on `Session`, where it has any. Tools of a kind may share a
//! file and a drag, as Paint and Erase do, and Line and Circle. It adds itself to `Tool` and
//! `Tool::ALL`, its drag to `Drag`, and an arm to each match on them: `Tool::puts_tiles`,
//! `Tool::keeps_entity_choice`, `Tool::brushes_entities`, `Tool::is_entity_tool`,
//! `pointer_down` (one match for each button), `pointer_move`, `pointer_up`, `wheel` (one match
//! on the drag and one on the tool), `key_down`, `key_up`, `key_lost`, `outlines`,
//! `hover_outlines`, `drag_label`, `delete_key` and `rotate`. A drag that draws over the map
//! adds itself to `overlay_quads`. Its key gets a row in `CONTROLS`.

use super::*;

mod area;
mod connections;
mod decals;
mod infra;
mod link;
mod look;
mod nudge;
mod paste;
mod pick;
mod pipes;
mod place;
mod prefabs;
mod select;
mod shapes;
#[cfg(test)]
pub(super) mod tests;
mod tiles;

pub(super) use area::Region;
pub use area::{MenuAction, MenuEntry};
pub(super) use decals::Decaling;
pub use decals::{decal_colour_bytes, decal_colour_text, DecalField, DecalGroup, DecalSettings, DecalSummary};
pub use infra::CableKind;
pub(super) use infra::Laying;
pub use link::{EntityLinkRows, LinkChooser, LinkKind, LinkLine};
pub(super) use link::Linking;
pub use look::{HoverLook, PointerLook};
pub use nudge::{arrows_reach_map, Arrow};
pub use pipes::{PipeNetwork, PipePieces, PIPE_COLOURS};
pub use paste::{escape_to, Clipboard, CopiedTile, EscapeTo};
pub(super) use paste::{Copying, Pasting};
pub use pick::{PickList, Pickable};
pub(super) use pick::Picking;
pub use place::PlacePreview;
pub(super) use place::Placing;
pub(super) use prefabs::Prefabs;
pub use select::StackList;
pub(super) use select::{Stepping, Turning};
pub(super) use tiles::Painting;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pan,
    Paint,
    Erase,
    Pick,
    Fill,
    Rectangle,
    Line,
    Circle,
    Area,
    Select,
    Place,
    Link,
    Cable,
    Pipe,
}

/// A tool as the toolbar lists it.
#[derive(Clone, Copy, Debug)]
pub struct ToolEntry {
    pub tool: Tool,
    /// Its name on its button.
    pub name: &'static str,
    /// The key that takes it in hand.
    pub key: char,
}

/// Every key and gesture of the editor and what it does, as the list of controls shows them.
/// A tool's row is under its key alone, which is how `ToolEntry::control` finds it.
pub const CONTROLS: &[(&str, &str)] = &[
    ("H", "Pan: drag the map"),
    ("B", "Paint: paint the chosen tile in every cell the brush crosses. With an entity or decal chosen, it places one in each cell that has a tile. A decal with snap off, or with Shift held, goes exactly where the cursor is"),
    ("E", "Erase: remove tiles in every cell the brush crosses. With an entity or decal chosen, it deletes those of that kind there and leaves the tiles"),
    ("I", "Pick: takes the top entity under the cursor and switches to E-Place, or else the decal with its settings and switches to Paint, or else the bare tile and switches to Paint. Over a stack, the wheel steps through a list of what's there and a click takes the current row"),
    ("G", "Fill: fill the area of the same tile under the cursor, up to 50,000 tiles"),
    ("R", "Rect: drag out a rectangle of the chosen tile, or of the chosen entity or decal one per cell, up to 10,000 cells. With E-Select or E-Place in hand, turns a quarter clockwise (the ghost while pasting). With Select in hand, turns the region a quarter (or the ghost while pasting). With a decal chosen and Paint, Rect, Line or Circle in hand, turns the decal's placement angle a quarter clockwise"),
    ("L", "Line: drag out a line of the chosen tile, or of the chosen entity or decal one per cell, up to 10,000 cells"),
    ("C", "Circle: drag out a filled circle from the cell you press to the cursor's cell, of the chosen tile, or of the chosen entity or decal one per cell, up to 10,000 cells"),
    ("S", "Select: drag out a region of tiles, one box at a time. Drag the region to move it along with everything on it"),
    ("V", "E-Select: select, move, turn and delete entities, and decals where there's no entity"),
    ("P", "E-Place: place the chosen entity, one per cell as you drag"),
    ("D", "Link: click a device that has links or a device list, then another device. For a list, the second device goes on the list. For links, a window shows the ports of both, the game's defaults first, along with the links the two already have. Shift+click uses a device as a link source even when it also has a list. Right-click a listed device to take it off. Escape stops"),
    ("K", "Cables: drag a path of the chosen cable, one per cell, skipping cells that already have it. Right-drag removes that cable"),
    ("J", "Pipes: drag a path of the chosen pipe, each piece fitted to the cells it joins. Start or end on a pipe to join it. Right-drag removes pipes and refits the ones next to them"),
    ("Shift+R", "With E-Select, E-Place or Select in hand, or a decal chosen, turns a quarter the other way"),
    ("R held, wheel", "With E-Select or E-Place in hand, turns five degrees per notch. With a decal chosen and a brush in hand, turns its placement angle the same way"),
    ("Shift+click", "E-Place: place exactly at the cursor. E-Select: add an entity, or take a selected one out. Pasting: stamp with the middle of the ghost's first cell exactly on the cursor"),
    ("Shift+drag", "E-Select: move unanchored things exactly with the cursor, or drag a box that adds to the selection. Select: drag a box that adds to the region"),
    ("Ctrl+click, Ctrl+drag", "E-Select: remove an entity, or a box of them, from the selection. Select: remove a box from the region"),
    ("Right-click", "E-Select: clear the selection, or close the wheel's list. Pasting: stop pasting. Select: on the region, open its menu, and off it, clear the region. Pick: close the list without picking. Cables and Pipes: remove along a drag"),
    ("Wheel", "Zoom. With E-Select and one entity selected, over it where others overlap, step through them. With Pick, where two or more things could be picked, step through them"),
    ("Middle drag, Space+drag", "Pan with any tool"),
    ("Arrow keys", "E-Select: move the selected entities and decals one cell along the grid's own axes, one undo step per press. Select: move the region one cell with its tiles, decals, air and everything on it. Hold to repeat"),
    ("Shift+Arrow keys", "E-Select: move one pixel (1/32 of a cell) when nothing selected is anchored. If anything is anchored, one cell"),
    ("Delete, Backspace", "Delete the selection. With Select in hand and a region, delete the region and everything on it"),
    ("Escape", "Close the region's menu, or else stop linking, or else stop pasting, or else clear the selection and the region, or close a dialog. In a side panel text field, only the field gets it"),
    ("Ctrl+C", "Copy the selected entities and what they hold. With Select in hand and a region, copy the region with its tiles, decals and everything on it"),
    ("Ctrl+X", "Cut: copy the selected entities, or the region, then delete them in one step"),
    ("Ctrl+V", "Paste: with E-Select in hand for entities, or Select for a region, a ghost of the copy follows the cursor by whole cells. Each click stamps it as one step, and you can keep pasting"),
    ("1 to 9", "Choose one of the first nine favourites, numbered on the rows of the Favourites tab (the star). The star on a palette row, or next to what's in hand on the status bar, adds or removes a favourite"),
    ("F", "Fit the map to the window"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Y, Ctrl+Shift+Z", "Redo"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S", "Save as"),
    ("Ctrl+O", "Open a map"),
    ("Ctrl+N", "New map"),
    ("Ctrl+F", "Find an entity by its id, name or uid. Enter or a click selects it and zooms to it. Up and Down move through the list"),
    ("?", "This list of controls"),
];

impl ToolEntry {
    /// What the tool does, from its row of [`CONTROLS`].
    pub fn control(self) -> Option<&'static str> {
        let key = self.key.to_string();
        CONTROLS.iter().find(|(keys, _)| *keys == key).map(|(_, does)| *does)
    }
}

impl Tool {
    /// The tools as the web editor's toolbar has them, in its order.
    pub const ALL: [ToolEntry; 14] = [
        ToolEntry { tool: Tool::Pan, name: "Pan", key: 'H' },
        ToolEntry { tool: Tool::Paint, name: "Paint", key: 'B' },
        ToolEntry { tool: Tool::Erase, name: "Erase", key: 'E' },
        ToolEntry { tool: Tool::Pick, name: "Pick", key: 'I' },
        ToolEntry { tool: Tool::Fill, name: "Fill", key: 'G' },
        ToolEntry { tool: Tool::Rectangle, name: "Rect", key: 'R' },
        ToolEntry { tool: Tool::Line, name: "Line", key: 'L' },
        ToolEntry { tool: Tool::Circle, name: "Circle", key: 'C' },
        ToolEntry { tool: Tool::Area, name: "Select", key: 'S' },
        ToolEntry { tool: Tool::Select, name: "E-Select", key: 'V' },
        ToolEntry { tool: Tool::Place, name: "E-Place", key: 'P' },
        ToolEntry { tool: Tool::Link, name: "Link", key: 'D' },
        ToolEntry { tool: Tool::Cable, name: "Cables", key: 'K' },
        ToolEntry { tool: Tool::Pipe, name: "Pipes", key: 'J' },
    ];

    /// Whether the tool works on entities. The toolbar lists those apart from the rest.
    pub fn is_entity_tool(self) -> bool {
        match self {
            Tool::Select | Tool::Place | Tool::Link => true,
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Area | Tool::Cable | Tool::Pipe => false,
        }
    }

    /// Whether the tool lays cables or pipes. The toolbar lists those last, apart from the rest.
    pub fn is_infrastructure_tool(self) -> bool {
        match self {
            Tool::Cable | Tool::Pipe => true,
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Area | Tool::Select | Tool::Place | Tool::Link => false,
        }
    }

    /// Whether the tool's brush works with an entity or a decal chosen: it puts it down, or the
    /// eraser deletes those of its prototype. Fill does not: it is a tile tool alone.
    fn brushes_entities(self) -> bool {
        match self {
            Tool::Paint | Tool::Erase | Tool::Rectangle | Tool::Line | Tool::Circle => true,
            Tool::Pan | Tool::Pick | Tool::Fill | Tool::Area | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => false,
        }
    }

    /// Whether the tool stays in hand when an entity is chosen: a brush that works with one,
    /// and the select tool. Any other is put down for the place tool.
    fn keeps_entity_choice(self) -> bool {
        match self {
            Tool::Paint | Tool::Erase | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Select => true,
            Tool::Pan | Tool::Pick | Tool::Fill | Tool::Area | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => false,
        }
    }

    /// Whether the tool puts the chosen tile down. The eraser does not: it takes tiles away.
    fn puts_tiles(self) -> bool {
        match self {
            Tool::Paint | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle => true,
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Area | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => false,
        }
    }
}

/// What a press asks the mapper to choose, having found nothing of its kind in the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hint {
    Tile,
    Entity,
}

impl Hint {
    /// What the status line says while the hint stands.
    pub(super) fn text(self) -> &'static str {
        match self {
            Hint::Tile => "Choose a tile to paint",
            Hint::Entity => "Choose an entity to place",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Middle,
    Secondary,
}

/// The modifiers and keys that change what the mouse does while they are down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub shift: bool,
    pub ctrl: bool,
    pub space: bool,
}

impl Held {
    /// What is held, in words, for the performance display.
    pub fn words(&self) -> String {
        let names: Vec<&str> = [(self.shift, "Shift"), (self.ctrl, "Ctrl"), (self.space, "Space")].into_iter().filter_map(|(down, name)| down.then_some(name)).collect();
        if names.is_empty() { "nothing".to_owned() } else { names.join(" ") }
    }
}

/// A four-cornered outline for the interface to draw over the map, in map coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Outline {
    pub corners: [(f32, f32); 4],
    pub style: OutlineStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineStyle {
    /// The cell under the cursor.
    Hover,
    /// A rectangle being dragged out.
    Drag,
    /// A box being dragged out that adds to the selection.
    DragAdd,
    /// A box being dragged out that takes away from the selection.
    DragTakeAway,
    /// An end of a line being dragged out.
    Line,
    /// The centre of a circle being dragged out.
    Circle,
    /// A selected decal.
    Decal,
    /// The cell a jump to a finding of Validate Map went to.
    Found,
    /// An entity the link tool could take as a source.
    Linkable,
    /// The link tool's source, and the other end of the pair in its chooser.
    LinkSource,
    /// What the link tool's source is linked to, or has on its list.
    Linked,
}

/// What a press of the mouse has begun.
pub(super) enum Drag {
    None,
    Pan { last: (f32, f32) },
    Stroke(tiles::StrokeDrag),
    Rectangle(tiles::RectangleDrag),
    Shape(shapes::ShapeDrag),
    Move(select::MoveDrag),
    SelectBox(select::BoxDrag),
    Place(place::PlaceDrag),
    AreaBox(area::BoxDrag),
    AreaMove(area::MoveDrag),
    Path(infra::PathDrag),
}

/// The corners of a box given as (left, bottom, right, top).
fn boxed((l, b, r, t): (f32, f32, f32, f32)) -> [(f32, f32); 4] {
    [(l, b), (r, b), (r, t), (l, t)]
}

impl Session {
    // ---- the mouse ----

    /// Takes a tool in hand. It is the one way the tool changes, and it ends a drag that is
    /// live and a hold of R, and closes the lists the wheel steps through. A change to another
    /// tool drops the area tool's region and ends pasting.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.region = None;
            self.pasting.end();
            self.linking.end();
        }
        self.tool = tool;
        self.drag = Drag::None;
        self.turning.end();
        self.decal_turn_key_up('R');
        self.stepping.close();
        self.picking.close();
    }

    /// A key that may be a tool's went down, with Shift or without. The interface hands it on
    /// once, and not as the keyboard repeats it. The tool in hand is asked first and says
    /// whether it took the key. An entity tool takes R, which begins a hold that turns things
    /// there. A brush placing a decal takes R, which turns the decal's placement angle. A key
    /// no tool took is a tool by its key, as the web editor has them, where R is the rectangle
    /// tool.
    pub fn key_down(&mut self, key: char, shift: bool) -> bool {
        let key = key.to_ascii_uppercase();
        let taken = match self.tool {
            Tool::Select | Tool::Place | Tool::Area => self.turn_key_down(key, shift),
            Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle => self.decal_turn_key(key, shift),
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Link | Tool::Cable | Tool::Pipe => false,
        };
        if taken {
            return true;
        }
        match Tool::ALL.iter().find(|entry| entry.key == key) {
            Some(entry) => {
                self.set_tool(entry.tool);
                true
            }
            None => false,
        }
    }

    /// A key that went down came up. Only the tool in hand is asked.
    pub fn key_up(&mut self, key: char) {
        let key = key.to_ascii_uppercase();
        match self.tool {
            Tool::Select | Tool::Place | Tool::Area => self.turn_key_up(key),
            Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle => self.decal_turn_key_up(key),
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Link | Tool::Cable | Tool::Pipe => {}
        }
    }

    /// A key is not down any more and was not seen to come up: the keyboard went to a text
    /// field, a dialog or another window. What the key began ends there and asks for nothing
    /// more. The interface says so in every frame the key is not down, so that no hold can
    /// outlast its key.
    pub fn key_lost(&mut self, key: char) {
        let key = key.to_ascii_uppercase();
        match self.tool {
            Tool::Select | Tool::Place | Tool::Area => self.turn_key_lost(key),
            Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle => self.decal_turn_key_up(key),
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Link | Tool::Cable | Tool::Pipe => {}
        }
    }

    /// Whether the wheel turns things now, and does not zoom: an entity tool is in hand and R
    /// is held, or a brush places a decal and R is held since it turned the placement angle.
    pub fn wheel_turns(&self) -> bool {
        (self.turning.is_live() && self.tool.is_entity_tool()) || self.decal_wheel_turns()
    }

    /// Takes away a hint that asked for a choice in the palettes, now that one has been made.
    fn hint_answered(&mut self) {
        self.hint = None;
    }

    /// A mouse button went down over the map, at a point of the map area in device pixels.
    pub fn pointer_down(&mut self, point: (f32, f32), view: (u32, u32), button: Button) {
        let world = self.world_at(point, view);
        self.hover = Some(world);
        // Any press closes the region's menu. A secondary one on the region opens it again there.
        self.region_menu = None;
        match button {
            Button::Middle => self.drag = Drag::Pan { last: point },
            Button::Primary if self.held.space => self.drag = Drag::Pan { last: point },
            Button::Primary => match self.tool {
                Tool::Pan => self.drag = Drag::Pan { last: point },
                // Every other tool works on the document.
                _ if self.doc.is_none() => {}
                Tool::Paint | Tool::Erase => self.stroke_down(world),
                Tool::Pick => self.pick_down(world),
                Tool::Fill => self.fill_down(world),
                Tool::Rectangle => self.rectangle_down(world),
                Tool::Line => self.shape_down(world, shapes::Shape::Line),
                Tool::Circle => self.shape_down(world, shapes::Shape::Circle),
                Tool::Area if self.pasting.is_live() => self.region_paste_down(world),
                Tool::Area => self.area_down(world),
                Tool::Select if self.pasting.is_live() => self.paste_down(world),
                Tool::Select => self.select_down(world, point),
                Tool::Place => self.place_down(world),
                Tool::Link => self.link_down(world),
                Tool::Cable | Tool::Pipe => self.path_down(world, false),
            },
            Button::Secondary => match self.tool {
                // The tools that take it work on the document.
                _ if self.doc.is_none() => {}
                Tool::Area | Tool::Select if self.pasting.is_live() => {
                    self.pasting.end();
                }
                Tool::Area => self.area_secondary_down(world),
                Tool::Select => self.select_secondary_down(),
                Tool::Pick => self.pick_secondary_down(),
                Tool::Link => self.link_secondary_down(world),
                Tool::Cable | Tool::Pipe => self.path_down(world, true),
                Tool::Pan | Tool::Paint | Tool::Erase | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Place => {}
            },
        }
    }

    /// The mouse moved over the map.
    pub fn pointer_move(&mut self, point: (f32, f32), view: (u32, u32)) {
        let world = self.world_at(point, view);
        self.hover = Some(world);
        self.step_moved(world);
        self.pick_moved(world);
        self.drag = match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::None => Drag::None,
            Drag::Pan { last } => {
                self.pan((point.0 - last.0, point.1 - last.1));
                Drag::Pan { last: point }
            }
            Drag::Stroke(drag) => self.stroke_dragged(world, drag),
            Drag::Rectangle(drag) => self.rectangle_dragged(world, drag),
            Drag::Shape(drag) => self.shape_dragged(world, drag),
            Drag::Move(drag) => self.move_dragged(world, point, drag),
            Drag::SelectBox(drag) => self.select_box_dragged(world, drag),
            Drag::Place(drag) => self.place_dragged(world, drag),
            Drag::AreaBox(drag) => self.area_box_dragged(world, drag),
            Drag::AreaMove(drag) => self.area_move_dragged(world, drag),
            Drag::Path(drag) => self.path_dragged(world, drag),
        };
    }

    /// The mouse button came up.
    pub fn pointer_up(&mut self, point: (f32, f32), view: (u32, u32)) {
        let world = self.world_at(point, view);
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Rectangle(drag) => self.rectangle_up(drag),
            Drag::Shape(drag) => self.shape_up(drag),
            Drag::Move(drag) => self.move_up(drag),
            Drag::SelectBox(drag) => self.select_box_up(world, drag),
            Drag::AreaBox(drag) => self.area_box_up(world, drag),
            Drag::AreaMove(drag) => self.area_move_up(world, drag),
            Drag::Path(drag) => self.path_up(drag),
            Drag::None | Drag::Pan { .. } | Drag::Stroke(_) | Drag::Place(_) => {}
        }
    }

    /// The cursor left the map, or was lost with a button down. A drag that is live ends there
    /// and asks for nothing more, and what it has already asked for stays. The lists the
    /// wheel steps through close.
    pub fn pointer_left(&mut self) {
        self.hover = None;
        self.drag = Drag::None;
        self.stepping.close();
        self.picking.close();
    }

    /// The wheel turned over the map by `notches` of a real wheel, positive when turned away
    /// from the user. A notch may come in parts, which add up to it. The drag that is live is
    /// asked first and then the tool in hand, and each says whether it took the wheel. An
    /// entity tool takes it while R is held (`wheel_turns`). Otherwise the select tool takes
    /// it over the one selected entity where another is under or over it (`step_wheel`), and
    /// the pick tool where two or more things could be picked (`pick_wheel`). A brush placing a
    /// decal takes it while R is held, and turns the placement angle (`decal_wheel`). Those take the
    /// notches as they come. A wheel nothing took zooms, in when turned away, by
    /// `ZOOM_STEPS_PER_NOTCH` of a step of 1.15 times for each notch. The zoom is not given at
    /// once but over the next frames (`zoom.rs`, [`Self::advance_zoom`]), about the point.
    pub fn wheel(&mut self, notches: f32, point: (f32, f32), view: (u32, u32)) {
        let mut taken = match &self.drag {
            Drag::None | Drag::Pan { .. } | Drag::Stroke(_) | Drag::Rectangle(_) | Drag::Shape(_) | Drag::Move(_) | Drag::SelectBox(_) | Drag::Place(_) => false,
            Drag::AreaBox(_) | Drag::AreaMove(_) | Drag::Path(_) => false,
        };
        if !taken {
            taken = match self.tool {
                // Pasting, the wheel zooms, with R held or not.
                Tool::Select if self.pasting.is_live() => false,
                Tool::Select => self.turn_wheel(notches) || self.step_wheel(notches, self.world_at(point, view)),
                Tool::Place => self.turn_wheel(notches),
                Tool::Pick => self.pick_wheel(notches, self.world_at(point, view)),
                Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle => self.decal_wheel(notches),
                Tool::Pan | Tool::Erase | Tool::Fill | Tool::Area | Tool::Link | Tool::Cable | Tool::Pipe => false,
            };
        }
        if !taken {
            self.owe_zoom(notches, point, view);
        }
    }

    // ---- what the interface draws over the map ----

    /// The editor's own rectangles for the renderer: tiles asked for and not yet answered and
    /// the ghost of the chosen tile (to go under everything but the tiles), and the ghost of
    /// the chosen entity where a press would put it (on top). That is the engine's look of it
    /// from the nearest of four sides, turned the rest of the way to the turn it will be put
    /// down with. The area tool's own rectangles go on top too:
    /// the fill of its region, or the ghost of a region being moved. So do the ghost of a paste
    /// and the ghost of the chosen decal. The lines of the connections view go on top first,
    /// under all of those.
    pub fn overlay_quads(&mut self) -> (Vec<f32>, Vec<f32>) {
        let mut under = self.painting.quads(&mut self.lists);
        self.tile_ghost(&mut under);

        let mut over = Vec::new();
        self.connection_overlay(&mut over);
        if let Some(ghost) = self.entity_ghost_quads() {
            over.extend(ghost);
        }
        self.decal_overlay(&mut over);
        self.shape_overlay(&mut over);
        self.area_overlay(&mut over);
        self.paste_overlay(&mut over);
        self.path_overlay(&mut over);
        (under, over)
    }

    /// Where the cursor is on the map, for a ghost of what a press would put down. None while
    /// Space is held or the map is panned, while a dialog is open, and with the cursor off the map.
    fn ghost_hover(&self) -> Option<(f32, f32)> {
        if self.held.space || self.dialog_open || matches!(self.drag, Drag::Pan { .. }) {
            return None;
        }
        self.hover
    }

    /// Outlines for the interface to draw over the map: the selected decals, the cell a jump to
    /// a finding went to, the link tool's, the cell under the cursor, a rectangle being dragged. (The border of
    /// the selected entities is the renderer's: it follows the sprites. The decals are drawn in
    /// one piece for each grid, so theirs is an outline. The air overlay's leaks are the renderer's too.)
    pub fn outlines(&self) -> Vec<Outline> {
        let mut outlines: Vec<Outline> = Vec::new();
        self.decal_selection_outlines(&mut outlines);
        self.found_outlines(&mut outlines);
        if self.tool == Tool::Link {
            self.link_outlines(&mut outlines);
        }

        match &self.drag {
            Drag::Rectangle(drag) => self.rectangle_outlines(drag, &mut outlines),
            Drag::Shape(drag) => self.shape_outlines(drag, &mut outlines),
            Drag::SelectBox(drag) => self.select_box_outlines(drag, &mut outlines),
            Drag::AreaBox(drag) => self.area_box_outlines(drag, &mut outlines),
            // The region is drawn where it would go, and no cell under the cursor.
            Drag::AreaMove(_) => {}
            Drag::None | Drag::Pan { .. } | Drag::Stroke(_) | Drag::Move(_) | Drag::Place(_) | Drag::Path(_) => self.hover_outlines(&mut outlines),
        }
        outlines
    }

    /// The tool's outline under the cursor, for when no drag draws an outline of its own.
    fn hover_outlines(&self, outlines: &mut Vec<Outline>) {
        let (Some(world), Some(doc)) = (self.hover, self.doc.as_ref()) else { return };
        match self.tool {
            Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Area | Tool::Cable | Tool::Pipe => self.tile_hover_outlines(world, doc, outlines),
            Tool::Place => self.place_hover_outlines(world, doc, outlines),
            // The link tool's outlines do not follow the cursor (`outlines`).
            Tool::Pan | Tool::Select | Tool::Link => {}
        }
    }

    /// What is under the cursor, with any tool, for the interface to write above its cell
    /// with a faint outline of the cell: the corners of the cell on the map, and the name of
    /// the topmost visible entity under the cursor with a count of the others in the cell's
    /// stack (`stack_at`). Where none is under the cursor it names the top of the stack. The
    /// name is the entity's own, or its prototype's, or else the prototype's id. Where no
    /// entity is, the topmost visible decal's prototype with a count of the other decals, as
    /// the web editor writes it. None where nothing visible is, while a drag is live, and
    /// while the select tool is pasting.
    pub fn hover_label(&mut self) -> Option<([(f32, f32); 4], String)> {
        if !matches!(self.drag, Drag::None) || self.pasting.is_live() {
            return None;
        }
        let world = self.hover?;
        let (under, found) = self.stack_at(world);
        let decals = if found.is_empty() { self.decals_at(world) } else { Vec::new() };
        let doc = self.doc.as_ref()?;
        let (mut text, others) = match (under.first().or(found.first()), decals.first()) {
            (Some(uid), _) => {
                let top = doc.entities.get(uid)?;
                let prototype = self.prototype(&top.prototype).map(|prototype| prototype.name.as_str()).filter(|name| !name.is_empty());
                (top.name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| prototype.unwrap_or(&top.prototype).to_owned()), found.len() - 1)
            }
            (None, Some(&key)) => (self.decal(key)?.prototype.clone(), decals.len() - 1),
            (None, None) => return None,
        };
        if others > 0 {
            text += &format!(" (+{others} more)");
        }
        let corners = match self.cell_of(world) {
            (0, cell) => boxed((cell.0 as f32, cell.1 as f32, cell.0 as f32 + 1.0, cell.1 as f32 + 1.0)),
            (grid, cell) => doc.grid(grid).map(|grid| grid.cells_outline(cell, cell))?,
        };
        Some((corners, text))
    }

    /// A label for the drag that is live, for the interface to write above it: where on the
    /// map, the middle of its top edge, and the text. A rectangle and a box of the area tool
    /// have one, the size in cells as "WxH". A line has its number of cells and a circle
    /// "r=N", over the cursor's cell. A cable or pipe path has its number of cells with a tile,
    /// over the cursor's cell.
    pub fn drag_label(&self) -> Option<((f32, f32), String)> {
        match &self.drag {
            Drag::Rectangle(drag) => self.rectangle_label(drag),
            Drag::Shape(drag) => self.shape_label(drag),
            Drag::AreaBox(drag) => self.area_box_label(drag),
            Drag::Path(drag) => self.path_label(drag),
            Drag::None | Drag::Pan { .. } | Drag::Stroke(_) | Drag::Move(_) | Drag::SelectBox(_) | Drag::Place(_) | Drag::AreaMove(_) => None,
        }
    }

    /// What is under the cursor, in words, for a status bar.
    pub fn hover_text(&self) -> Option<String> {
        let (world, doc) = (self.hover?, self.doc.as_ref()?);
        let grid = doc.grid_at(world)?;
        let cell = grid.cell_at(world);
        Some(format!("{}, {}  {}", cell.0, cell.1, doc.tile_at(grid.uid, cell)))
    }
}
