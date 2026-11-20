//! The select tool: finding entities on the map, and moving, turning and deleting the selected ones.

use super::decals::decal_update_op;
use super::*;
use crate::model::{Decal, Grid};

/// Moving the selection: where the press was, and where each entity and decal started.
pub(in crate::session) struct MoveDrag {
    stroke: u64,
    from: (f32, f32),
    pressed: (f32, f32),
    originals: Vec<Entity>,
    /// The selected decals with their grids' uids.
    decals: Vec<(i32, Decal)>,
    moved: bool,
    /// The entity or the decal a press with Shift found selected. Letting go takes it out of
    /// the selection if nothing has moved by then.
    takes_out: Option<i32>,
    takes_out_decal: Option<(i32, i32)>,
}

/// A selection box being dragged out, and what it was begun for.
pub(in crate::session) struct BoxDrag {
    from: (f32, f32),
    to: (f32, f32),
    mode: SelectMode,
}

/// What one notch of the wheel turns things by while R is held, in radians: five degrees.
const NOTCH_TURN: f64 = std::f64::consts::PI / 36.0;

/// An angle a quarter turn on, with the clock or against it. The angle is taken as so many
/// whole quarter turns and a rest. One quarter is taken or added, and the count stays within
/// the four that begin just after minus a half turn, or the same four any number of whole
/// turns away, whichever the angle is already in. So four turns come back to the very number,
/// and a map's angle is written in the turn the file had it in. It is `ByQuarters` of the
/// engine check (`RegionCheck.cs`), which found that this gives the file back. An angle
/// half way between two quarters counts as the upper one, each time, or it would not come back.
pub(super) fn quarter_on(angle: f64, clockwise: bool) -> f64 {
    let quarter = std::f64::consts::FRAC_PI_2;
    let quarters = (angle / quarter + 0.5).floor();
    let rest = angle - quarters * quarter;
    let first = 4.0 * ((quarters + 1.0) / 4.0).floor() - 1.0;
    let mut next = if clockwise { quarters - 1.0 } else { quarters + 1.0 };
    if next > first + 3.0 {
        next -= 4.0;
    } else if next < first {
        next += 4.0;
    }
    next * quarter + rest
}

/// The box around a picture's rectangles on the map: (left, bottom, right, top).
fn quads_bounds(quads: &[f32]) -> Option<(f32, f32, f32, f32)> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
        for corner in quad[..8].chunks_exact(2) {
            bounds = Some(match bounds {
                None => (corner[0], corner[1], corner[0], corner[1]),
                Some((l, b, r, t)) => (l.min(corner[0]), b.min(corner[1]), r.max(corner[0]), t.max(corner[1])),
            });
        }
    }
    bounds
}

/// What the tools that turn things keep while R is held. R let go is a quarter turn, unless
/// the wheel was turned in the meantime, which for an entity tool turns by five degrees a notch.
#[derive(Default)]
pub(in crate::session) struct Turning {
    /// The hold that is live, from R going down with a tool that turns things in hand.
    hold: Option<Hold>,
}

/// One hold of R.
struct Hold {
    /// Whether Shift was down as R went down, which turns the quarter the other way.
    shift: bool,
    /// Whether the wheel was turned at all. R let go is then no quarter turn.
    wheeled: bool,
    /// How far the wheel has turned, in notches and the parts they came in, and how many
    /// whole notches of that have been turned by.
    notches: f32,
    turned: i32,
    /// The selection the wheel is turning, once it has turned it.
    selection: Option<Turned>,
}

/// A selection being turned by the wheel: which it is, its one step to undo, and the angle
/// each entity of it stood at when its turn began, and each decal as it was.
struct Turned {
    selection: Vec<i32>,
    selected_decals: Vec<(i32, i32)>,
    stroke: u64,
    originals: Vec<(i32, f64)>,
    decals: Vec<(i32, Decal)>,
}

impl Turning {
    /// Whether R is held.
    pub(super) fn is_live(&self) -> bool {
        self.hold.is_some()
    }

    /// Ends the hold that is live with nothing more asked.
    pub(super) fn end(&mut self) {
        self.hold = None;
    }
}

