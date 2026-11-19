//! Decals: choosing one in the palette, the settings it is placed with, and placing it.

use super::*;
use crate::model::{Decal, Grid};

/// The tags whose groups a decal palette shows first, in this order, as the web editor does.
const FIRST_TAGS: [&str; 4] = ["station", "markings", "flora", "dirty"];

/// The group of a decal with no tags.
const UNTAGGED: &str = "other";

/// What a decal is placed with, as the web editor's placement settings have it.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalSettings {
    /// The tint as #RRGGBBAA. None leaves the prototype's own colours.
    pub color: Option<String>,
    /// How it is turned, in radians.
    pub angle: f64,
    pub z_index: i32,
    pub cleanable: bool,
}

impl Default for DecalSettings {
    fn default() -> Self {
        DecalSettings { color: None, angle: 0.0, z_index: 0, cleanable: false }
    }
}

/// A group of the decal palette: a tag, and the decals it holds as indices into
/// [`Session::decal_prototypes`].
#[derive(Clone, Debug, PartialEq)]
pub struct DecalGroup {
    pub tag: String,
    pub decals: Vec<usize>,
}

/// What the decal tools keep from one drag to the next: the placement settings, an edit of
/// the selected decals' fields that is going on, and whether R is held since it turned the
/// placement angle, so that the wheel turns it too.
#[derive(Default)]
pub(in crate::session) struct Decaling {
    settings: DecalSettings,
    editing: Option<FieldEdit>,
    held: bool,
}

/// How far a notch of the wheel turns the placement angle with R held: five degrees, as in the
/// web editor.
pub(super) const WHEEL_TURN: f64 = std::f64::consts::PI / 36.0;

/// One field of a decal, with the value a panel gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum DecalField {
    Color(Option<String>),
    /// In radians.
    Angle(f64),
    ZIndex(i32),
    Cleanable(bool),
}

impl DecalField {
    /// The decal with this field changed.
    fn applied(&self, decal: &Decal) -> Decal {
        let mut decal = decal.clone();
        match self {
            DecalField::Color(color) => decal.color = color.clone(),
            DecalField::Angle(angle) => decal.angle = *angle,
            DecalField::ZIndex(z_index) => decal.z_index = *z_index,
            DecalField::Cleanable(cleanable) => decal.cleanable = *cleanable,
        }
        decal
    }
}

/// A gesture of a panel on one field of the selected decals: which decals, which field, its
/// one step to undo, and each decal as it was when the gesture began.
struct FieldEdit {
    selection: Vec<(i32, i32)>,
    field: std::mem::Discriminant<DecalField>,
    stroke: u64,
    originals: Vec<(i32, Decal)>,
}

/// The selected decals summed up for a panel. A field is None where they differ.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalSummary {
    pub count: usize,
    /// The three prototypes most of them are of as "id xN", with how many more there are.
    pub breakdown: String,
    pub prototype: Option<String>,
    /// Some(None) when none has a colour of its own.
    pub color: Option<Option<String>>,
    pub angle: Option<f64>,
    pub z_index: Option<i32>,
    pub cleanable: Option<bool>,
    /// How many decals on their grids have the colour they share, if they share one.
    pub same_colour: usize,
}

/// The one value every item has, or None where they differ or there are none.
fn shared<T: PartialEq + Clone, I>(items: &[I], value: impl Fn(&I) -> T) -> Option<T> {
    let first = value(items.first()?);
    items.iter().all(|item| value(item) == first).then_some(first)
}

/// A colour as #RRGGBBAA, read into its four bytes. #RRGGBB has an alpha of 255. None for
/// anything else.
pub fn decal_colour_bytes(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, if hex.len() == 8 { byte(6)? } else { 255 }])
}

/// Four bytes of a colour as #RRGGBBAA, in capitals as the engine writes them.
pub fn decal_colour_text(bytes: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2], bytes[3])
}

impl Session {
    /// Chooses the decal the brushes put down, by its prototype's id, which makes Decals the
    /// target. A brush that works with an entity works with a decal (`Tool::brushes_entities`)
    /// and stays in hand. Any other tool is put down for Paint in its shape, as in the web editor.
    pub fn choose_decal(&mut self, id: &str) {
        self.palette = Palette::Decal(id.to_owned());
        self.chosen();
        if !self.tool.brushes_entities() {
            self.take_bar_tool(BarTool::Paint);
        }
        self.hint_answered();
    }

    /// The decal the palette holds, by its prototype's id, if it holds a decal.
    pub fn chosen_decal(&self) -> Option<&str> {
        match &self.palette {
            Palette::Decal(id) => Some(id),
            _ => None,
        }
    }

    /// The decal prototype of this id, if the engine knows one.
    pub fn decal_prototype(&self, id: &str) -> Option<&DecalPrototype> {
        self.decal_prototypes.iter().find(|decal| decal.id == id)
    }

    /// What a decal is placed with now.
    pub fn decal_settings(&self) -> &DecalSettings {
        &self.decaling.settings
    }

    /// R went down for a brush. With a decal in hand it turns the placement angle by a quarter,
    /// clockwise, and the other way with Shift, as in the web editor. Says whether it took the
    /// key: without a decal R is the rectangle tool. R stays held for the wheel till it comes
    /// up or is lost.
    pub(super) fn decal_turn_key(&mut self, key: char, shift: bool) -> bool {
        if key != 'R' || self.chosen_decal().is_none() {
            return false;
        }
        let quarter = std::f64::consts::FRAC_PI_2;
        self.turn_decal_angle(if shift { quarter } else { -quarter });
        self.decaling.held = true;
        true
    }

    /// R came up, or was lost, for a brush.
    pub(super) fn decal_turn_key_up(&mut self, key: char) {
        if key == 'R' {
            self.decaling.held = false;
        }
    }

    /// Whether the wheel turns the placement angle now: R held since it turned it, a decal
    /// chosen and a brush that places one in hand.
    pub(super) fn decal_wheel_turns(&self) -> bool {
        self.decaling.held && self.chosen_decal().is_some() && matches!(self.tool, Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle)
    }

    /// The wheel turned with R held for a brush: the placement angle turns by five degrees a
    /// notch, the other way from R's quarter when turned away, as in the web editor. Says
    /// whether it took the wheel.
    pub(super) fn decal_wheel(&mut self, notches: f32) -> bool {
        if !self.decal_wheel_turns() {
            return false;
        }
        self.turn_decal_angle(notches as f64 * WHEEL_TURN);
        true
    }

    /// Turns the placement angle by so many radians.
    fn turn_decal_angle(&mut self, by: f64) {
        let mut settings = self.decal_settings().clone();
        settings.angle += by;
        self.set_decal_settings(settings);
    }

    /// Takes new placement settings. The angle is kept from 0 up to a whole turn.
    pub fn set_decal_settings(&mut self, mut settings: DecalSettings) {
        settings.angle = settings.angle.rem_euclid(std::f64::consts::TAU);
        self.decaling.settings = settings;
    }