/// What the select tool keeps while the wheel steps through the things under the cursor.
#[derive(Default)]
pub(in crate::session) struct Stepping {
    /// The list that is open, from the first turn of the wheel over it.
    stack: Option<Stack>,
}

/// A cell: the grid it is of, or 0 for a cell of the map itself, and where it is on that.
pub(super) type Place = (i32, (i32, i32));

/// One list the wheel steps through.
struct Stack {
    /// The cell the cursor was over as the list opened. The list closes when it leaves it.
    place: Place,
    /// The visible entities on the cell then (`stack_at`), topmost first, and the one the
    /// wheel is on.
    entries: Vec<i32>,
    current: usize,
    /// How far the wheel has turned since the list opened, in notches and the parts they came
    /// in, and how many whole notches of that have been stepped.
    notches: f32,
    stepped: i32,
}

impl Stepping {
    /// Closes the list, and says whether one was open.
    pub(super) fn close(&mut self) -> bool {
        self.stack.take().is_some()
    }
}

/// The list the wheel steps through, as the interface draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct StackList {
    /// The prototype of each thing in it, topmost first.
    pub entries: Vec<String>,
    /// Which of them the wheel is on. It is the one selected.
    pub current: usize,
    /// The corners of the cell the list is of, on the map.
    pub cell: [(f32, f32); 4],
}

impl Session {
    /// The topmost visible entity whose picture is under a map position.
    pub fn entity_at(&mut self, world: (f32, f32)) -> Option<i32> {
        self.entities_at(world).first().copied()
    }

    /// Every visible entity whose picture is under a map position, topmost first: the
    /// engine's draw order, read from its end. What is on a hidden layer is left out.
    pub(super) fn entities_at(&mut self, world: (f32, f32)) -> Vec<i32> {
        self.found_at(world, false).0
    }

    /// The stack on the cell a map position is in, as the web editor lists it: every visible
    /// entity on that cell, topmost first in the engine's draw order. An entity is on the cell
    /// its picture's middle is in. One whose picture is under the cursor is in it as well, so
    /// that what a press finds there is always in the stack. The first list is those alone
    /// (`entities_at`).
    pub(super) fn stack_at(&mut self, world: (f32, f32)) -> (Vec<i32>, Vec<i32>) {
        self.found_at(world, true)
    }