    /// The decals a palette offers for a search: those whose id has every one of the words in
    /// it, in either case, as indices into [`Self::decal_prototypes`] in its order.
    pub fn decals_matching(&self, words: &[&str]) -> Vec<usize> {
        let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
        self.decal_prototypes
            .iter()
            .enumerate()
            .filter(|(_, decal)| {
                let id = decal.id.to_lowercase();
                words.iter().all(|word| id.contains(word.as_str()))
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Every decal in groups by its first tag, as the web editor groups them: the groups of
    /// `FIRST_TAGS` first, then the rest by name. A decal with no tags is in "other". Within a
    /// group the decals are in the order of their ids.
    pub fn decal_groups(&self) -> Vec<DecalGroup> {
        let mut groups: Vec<DecalGroup> = Vec::new();
        for (index, decal) in self.decal_prototypes.iter().enumerate() {
            let tag = decal.tags.first().map_or(UNTAGGED, String::as_str);
            match groups.iter_mut().find(|group| group.tag == tag) {
                Some(group) => group.decals.push(index),
                None => groups.push(DecalGroup { tag: tag.to_owned(), decals: vec![index] }),
            }
        }
        let rank = |tag: &str| FIRST_TAGS.iter().position(|first| *first == tag).unwrap_or(FIRST_TAGS.len());
        groups.sort_by(|a, b| rank(&a.tag).cmp(&rank(&b.tag)).then_with(|| a.tag.cmp(&b.tag)));
        for group in &mut groups {
            group.decals.sort_by(|&a, &b| self.decal_prototypes[a].id.cmp(&self.decal_prototypes[b].id));
        }
        groups
    }

    // ---- placing ----

    /// Whether the paint tool puts a decal down freely, with its middle on the cursor: the
    /// toolbar's Free, as for entities, or the other way with Shift held (`places_free`). The
    /// shapes always go by cells.
    pub(super) fn decal_free(&self) -> bool {
        self.places_free()
    }

    /// A new decal of a prototype with its lower left corner at a place of a grid, with the
    /// placement settings, as the kernel's `decal` op takes it.
    fn new_decal_op(&self, grid: i32, corner: (f32, f32), id: &str) -> Value {
        let settings = &self.decaling.settings;
        json!({
            "kind": "decal", "grid": grid, "x": corner.0, "y": corner.1, "prototype": id,
            "color": settings.color, "rotation": settings.angle, "zIndex": settings.z_index, "cleanable": settings.cleanable,
        })
    }

    /// Where a decal would go for a cursor at `world`: the grid the tile tools work on there,
    /// and the decal's lower left corner in that grid's coordinates. That is the corner of the
    /// cursor's cell, or placed freely the place that puts the decal's middle on the cursor.
    fn decal_target(&self, world: (f32, f32), free: bool) -> Option<(i32, (f32, f32))> {
        let grid = self.doc.as_ref()?.grid_at(world)?;
        let corner = if free {
            let local = grid.to_local(world);
            (local.0 - 0.5, local.1 - 0.5)
        } else {
            let cell = grid.cell_at(world);
            (cell.0 as f32, cell.1 as f32)
        };
        Some((grid.uid, corner))
    }

    /// Asks for a decal of a prototype on the corner of each of these cells of a grid, as one
    /// edit. A cell with no tile is skipped, since the engine refuses a decal there, as is one
    /// in `placed`. The cells it puts one on go into `placed`. Decals may stack: a shape, a fill
    /// and a stroke past the cell pressed put the cells that hold one already in `placed`
    /// first (`decals_held`), as an entity's brush skips a cell that holds it, and a click puts
    /// one on whatever is there.
    pub(super) fn put_decals(&mut self, grid_uid: i32, cells: &[(i32, i32)], id: &str, label: &'static str, stroke: Option<u64>, placed: &mut HashSet<(i32, i32)>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(grid) = doc.grids.iter().find(|grid| grid.uid == grid_uid) else { return };
        doc.active_grid = grid_uid;
        self.grid = (grid.uid, grid.x, grid.y);
        let cells: Vec<(i32, i32)> = cells.iter().copied().filter(|cell| grid.tiles.contains_key(cell) && placed.insert(*cell)).collect();
        let ops = cells.into_iter().map(|cell| self.new_decal_op(grid_uid, (cell.0 as f32, cell.1 as f32), id)).collect();
        self.edit(ops, label, stroke, false);
    }

    /// The cells of a grid that hold a decal of a prototype: those its lower left corner is in.
    pub(super) fn decals_held(&self, grid: i32, id: &str) -> HashSet<(i32, i32)> {
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid)) else { return HashSet::new() };
        grid.decals.values().filter(|decal| decal.prototype == id).map(decal_cell).collect()
    }

    /// Asks for the decals on these cells of a grid to be removed: those whose lower left corner
    /// is in one of them, as the web editor finds them, all of them or with `only` those of a
    /// prototype. None while the decal layer is hidden.
    pub(super) fn delete_decals_on(&mut self, grid_uid: i32, cells: &[(i32, i32)], only: Option<&str>, stroke: Option<u64>) {
        if !self.layers.decals {
            return;
        }
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid_uid)) else { return };
        let cells: HashSet<(i32, i32)> = cells.iter().copied().collect();
        let mut ids: Vec<i32> = grid.decals.values().filter(|decal| only.is_none_or(|id| decal.prototype == id) && cells.contains(&decal_cell(decal))).map(|decal| decal.id).collect();
        ids.sort_unstable();
        let ops = ids.into_iter().map(|decal| json!({ "kind": "decal.remove", "grid": grid_uid, "uid": decal })).collect();
        self.edit(ops, "Erase decals", stroke, false);
    }

    /// Places one decal with its middle on the cursor, as one edit. The engine wants a tile
    /// under the decal's corner, so where there is none nothing is asked and the status line
    /// says why.
    pub(super) fn place_decal_freely(&mut self, world: (f32, f32), id: &str) {
        let Some((grid, corner)) = self.decal_target(world, true) else { return };
        let floored = (corner.0.floor() as i32, corner.1.floor() as i32);
        if !self.doc.as_ref().and_then(|doc| doc.grid(grid)).is_some_and(|grid| grid.tiles.contains_key(&floored)) {
            self.set_status("Not placed: a decal needs a tile under its lower-left corner");
            return;
        }
        if let Some(doc) = self.doc.as_mut() {
            doc.active_grid = grid;
        }
        let op = self.new_decal_op(grid, corner, id);
        self.edit(vec![op], "Place decal", None, false);
    }

    /// The ghost of the chosen decal under the cursor, for `overlay_quads`: its picture, tinted
    /// and turned as it would be placed and half seen through, where a press of the tool in hand
    /// would put it. Only the tools that put a decal down show it, and not while a shape is
    /// dragged out, which shows its cells instead.
    pub(super) fn decal_overlay(&mut self, over: &mut Vec<f32>) {
        let shows = matches!(self.tool, Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle) && matches!(self.drag, Drag::None | Drag::Stroke(_));
        let (true, Some(world), Some(id)) = (shows, self.hover, self.chosen_decal()) else { return };
        let Some(file) = self.decal_prototype(id).and_then(DecalPrototype::file) else { return };
        let free = self.tool == Tool::Paint && self.decal_free();
        let Some((grid, corner)) = self.decal_target(world, free) else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid)) else { return };
        let corners = decal_corners(grid, corner, self.decaling.settings.angle);
        let [r, g, b, a] = self.decaling.settings.color.as_deref().and_then(decal_colour_bytes).unwrap_or([255; 4]);
        let texture = self.lists.local_texture(&file, 0, 32, 32);
        for corner in corners {
            over.extend_from_slice(&[corner.0, corner.1]);
        }
        over.extend_from_slice(&[f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, f32::from(a) / 255.0 * 0.5]);
        over.push(texture as f32);
        over.extend_from_slice(&[0.0, 0.0, 32.0, 32.0]);
    }
}

impl Session {
    // ---- finding and changing decals on the map ----

    /// A decal of the mirror, by its grid's uid and its id.
    pub fn decal(&self, (grid, id): (i32, i32)) -> Option<&Decal> {
        self.doc.as_ref()?.grid(grid)?.decals.get(&id)
    }

    /// The decals whose cell-sized square is under a map position, top first: the higher
    /// z-index, and of the same the later id. None while the decal layer is hidden.
    pub(super) fn decals_at(&self, world: (f32, f32)) -> Vec<(i32, i32)> {
        let Some(doc) = self.doc.as_ref().filter(|_| self.layers.decals) else { return Vec::new() };
        let mut found: Vec<(i32, &Decal)> = Vec::new();
        for grid in &doc.grids {
            let local = grid.to_local(world);
            let under = grid.decals.values().filter(|decal| local.0 >= decal.x && local.0 < decal.x + 1.0 && local.1 >= decal.y && local.1 < decal.y + 1.0);
            found.extend(under.map(|decal| (grid.uid, decal)));
        }
        found.sort_by(|a, b| (b.1.z_index, b.1.id).cmp(&(a.1.z_index, a.1.id)));
        found.into_iter().map(|(grid, decal)| (grid, decal.id)).collect()
    }

    /// The decals whose middle is inside a box on the map, in the order of their grids and
    /// ids. None while the decal layer is hidden.
    pub(super) fn decals_in(&self, a: (f32, f32), b: (f32, f32)) -> Vec<(i32, i32)> {
        let Some(doc) = self.doc.as_ref().filter(|_| self.layers.decals) else { return Vec::new() };
        let (left, right, bottom, top) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        let mut found = Vec::new();
        for grid in &doc.grids {
            for decal in grid.decals.values() {
                let (x, y) = grid.to_world((decal.x + 0.5, decal.y + 0.5));
                if x >= left && x <= right && y >= bottom && y <= top {
                    found.push((grid.uid, decal.id));
                }
            }
        }
        found.sort_unstable();
        found
    }