    /// The visible entities whose picture is under a map position, and with `cell` those on
    /// its cell as well (`stack_at`), each topmost first.
    fn found_at(&mut self, world: (f32, f32), cell: bool) -> (Vec<i32>, Vec<i32>) {
        let (mut under, mut stack) = (Vec::new(), Vec::new());
        let hidden = self.layers.hidden_mask();
        let place = self.cell_of(world);
        let Some(list) = self.document.and_then(|document| self.lists.documents.get(&document)) else { return (under, stack) };
        // Only what comes near the cursor, found with sums alone, since this runs as the mouse
        // moves and a station has tens of thousands of things. Then topmost first, in the
        // engine's draw order (`in_draw_order`) read from its end.
        let mut near: Vec<(i32, u32, f32, i32, (f32, f32))> = list
            .items
            .iter()
            .filter(|(&id, item)| id > 0 && hidden & (1 << group_of(item.kind, item.depth, &self.lists.depths)) == 0)
            .filter_map(|(&id, item)| {
                let (l, b, r, t) = quads_bounds(&item.quads)?;
                let middle = ((l + r) / 2.0, (b + t) / 2.0);
                let over = world.0 >= l && world.0 <= r && world.1 >= b && world.1 <= t;
                let close = cell && (middle.0 - world.0).abs() < 2.0 && (middle.1 - world.1).abs() < 2.0;
                (over || close).then_some((item.depth, item.order, item.sort, id, middle))
            })
            .collect();
        near.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)).then(b.3.cmp(&a.3)));
        let hits: Vec<(i32, (f32, f32), Vec<(u32, f32, f32)>)> =
            near.into_iter().map(|(.., id, middle)| (id, middle, list.items[&id].quads.chunks_exact(FLOATS_PER_QUAD).rev().filter_map(|quad| quad_pixel_at(quad, world)).collect())).collect();
        for (id, middle, hits) in hits {
            if hits.into_iter().any(|(texture, x, y)| self.opaque_at(texture, x, y)) {
                under.push(id);
                stack.push(id);
            } else if cell && self.cell_of(middle) == place {
                stack.push(id);
            }
        }
        (under, stack)
    }

    /// The list the wheel is stepping through, for the interface to draw beside its cell.
    pub fn stack(&self) -> Option<StackList> {
        let (stack, doc) = (self.stepping.stack.as_ref()?, self.doc.as_ref()?);
        // A selection made some other way since is not the list's.
        if self.selection != [stack.entries[stack.current]] {
            return None;
        }
        let entries = stack.entries.iter().map(|uid| doc.entities.get(uid).map(|entity| entity.prototype.clone()).unwrap_or_default()).collect();
        Some(StackList { entries, current: stack.current, cell: self.place_corners(stack.place)? })
    }

    /// The corners of a cell on the map, for a list drawn beside it. None with no document.
    pub(super) fn place_corners(&self, (grid, cell): Place) -> Option<[(f32, f32); 4]> {
        let doc = self.doc.as_ref()?;
        Some(match doc.grid(grid) {
            Some(grid) => grid.cells_outline(cell, cell),
            None => boxed((cell.0 as f32, cell.1 as f32, cell.0 as f32 + 1.0, cell.1 as f32 + 1.0)),
        })
    }

    /// The cell a map position is in: of the grid there, or of the map itself where the
    /// document has no grid.
    pub(super) fn cell_of(&self, world: (f32, f32)) -> Place {
        match self.doc.as_ref().and_then(|doc| doc.grid_at(world)) {
            Some(grid) => (grid.uid, grid.cell_at(world)),
            None => (0, (world.0.floor() as i32, world.1.floor() as i32)),
        }
    }

    /// The wheel turned for the select tool with R not held. It is taken where the cell under
    /// the cursor holds the one selected entity and at least two visible ones (`stack_at`),
    /// with no button down, and steps through them: one down the list for each whole notch
    /// towards the user, one up for each away, round both ends. It begins at the selected one,
    /// and the one stepped to is the selection. With several selected it is not taken, and
    /// zooms. The list stays open, and counts the notches and their parts, while the cursor is
    /// in its cell.
    pub(super) fn step_wheel(&mut self, notches: f32, world: (f32, f32)) -> bool {
        let open = self.stepping.stack.take();
        let (Drag::None, &[chosen]) = (&self.drag, &self.selection[..]) else { return false };
        let entries = self.stack_at(world).1;
        let Some(selected) = entries.iter().position(|&uid| uid == chosen).filter(|_| entries.len() >= 2) else { return false };
        let place = self.cell_of(world);
        let mut stack = match open {
            Some(stack) if stack.place == place && stack.entries == entries && self.selection == [stack.entries[stack.current]] => stack,
            _ => Stack { place, entries, current: selected, notches: 0.0, stepped: 0 },
        };
        stack.notches += notches;
        let whole = stack.notches.round() as i32;
        // A notch towards the user is negative, and goes down the list.
        stack.current = (stack.current as i32 - (whole - stack.stepped)).rem_euclid(stack.entries.len() as i32) as usize;
        stack.stepped = whole;
        self.selection = vec![stack.entries[stack.current]];
        self.stepping.stack = Some(stack);
        true
    }

    /// The cursor moved. The list the wheel steps through closes when it has left its cell.
    pub(super) fn step_moved(&mut self, world: (f32, f32)) {
        if self.stepping.stack.as_ref().is_some_and(|stack| stack.place != self.cell_of(world)) {
            self.stepping.close();
        }
    }

    /// Whether a frame has something drawn at a pixel. True when the frame cannot be read.
    fn opaque_at(&mut self, texture: u32, x: f32, y: f32) -> bool {
        let Some(textures) = self.textures.as_ref() else { return true };
        let frame = self.frames.entry(texture).or_insert_with(|| {
            let source = self.lists.textures.get(&texture)?;
            let pixels = textures.sheet(&source.file())?.frame(source.index, source.width, source.height)?;
            Some((source.width, source.height, pixels))
        });
        match frame {
            Some((width, height, pixels)) => {
                let (x, y) = ((x.floor().max(0.0) as u32).min(*width - 1), (y.floor().max(0.0) as u32).min(*height - 1));
                pixels[((y * *width + x) * 4 + 3) as usize] > 16
            }
            None => true,
        }
    }

    /// The box around an entity's picture on the map: (left, bottom, right, top).
    pub fn entity_bounds(&self, uid: i32) -> Option<(f32, f32, f32, f32)> {
        quads_bounds(&self.lists.documents.get(&self.document?)?.items.get(&uid)?.quads)
    }

    /// Visible entities whose picture's middle is inside a box on the map.
    fn entities_in(&self, a: (f32, f32), b: (f32, f32)) -> Vec<i32> {
        let (left, right, bottom, top) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        self.entities_where(|(x, y)| x >= left && x <= right && y >= bottom && y <= top)
    }

    /// Visible entities whose picture's middle on the map passes a test, in the order of
    /// their uids. What is on a hidden layer is left out.
    pub(super) fn entities_where(&self, inside: impl Fn((f32, f32)) -> bool) -> Vec<i32> {
        let Some(document) = self.document.and_then(|id| self.lists.documents.get(&id)) else { return Vec::new() };
        let hidden = self.layers.hidden_mask();
        let mut found: Vec<i32> = document
            .items
            .iter()
            .filter(|(&id, item)| id > 0 && !item.quads.is_empty() && hidden & (1 << group_of(item.kind, item.depth, &self.lists.depths)) == 0)
            .filter(|(_, item)| quads_bounds(&item.quads).is_some_and(|(l, b, r, t)| inside(((l + r) / 2.0, (b + t) / 2.0))))
            .map(|(&id, _)| id)
            .collect();
        found.sort_unstable();
        found
    }

    /// Whether an entity is drawn on a layer that shows.
    pub(super) fn entity_shown(&self, uid: i32) -> bool {
        let Some(document) = self.document.and_then(|id| self.lists.documents.get(&id)) else { return false };
        let hidden = self.layers.hidden_mask();
        document.items.get(&uid).is_some_and(|item| !item.quads.is_empty() && hidden & (1 << group_of(item.kind, item.depth, &self.lists.depths)) == 0)
    }

    /// Deletes the selected entities and decals, as one edit.
    pub fn delete_selection(&mut self) {
        let mut ops: Vec<Value> = self.selection.iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        ops.extend(self.selected_decal_list().iter().map(|(grid, decal)| json!({ "kind": "decal.remove", "grid": grid, "uid": decal.id })));
        self.edit(ops, "Delete", None, false);
    }

    /// Clears the selection of entities and of decals.
    pub fn deselect(&mut self) {
        self.selection.clear();
        self.selected_decals.clear();
    }

    /// A selection of more than one entity summed up for a panel: how many, and the three
    /// prototypes most of them are of as "id xN", with how many more there are. Those of as
    /// many go by their ids. None for one entity or none.
    pub fn selection_summary(&self) -> Option<(String, String)> {
        let doc = self.doc.as_ref()?;
        if self.selection.len() < 2 {
            return None;
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for entity in self.selection.iter().filter_map(|uid| doc.entities.get(uid)) {
            *counts.entry(entity.prototype.as_str()).or_default() += 1;
        }
        let mut counts: Vec<(&str, usize)> = counts.into_iter().collect();
        counts.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut parts: Vec<String> = counts.iter().take(3).map(|(id, count)| format!("{id} x{count}")).collect();
        if counts.len() > 3 {
            parts.push(format!("+{} more types", counts.len() - 3));
        }
        Some((format!("{} entities selected", self.selection.len()), parts.join(", ")))
    }

    /// A quarter turn: of what the place tool, a brush or Fill puts down next
    /// (`turns_placing`), of the area tool's region, of the ghost of a paste, or else of the
    /// selected entities.
    pub fn rotate(&mut self, clockwise: bool) {
        match self.tool {
            _ if self.turns_placing() => self.placing.turn(if clockwise { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 }),
            Tool::Area | Tool::Select if self.pasting.is_live() => self.turn_paste(clockwise),
            Tool::Area => self.turn_region(clockwise),
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Lasso | Tool::Polygon | Tool::Room | Tool::Select | Tool::Place | Tool::Link | Tool::Cable | Tool::Pipe => self.rotate_selection(clockwise),
        }
    }

    /// A quarter turn of the selected entities and decals, whatever tool is in hand, as one
    /// step to undo. Each angle is counted on in quarters (`quarter_on`). A decal turns about
    /// its own middle and stays where it is.
    pub fn rotate_selection(&mut self, clockwise: bool) {
        let Some(doc) = self.doc.as_ref() else { return };
        let mut ops: Vec<Value> = self
            .selection
            .iter()
            .filter_map(|uid| doc.entities.get(uid))
            .map(|entity| json!({ "kind": "rotate", "uid": entity.uid, "rotation": quarter_on(entity.rotation, clockwise) }))
            .collect();
        for (grid, mut decal) in self.selected_decal_list() {
            decal.angle = quarter_on(decal.angle, clockwise);
            ops.push(decal_update_op(grid, &decal));
        }
        self.edit(ops, "Rotate", None, false);
    }

    /// A key went down for a tool that turns things. R begins a hold and turns nothing yet.
    /// Shift is read here and not as R comes up. R that goes down again while it is held, as
    /// a keyboard repeats it, leaves the hold as it is. Says whether it took the key.
    pub(super) fn turn_key_down(&mut self, key: char, shift: bool) -> bool {
        if key != 'R' {
            return false;
        }
        if self.turning.hold.is_none() {
            self.turning.hold = Some(Hold { shift, wheeled: false, notches: 0.0, turned: 0, selection: None });
        }
        true
    }

    /// A key came up for a tool that turns things. R ends its hold, with a quarter turn if
    /// the wheel was not turned in it: clockwise, and the other way if it began with Shift.
    pub(super) fn turn_key_up(&mut self, key: char) {
        if key != 'R' {
            return;
        }
        if let Some(Hold { shift, wheeled: false, .. }) = self.turning.hold.take() {
            self.rotate(!shift);
        }
    }

    /// A key was lost for a tool that turns things. A hold of R ends with nothing more asked.
    pub(super) fn turn_key_lost(&mut self, key: char) {
        if key == 'R' {
            self.turning.end();
        }
    }

    /// The wheel turned for a tool that turns things. It is taken while R is held, and turns
    /// by five degrees for each whole notch the wheel has come to since R went down, clockwise
    /// when turned towards the user: what the place tool places next, or else the selected
    /// entities, each about its own position. The turns of the selection in one hold are one
    /// step to undo. Each states the angle of every entity from where it stood when the first
    /// began, as a move does, so that the mirror being behind the engine adds nothing up.
    /// Another selection in the same hold begins a step of its own from where the wheel is.
    pub(super) fn turn_wheel(&mut self, notches: f32) -> bool {
        let Some(mut hold) = self.turning.hold.take() else { return false };
        hold.wheeled = true;
        hold.notches += notches;
        let whole = hold.notches.round() as i32;
        if whole != hold.turned && self.turns_placing() {
            self.placing.turn(f64::from(whole - hold.turned) * NOTCH_TURN);
            hold.turned = whole;
        } else if whole != hold.turned {
            let turned = match hold.selection.take() {
                Some(turned) if turned.selection == self.selection && turned.selected_decals == self.selected_decals => turned,
                _ => {
                    // The count starts again where this selection stands now.
                    hold.notches -= hold.turned as f32;
                    hold.turned = 0;
                    let originals: Vec<(i32, f64)> = self
                        .doc
                        .as_ref()
                        .map(|doc| self.selection.iter().filter_map(|uid| doc.entities.get(uid)).map(|entity| (entity.uid, entity.rotation)).collect())
                        .unwrap_or_default();
                    let decals = self.selected_decal_list();
                    Turned { selection: self.selection.clone(), selected_decals: self.selected_decals.clone(), stroke: self.stroke(), originals, decals }
                }
            };
            hold.turned = hold.notches.round() as i32;
            let turn = f64::from(hold.turned) * NOTCH_TURN;
            let mut ops: Vec<Value> =
                turned.originals.iter().map(|&(uid, rotation)| json!({ "kind": "rotate", "uid": uid, "rotation": (rotation + turn).rem_euclid(std::f64::consts::TAU) })).collect();
            for (grid, decal) in &turned.decals {
                let turned_decal = Decal { angle: (decal.angle + turn).rem_euclid(std::f64::consts::TAU), ..decal.clone() };
                ops.push(decal_update_op(*grid, &turned_decal));
            }
            self.edit(ops, "Rotate", Some(turned.stroke), true);
            hold.selection = Some(turned);
        }
        self.turning.hold = Some(hold);
        true
    }

    /// Where a drag by `delta` on the map puts each entity, in its parent's own coordinates.
    /// The way it goes is rounded to whole cells of the parent, so a thing keeps its place
    /// within its cell. With `free` it goes by the delta exactly, unless one of them is fixed
    /// to the floor: the engine keeps that one on the middle of a cell, and the rest go as it
    /// does, so that they all stay as they stood to each other. What is on no grid goes by
    /// the map's own units. A decal goes as an entity of its grid does, and stays on its grid.
    fn move_ops(&self, originals: &[Entity], decals: &[(i32, Decal)], delta: (f32, f32), free: bool) -> Vec<Value> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let whole = !free || originals.iter().any(|entity| entity.anchored);
        let local = |grid: Option<&Grid>| {
            let local = grid.map_or(delta, |grid| grid.direction_to_local(delta));
            if whole { (local.0.round(), local.1.round()) } else { local }
        };
        let mut ops: Vec<Value> = originals
            .iter()
            .map(|entity| {
                let local = local(doc.grid(entity.parent));
                json!({ "kind": "move", "uid": entity.uid, "x": entity.x + local.0, "y": entity.y + local.1 })
            })
            .collect();
        for (grid, decal) in decals {
            let local = local(doc.grid(*grid));
            ops.push(decal_update_op(*grid, &Decal { x: decal.x + local.0, y: decal.y + local.1, ..decal.clone() }));
        }
        ops
    }

    /// The select tool was pressed. The entity it is pressed on is the topmost one under the
    /// cursor. A press that closes the wheel's list is on the entity the list is on instead,
    /// while it is in the cell's stack, so that what the wheel stepped to under another can be
    /// dragged at once. On an entity it selects it and begins a move of the selection. With
    /// Shift held it only adds an entity that is not selected, and begins no drag, so that a
    /// hand that wobbles on the click moves nothing. One that is selected it leaves in until
    /// the press is let go: a press that was dragged has moved the selection, and one that
    /// was not takes the entity out (`move_up`). On nothing it begins a box, which adds with
    /// Shift held. With Ctrl held it takes the entity under it out of the selection and
    /// begins a box that takes away, and never a move.
    ///
    /// Where no entity is under the cursor the topmost decal there is pressed on in the same
    /// way, as the web editor does (`decals_at`): a plain press selects it alone and begins a
    /// move, Shift adds it or takes it out, and Ctrl takes it out. A move takes the selected
    /// entities and decals along together.
    ///
    /// Select's other shapes go through `select_shape_down`.
    pub(super) fn select_down(&mut self, world: (f32, f32), point: (f32, f32)) {
        if self.select_shape() != SelectShape::Rectangle {
            self.select_shape_down(world, point);
            return;
        }
        // The entity the list is on, while it is the whole selection.
        let stepped = self.stepping.stack.take().map(|stack| stack.entries[stack.current]).filter(|&uid| self.selection == [uid]);
        let (under, stack) = if stepped.is_some() { self.stack_at(world) } else { (self.entities_at(world), Vec::new()) };
        let targets = self.select_targets();
        let entity = stepped.filter(|uid| stack.contains(uid)).or(under.first().copied()).filter(|_| targets.entities);
        let decal = if entity.is_none() && targets.decals { self.decals_at(world).first().copied() } else { None };
        // Shift adds and Ctrl takes away, as the toolbar's mode does.
        let mode = self.press_mode();
        let adds = mode == SelectMode::Add;
        match (entity, decal) {
            // Subtract comes first, so that Ctrl wins over Shift.
            (under, decal) if mode == SelectMode::Subtract => {
                if let Some(uid) = under {
                    self.selection.retain(|&selected| selected != uid);
                }
                if let Some(key) = decal {
                    self.selected_decals.retain(|&selected| selected != key);
                }
                self.drag = Drag::SelectBox(BoxDrag { from: world, to: world, mode: SelectMode::Subtract });
            }
            (Some(uid), _) if adds && !self.selection.contains(&uid) => self.selection.push(uid),
            (Some(uid), _) => {
                let selected = self.selection.contains(&uid);
                let takes_out = (adds && selected).then_some(uid);
                if !selected {
                    self.selection = vec![uid];
                    self.selected_decals.clear();
                }
                self.begin_move(world, point, takes_out, None);
            }
            (None, Some(key)) if adds && !self.selected_decals.contains(&key) => self.selected_decals.push(key),
            (None, Some(key)) => {
                let selected = self.selected_decals.contains(&key);
                let takes_out = (adds && selected).then_some(key);
                if !selected {
                    self.selection.clear();
                    self.selected_decals = vec![key];
                }
                self.begin_move(world, point, None, takes_out);
            }
            (None, None) => {
                let mode = if adds { SelectMode::Add } else { SelectMode::Replace };
                self.drag = Drag::SelectBox(BoxDrag { from: world, to: world, mode });
            }
        }
    }

    /// Select was pressed with a shape other than Rectangle. Select same takes what is under
    /// the cursor at once. With the others a plain press on what is selected moves the
    /// selection, as with Rectangle, and any other press begins the shape, even over an
    /// entity, so that a lasso can begin anywhere.
    fn select_shape_down(&mut self, world: (f32, f32), point: (f32, f32)) {
        let shape = self.select_shape();
        let mode = self.press_mode();
        if shape == SelectShape::Same {
            self.select_same_things(world, mode);
            return;
        }
        if mode == SelectMode::Replace {
            let targets = self.select_targets();
            let entity = self.entities_at(world).first().copied().filter(|_| targets.entities);
            let decal = if entity.is_none() && targets.decals { self.decals_at(world).first().copied() } else { None };
            let selected = entity.is_some_and(|uid| self.selection.contains(&uid)) || decal.is_some_and(|key| self.selected_decals.contains(&key));
            if selected {
                self.begin_move(world, point, None, None);
                return;
            }
        }
        self.marquee_down(world, None, shape, mode);
    }

    /// Begins a move of the selected entities and decals from a press.
    fn begin_move(&mut self, world: (f32, f32), point: (f32, f32), takes_out: Option<i32>, takes_out_decal: Option<(i32, i32)>) {
        let originals: Vec<Entity> = self.doc.as_ref().map(|doc| self.selection.iter().filter_map(|uid| doc.entities.get(uid).cloned()).collect()).unwrap_or_default();
        let decals = self.selected_decal_list();
        let stroke = self.stroke();
        self.drag = Drag::Move(MoveDrag { stroke, from: world, pressed: point, originals, decals, moved: false, takes_out, takes_out_decal });
    }

    /// The select tool was pressed with the secondary button. With the wheel's list open it
    /// closes the list and keeps the selection, as the web editor does. Otherwise nothing is
    /// selected any more, whatever is under the cursor.
    pub(super) fn select_secondary_down(&mut self) {
        if !self.stepping.close() {
            self.deselect();
        }
    }

    /// A move was dragged on: the selected entities follow the cursor from where they started,
    /// by whole cells, or by the cursor exactly while Shift is held (`move_ops`). Shift is read
    /// anew each time, so it may be pressed or let go on the way. Nothing moves until the
    /// cursor is more than four pixels from the press. A drag that gets that far with Shift
    /// held and something anchored selected says on the status line why it keeps to cells.
    pub(super) fn move_dragged(&mut self, world: (f32, f32), point: (f32, f32), drag: MoveDrag) -> Drag {
        // A press that barely moves is a click, not a drag.
        let far = (point.0 - drag.pressed.0).hypot(point.1 - drag.pressed.1) > 4.0;
        if far && !drag.moved && self.held.shift && drag.originals.iter().any(|entity| entity.anchored) {
            self.set_status("Shift only moves freely when nothing selected is anchored");
        }
        if drag.moved || far {
            let ops = self.move_ops(&drag.originals, &drag.decals, (world.0 - drag.from.0, world.1 - drag.from.1), self.held.shift);
            if let Some(op) = ops.first().filter(|op| op["kind"] == "move") {
                let (x, y) = (op["x"].as_f64().unwrap_or(0.0), op["y"].as_f64().unwrap_or(0.0));
                self.last_put = Some(format!("moved {} to ({x:.3}, {y:.3}), {} held", op["uid"], self.held.words()));
            }
            self.edit(ops, "Move", Some(drag.stroke), true);
        }
        Drag::Move(MoveDrag { moved: drag.moved || far, ..drag })
    }

    /// A move was let go. A press with Shift on a selected entity or decal that was never
    /// dragged was a click, which takes it out of the selection.
    pub(super) fn move_up(&mut self, drag: MoveDrag) {
        if drag.moved {
            return;
        }
        if let Some(uid) = drag.takes_out {
            self.selection.retain(|&selected| selected != uid);
        }
        if let Some(key) = drag.takes_out_decal {
            self.selected_decals.retain(|&selected| selected != key);
        }
    }

    /// A selection box was dragged on: its far corner follows the cursor.
    pub(super) fn select_box_dragged(&self, world: (f32, f32), drag: BoxDrag) -> Drag {
        let BoxDrag { from, mode, .. } = drag;
        Drag::SelectBox(BoxDrag { from, to: world, mode })
    }

    /// A selection box was let go: selects the entities and decals in it, adds them to the
    /// selection or takes them out of it. It does what it was begun for, whatever is held now.
    /// A box of no size is a click and finds nothing, since the press has dealt with what was
    /// under it.
    pub(super) fn select_box_up(&mut self, world: (f32, f32), drag: BoxDrag) {
        let BoxDrag { from, mode, .. } = drag;
        let (found, decals) = if from == world { (Vec::new(), Vec::new()) } else { (self.entities_in(from, world), self.decals_in(from, world)) };
        self.apply_selection(found, decals, mode);
    }

    /// Entities and decals that a shape of Select found: those of the select targets take the
    /// selection's place, are added to it or are taken out of it, as the mode says.
    pub(super) fn apply_selection(&mut self, mut found: Vec<i32>, mut decals: Vec<(i32, i32)>, mode: SelectMode) {
        let targets = self.select_targets();
        found.retain(|_| targets.entities);
        decals.retain(|_| targets.decals);
        match mode {
            SelectMode::Replace => (self.selection, self.selected_decals) = (found, decals),
            SelectMode::Add => {
                for uid in found {
                    if !self.selection.contains(&uid) {
                        self.selection.push(uid);
                    }
                }
                for key in decals {
                    if !self.selected_decals.contains(&key) {
                        self.selected_decals.push(key);
                    }
                }
            }
            SelectMode::Subtract => {
                self.selection.retain(|uid| !found.contains(uid));
                self.selected_decals.retain(|key| !decals.contains(key));
            }
        }
    }

    /// The outline of a selection box being dragged out, which says what the box is for.
    pub(super) fn select_box_outlines(&self, drag: &BoxDrag, outlines: &mut Vec<Outline>) {
        let BoxDrag { from, to, mode } = drag;
        let style = mode_style(*mode);
        outlines.push(Outline { corners: boxed((from.0.min(to.0), from.1.min(to.1), from.0.max(to.0), from.1.max(to.1))), style });
    }

    /// The rectangles the engine draws the selected entities with, for the renderer to put a
    /// border around (`MapRenderer::set_selected`).
    pub fn selection_quads(&self) -> Vec<f32> {
        let Some(document) = self.document.and_then(|id| self.lists.documents.get(&id)) else { return Vec::new() };
        self.selection.iter().filter_map(|uid| document.items.get(uid)).flat_map(|item| item.quads.iter().copied()).collect()
    }
}