    /// The selected decals as the mirror has them, with their grids' uids.
    pub fn selected_decal_list(&self) -> Vec<(i32, Decal)> {
        self.selected_decals.iter().filter_map(|&key| self.decal(key).map(|decal| (key.0, decal.clone()))).collect()
    }

    // ---- the decal panel ----

    /// The selected decals summed up for a panel. None with none selected.
    pub fn decal_summary(&self) -> Option<DecalSummary> {
        let decals = self.selected_decal_list();
        if decals.is_empty() {
            return None;
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for (_, decal) in &decals {
            match counts.iter_mut().find(|(id, _)| *id == decal.prototype) {
                Some((_, count)) => *count += 1,
                None => counts.push((&decal.prototype, 1)),
            }
        }
        counts.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut parts: Vec<String> = counts.iter().take(3).map(|(id, count)| format!("{id} x{count}")).collect();
        if counts.len() > 3 {
            parts.push(format!("+{} more types", counts.len() - 3));
        }
        let color = shared(&decals, |(_, decal)| decal.color.clone());
        let same_colour = color.as_ref().map_or(0, |color| self.decals_coloured(&decals, color).len());
        Some(DecalSummary {
            count: decals.len(),
            breakdown: parts.join(", "),
            prototype: shared(&decals, |(_, decal)| decal.prototype.clone()),
            color,
            angle: shared(&decals, |(_, decal)| decal.angle),
            z_index: shared(&decals, |(_, decal)| decal.z_index),
            cleanable: shared(&decals, |(_, decal)| decal.cleanable),
            same_colour,
        })
    }

    /// Every decal on the grids of these decals that has this colour, in the order of grids and ids.
    fn decals_coloured(&self, on: &[(i32, Decal)], color: &Option<String>) -> Vec<(i32, Decal)> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let mut grids: Vec<i32> = on.iter().map(|(grid, _)| *grid).collect();
        grids.sort_unstable();
        grids.dedup();
        let mut found: Vec<(i32, Decal)> =
            grids.iter().filter_map(|&grid| doc.grid(grid)).flat_map(|grid| grid.decals.values().filter(|decal| &decal.color == color).map(|decal| (grid.uid, decal.clone()))).collect();
        found.sort_unstable_by_key(|(grid, decal)| (*grid, decal.id));
        found
    }

    /// Sets one field of every selected decal, as a panel changes it. The changes of one
    /// gesture on one field are one step to undo, and each states every decal from where it
    /// was when the gesture began. The gesture goes on while the same field of the same
    /// decals is changed, until `end_decal_edit`.
    pub fn edit_selected_decals(&mut self, field: DecalField) {
        let kind = std::mem::discriminant(&field);
        let going_on = self.decaling.editing.as_ref().is_some_and(|edit| edit.selection == self.selected_decals && edit.field == kind);
        if !going_on {
            let originals = self.selected_decal_list();
            let stroke = self.stroke();
            self.decaling.editing = Some(FieldEdit { selection: self.selected_decals.clone(), field: kind, stroke, originals });
        }
        let Some(edit) = self.decaling.editing.as_ref() else { return };
        let ops: Vec<Value> = edit.originals.iter().map(|(grid, decal)| decal_update_op(*grid, &field.applied(decal))).collect();
        let (stroke, label) = (edit.stroke, if ops.len() == 1 { "Edit decal" } else { "Edit decals" });
        self.edit(ops, label, Some(stroke), true);
    }

    /// The panel's gesture on a field has ended. The next change is a step of its own.
    pub fn end_decal_edit(&mut self) {
        self.decaling.editing = None;
    }

    /// Selects every decal of the prototype the selected decals share, on their grids. Nothing
    /// changes where they are of more than one.
    pub fn select_decals_like(&mut self) {
        let decals = self.selected_decal_list();
        let (Some(prototype), Some(doc)) = (shared(&decals, |(_, decal)| decal.prototype.clone()), self.doc.as_ref()) else { return };
        let mut grids: Vec<i32> = decals.iter().map(|(grid, _)| *grid).collect();
        grids.sort_unstable();
        grids.dedup();
        let mut found: Vec<(i32, i32)> =
            grids.iter().filter_map(|&grid| doc.grid(grid)).flat_map(|grid| grid.decals.values().filter(|decal| decal.prototype == prototype).map(|decal| (grid.uid, decal.id))).collect();
        found.sort_unstable();
        self.selection.clear();
        self.selected_decals = found;
    }

    /// Selects every decal with the colour the selected decals share, on their grids, as the
    /// web editor's Select All does. Nothing changes where their colours differ.
    pub fn select_decals_coloured(&mut self) {
        let decals = self.selected_decal_list();
        let Some(color) = shared(&decals, |(_, decal)| decal.color.clone()) else { return };
        let found = self.decals_coloured(&decals, &color).into_iter().map(|(grid, decal)| (grid, decal.id)).collect();
        self.selection.clear();
        self.selected_decals = found;
    }

    /// Gives every decal with the colour the selected decals share, on their grids, another
    /// colour, as one edit, as the web editor's Recolor All does. None takes the colour away.
    pub fn recolor_decals(&mut self, to: Option<String>) {
        let decals = self.selected_decal_list();
        let Some(color) = shared(&decals, |(_, decal)| decal.color.clone()) else { return };
        let ops = self.decals_coloured(&decals, &color).iter().map(|(grid, decal)| decal_update_op(*grid, &Decal { color: to.clone(), ..decal.clone() })).collect();
        self.edit(ops, "Recolour decals", None, false);
    }

    /// The outlines of the selected decals, turned as they are.
    pub(super) fn decal_selection_outlines(&self, outlines: &mut Vec<Outline>) {
        let Some(doc) = self.doc.as_ref() else { return };
        for &(grid, id) in &self.selected_decals {
            if let Some((grid, decal)) = doc.grid(grid).and_then(|grid| grid.decals.get(&id).map(|decal| (grid, decal))) {
                outlines.push(Outline { corners: decal_corners(grid, (decal.x, decal.y), decal.angle), style: OutlineStyle::Decal });
            }
        }
    }
}

/// A decal as it should be, for the kernel's `decal.update` op, which states every field.
pub(super) fn decal_update_op(grid: i32, decal: &Decal) -> Value {
    json!({
        "kind": "decal.update", "grid": grid, "uid": decal.id, "x": decal.x, "y": decal.y, "prototype": decal.prototype,
        "color": decal.color, "rotation": decal.angle, "zIndex": decal.z_index, "cleanable": decal.cleanable,
    })
}

/// The cell a decal is on, as the web editor counts it: the one its lower left corner is in.
pub(super) fn decal_cell(decal: &Decal) -> (i32, i32) {
    (decal.x.floor() as i32, decal.y.floor() as i32)
}

/// The four corners on the map of a decal one cell wide with its lower left corner at `corner`
/// of a grid, turned by `angle` about its middle: bottom left, bottom right, top right, top
/// left of its own axes, as a tile's are.
pub(super) fn decal_corners(grid: &Grid, corner: (f32, f32), angle: f64) -> [(f32, f32); 4] {
    let (sin, cos) = (angle as f32).sin_cos();
    let middle = (corner.0 + 0.5, corner.1 + 0.5);
    [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)].map(|(x, y)| grid.to_world((middle.0 + x * cos - y * sin, middle.1 + x * sin + y * cos)))
}
