//! The tile tools: setting cells of a grid, the brush, and the tiles drawn ahead of the engine's answer.

use super::*;
use crate::renderer::group_of;
use crate::toolbox::Target;

/// A fill larger than this is refused rather than sent.
const FILL_LIMIT: usize = 50_000;

/// A shape or a fill of entities or decals of more cells than this is refused rather than sent.
pub(super) const SHAPE_LIMIT: usize = 10_000;

/// What the tile tools keep from one drag to the next: the tiles drawn ahead of the engine.
#[derive(Default)]
pub(in crate::session) struct Painting {
    pending: Vec<PendingTile>,
    /// The latest request the engine has answered.
    answered_seq: u64,
}

/// A tile that has been asked for and not answered yet, drawn at once so the brush never lags.
struct PendingTile {
    seq: u64,
    corners: [(f32, f32); 4],
    /// The tile's image. None draws the empty canvas (the tile was erased).
    sprite: Option<String>,
}

impl Painting {
    /// The engine has answered a request: the tiles drawn ahead of that answer have done
    /// their job.
    fn answered(&mut self, seq: u64) {
        self.answered_seq = self.answered_seq.max(seq);
        let answered = self.answered_seq;
        self.pending.retain(|tile| tile.seq > answered);
    }

    /// Draws tiles ahead of the engine's answer to the request of this number: the squares
    /// of cells that are to become a tile. A tile with no picture known is not drawn ahead.
    pub(super) fn lay_ahead(&mut self, seq: u64, corners: Vec<[(f32, f32); 4]>, sprite: Option<String>) {
        if let Some(sprite) = sprite {
            self.pending.extend(corners.into_iter().map(|corners| PendingTile { seq, corners, sprite: Some(sprite.clone()) }));
        }
    }

    /// Forgets the tiles drawn ahead, as when another map is opened.
    pub(in crate::session) fn clear(&mut self) {
        self.pending.clear();
    }

    /// The tiles drawn ahead, as rectangles for the renderer. A tile's image is given a texture
    /// of the editor's own in `lists`.
    pub(super) fn quads(&self, lists: &mut DrawLists) -> Vec<f32> {
        let mut quads = Vec::with_capacity(self.pending.len() * FLOATS_PER_QUAD);
        for tile in &self.pending {
            let (texture, colour, source) = match &tile.sprite {
                Some(sprite) => (lists.local_texture(sprite, 0, 32, 32), [1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 32.0, 32.0]),
                None => {
                    let (r, g, b) = crate::shell::colours::CANVAS;
                    (WHITE_TEXTURE, [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0], WHITE_SOURCE)
                }
            };
            for corner in tile.corners {
                quads.extend_from_slice(&[corner.0, corner.1]);
            }
            quads.extend_from_slice(&colour);
            quads.push(texture as f32);
            quads.extend_from_slice(&source);
        }
        quads
    }
}

/// What a drag puts down or takes away, read from the palette and the options once, at the
/// press. A change of either during the drag changes nothing of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::session) enum Brush {
    /// A tile's id.
    Tile(String),
    /// An index into [`Session::prototypes`].
    Entity(usize),
    /// A decal prototype's id. It is put down with the placement settings of the moment.
    Decal(String),
    /// The eraser's.
    Erase(Erasing),
}

/// What the eraser takes away: everything of its target that can be seen, or with Only on the
/// tiles, entities or decals of one id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::session) struct Erasing {
    pub(super) target: Target,
    /// The id Only keeps it to: a tile's, a prototype's or a decal's.
    pub(super) only: Option<String>,
}

/// A stroke of the paint or erase brush on one grid.
pub(in crate::session) struct StrokeDrag {
    stroke: u64,
    grid: i32,
    last: (i32, i32),
    brush: Brush,
    /// The cells it has put an entity or a decal on, or erased entities or decals from, since
    /// the mirror is behind the engine.
    done: HashSet<(i32, i32)>,
}

/// A rectangle being dragged out on one grid.
pub(in crate::session) struct RectangleDrag {
    grid: i32,
    from: (i32, i32),
    to: (i32, i32),
    /// None when nothing it can put down was chosen at the press. It sets nothing then.
    brush: Option<Brush>,
}

impl RectangleDrag {
    /// Its grid, its cells and the entity it puts down, if it puts one down.
    pub(super) fn entity_cells(&self) -> Option<(i32, Vec<(i32, i32)>, usize)> {
        let Some(Brush::Entity(index)) = &self.brush else { return None };
        let index = *index;
        Some((self.grid, rectangle_cells(self.from, self.to), index))
    }
}

/// The cells of a rectangle from one corner to the other, column by column.
pub(super) fn rectangle_cells(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let mut cells = Vec::new();
    for x in from.0.min(to.0)..=from.0.max(to.0) {
        for y in from.1.min(to.1)..=from.1.max(to.1) {
            cells.push((x, y));
        }
    }
    cells
}

/// The cells of a grid that hold an entity of a prototype: a child of the grid that is
/// inside nothing.
pub(super) fn cells_holding(doc: &Doc, grid: i32, prototype: &str) -> HashSet<(i32, i32)> {
    doc.entities
        .values()
        .filter(|entity| entity.parent == grid && !entity.contained && entity.prototype == prototype)
        .map(|entity| (entity.x.floor() as i32, entity.y.floor() as i32))
        .collect()
}

impl Session {
    /// The engine has answered a request, which the tile tools hear of for what they drew ahead.
    pub(in crate::session) fn answered(&mut self, seq: u64) {
        self.painting.answered(seq);
    }

    /// Asks for cells of a grid to become a tile, and shows them at once.
    fn set_cells(&mut self, grid_uid: i32, cells: &[(i32, i32)], tile: &str, label: &'static str, stroke: Option<u64>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(grid) = doc.grids.iter().find(|grid| grid.uid == grid_uid) else { return };
        doc.active_grid = grid_uid;
        self.grid = (grid.uid, grid.x, grid.y);

        let mut ops = Vec::with_capacity(cells.len());
        let mut corners = Vec::with_capacity(cells.len());
        for &cell in cells {
            let now = grid.tiles.get(&cell).map_or(SPACE, |now| doc.tile_names[now.id as usize].as_str());
            if now != tile {
                ops.push(json!({ "kind": "tile", "grid": grid_uid, "x": cell.0, "y": cell.1, "tile": tile }));
                corners.push(grid.cells_outline(cell, cell));
            }
        }
        if ops.is_empty() {
            return;
        }
        let sprite = if tile == SPACE { None } else { self.tile_sprites.get(tile).cloned().flatten() };
        let seq = self.edit(ops, label, stroke, false);
        if tile == SPACE || sprite.is_some() {
            self.painting.pending.extend(corners.into_iter().map(|corners| PendingTile { seq, corners, sprite: sprite.clone() }));
        }
    }

    /// Asks for an entity of a prototype in the middle of each of these cells of a grid,
    /// turned as the place tool turns it (R), in the form the place tool sends. A cell that already holds an entity of the
    /// prototype or is in `placed` is skipped. So is a cell with no tile, unless the prototype
    /// needs a floor and laying one is switched on (`floor_for`): then the tile is laid there
    /// first, in the same edit. The cells it puts one on go into `placed`.
    fn put_entities(&mut self, grid_uid: i32, cells: &[(i32, i32)], prototype: usize, label: &'static str, stroke: Option<u64>, placed: &mut HashSet<(i32, i32)>) {
        let Some(prototype) = self.prototypes.get(prototype) else { return };
        let (id, floor, rotation) = (prototype.id.clone(), self.floor_for(prototype), self.placing.rotation());
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(grid) = doc.grids.iter().find(|grid| grid.uid == grid_uid) else { return };
        doc.active_grid = grid_uid;
        self.grid = (grid.uid, grid.x, grid.y);

        let holding = cells_holding(doc, grid_uid, &id);
        let (mut ops, mut spawns, mut laid) = (Vec::new(), Vec::new(), Vec::new());
        for &cell in cells {
            let bare = doc.tile_at(grid_uid, cell) == SPACE;
            if (bare && floor.is_none()) || holding.contains(&cell) || !placed.insert(cell) {
                continue;
            }
            if let (true, Some(tile)) = (bare, &floor) {
                ops.push(json!({ "kind": "tile", "grid": grid_uid, "x": cell.0, "y": cell.1, "tile": tile }));
                laid.push(grid.cells_outline(cell, cell));
            }
            let (x, y) = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
            spawns.push(json!({ "kind": "spawn", "prototype": id, "parent": grid_uid, "x": x, "y": y, "rotation": rotation }));
        }
        // The tiles go first, so that no entity is put on a cell with no tile.
        ops.extend(spawns);
        let seq = self.edit(ops, label, stroke, false);
        let sprite = floor.and_then(|tile| self.tile_sprites.get(&tile).cloned().flatten());
        self.painting.lay_ahead(seq, laid, sprite);
    }

    /// Asks for the entities on these cells of a grid to be deleted, and leaves the tiles: the
    /// children of the grid that are inside nothing, so never the grid, the map or what stands
    /// on the map off the grids. What they hold goes with them, as with the select tool's
    /// Delete. Without `only` it is every entity that can be seen (`erasable`), and with it
    /// those of that prototype that are not on a hidden layer.
    fn delete_entities_on(&mut self, grid_uid: i32, cells: &[(i32, i32)], only: Option<&str>, stroke: Option<u64>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let cells: HashSet<(i32, i32)> = cells.iter().copied().collect();
        let mut uids: Vec<i32> = doc
            .entities
            .values()
            .filter(|entity| entity.parent == grid_uid && !entity.contained && cells.contains(&(entity.x.floor() as i32, entity.y.floor() as i32)))
            .filter(|entity| only.is_none_or(|id| entity.prototype == id) && self.erasable(entity.uid, only.is_some()))
            .map(|entity| entity.uid)
            .collect();
        uids.sort_unstable();
        let ops = uids.into_iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        self.edit(ops, "Erase entities", stroke, false);
    }

    /// Whether the eraser may take an entity: it is drawn on a layer that is shown, as a box of
    /// the select tool finds it. One the map does not draw (it has no picture, or lies under a
    /// floor with T-Ray off) is taken only when Only names its prototype (`named`). One on a
    /// hidden layer, or with Show entities off, is never taken.
    fn erasable(&self, uid: i32, named: bool) -> bool {
        let item = self.document.and_then(|id| self.lists.documents.get(&id)).and_then(|document| document.items.get(&uid));
        match item {
            Some(item) if self.layers.hidden_mask() & (1 << group_of(item.kind, item.depth)) != 0 => false,
            Some(item) if !item.quads.is_empty() => true,
            _ => named && self.layers.entities,
        }
    }

    /// Takes away what the eraser takes on these cells of a grid, as one edit or a part of its
    /// stroke: tiles, entities or decals (`Erasing`).
    fn erase_cells(&mut self, grid: i32, cells: &[(i32, i32)], erasing: &Erasing, stroke: Option<u64>) {
        let only = erasing.only.as_deref();
        match erasing.target {
            Target::Tiles => {
                let Some(doc) = self.doc.as_ref() else { return };
                let cells: Vec<(i32, i32)> = cells.iter().copied().filter(|&cell| only.is_none_or(|id| doc.tile_at(grid, cell) == id)).collect();
                self.set_cells(grid, &cells, SPACE, "Erase tiles", stroke);
            }
            Target::Entities => self.delete_entities_on(grid, cells, only, stroke),
            Target::Decals => self.delete_decals_on(grid, cells, only, stroke),
        }
    }

    /// Puts the brush down on cells of a grid as one edit with no stroke, for a shape or a
    /// fill, or takes away what the eraser takes there. A cell that holds what the brush puts
    /// down already is skipped. A shape of entities or decals of more than `SHAPE_LIMIT` cells
    /// is refused, with a line on the status bar that names the shape, and so is one the
    /// eraser would take them from.
    pub(super) fn put_shape(&mut self, grid: i32, cells: &[(i32, i32)], brush: &Brush, labels: (&'static str, &'static str, &'static str), shape: &str) {
        let many = cells.len() > SHAPE_LIMIT;
        match brush {
            Brush::Tile(tile) => self.set_cells(grid, cells, tile, labels.0, None),
            Brush::Erase(erasing) if erasing.target == Target::Tiles => self.erase_cells(grid, cells, erasing, None),
            Brush::Entity(_) | Brush::Decal(_) | Brush::Erase(_) if many => {
                let not = if matches!(brush, Brush::Erase(_)) { "Not erased" } else { "Not placed" };
                self.set_status(format!("{not}: the {shape} has more than {} cells", thousands(SHAPE_LIMIT)));
            }
            Brush::Erase(erasing) => self.erase_cells(grid, cells, erasing, None),
            Brush::Entity(prototype) => self.put_entities(grid, cells, *prototype, labels.1, None, &mut HashSet::new()),
            Brush::Decal(id) => {
                let mut held = self.decals_held(grid, id);
                self.put_decals(grid, cells, id, labels.2, None, &mut held);
            }
        }
    }

    /// The grid and cell a tile tool works on at a map position.
    pub(super) fn cell_at(&self, world: (f32, f32)) -> Option<(i32, (i32, i32))> {
        let grid = self.doc.as_ref()?.grid_at(world)?;
        Some((grid.uid, grid.cell_at(world)))
    }

    /// Chooses the tile the tile tools put down, which makes Tiles the target, and goes to Paint
    /// in its shape unless a tool that puts tiles down is in hand already.
    pub fn choose_tile(&mut self, id: &str) {
        self.palette = Palette::Tile(id.to_owned());
        self.chosen();
        if !self.tool.puts_tiles() {
            self.take_bar_tool(BarTool::Paint);
        }
        self.hint_answered();
    }

    /// What the tool in hand puts down or takes away for a press. The eraser's brush is its
    /// target, kept to the kind in hand with Only on (`erase_only_id`). Any other brush is what
    /// the palette holds, an entity or a decal only for a tool that works with one
    /// (`Tool::brushes_entities`). Where the tool has nothing it can use, a hint on the status
    /// line asks for something of its target, since nothing else comes of the press.
    pub(super) fn brush_for_press(&mut self) -> Option<Brush> {
        let target = self.target();
        if self.tool == Tool::Erase {
            if !self.erase_only() {
                return Some(Brush::Erase(Erasing { target, only: None }));
            }
            let only = self.erase_only_id();
            if only.is_none() {
                self.hint = Some(Hint::EraseOnly(target));
            }
            return only.map(|only| Brush::Erase(Erasing { target, only: Some(only) }));
        }
        let brush = match &self.palette {
            Palette::Entity(index) if self.tool.brushes_entities() => Some(Brush::Entity(*index)),
            Palette::Decal(id) if self.tool.brushes_entities() => Some(Brush::Decal(id.clone())),
            Palette::Tile(id) => Some(Brush::Tile(id.clone())),
            Palette::None | Palette::Entity(_) | Palette::Decal(_) => None,
        };
        if brush.is_none() {
            self.hint = Some(match target {
                Target::Tiles => Hint::Tile,
                Target::Entities => Hint::Entity,
                Target::Decals => Hint::Decal,
            });
        }
        brush
    }

    /// Puts a stroke's brush down on cells it has reached, as a part of its stroke. Past the
    /// cell pressed, a decal is not put on a cell that holds one of it already, as an entity is
    /// not.
    fn stroke_cells(&mut self, grid: i32, cells: &[(i32, i32)], stroke: u64, brush: &Brush, done: &mut HashSet<(i32, i32)>, pressed: bool) {
        match brush {
            Brush::Tile(tile) => self.set_cells(grid, cells, tile, "Paint tiles", Some(stroke)),
            Brush::Erase(erasing) if erasing.target == Target::Tiles => self.erase_cells(grid, cells, erasing, Some(stroke)),
            Brush::Erase(erasing) => {
                // A cell crossed again is not asked about twice.
                let cells: Vec<(i32, i32)> = cells.iter().copied().filter(|&cell| done.insert(cell)).collect();
                self.erase_cells(grid, &cells, erasing, Some(stroke));
            }
            Brush::Entity(prototype) => self.put_entities(grid, cells, *prototype, "Paint entities", Some(stroke), done),
            Brush::Decal(id) => {
                if !pressed {
                    done.extend(self.decals_held(grid, id));
                }
                self.put_decals(grid, cells, id, "Paint decals", Some(stroke), done);
            }
        }
    }

    /// The paint or erase tool was pressed: puts the brush down on the cell under it and
    /// begins a stroke. A decal placed freely (`decal_free`) is one decal at the cursor, and
    /// begins no stroke.
    pub(super) fn stroke_down(&mut self, world: (f32, f32)) {
        let brush = self.brush_for_press();
        if let (Some(Brush::Decal(id)), true) = (&brush, self.decal_free()) {
            self.place_decal_freely(world, id);
            return;
        }
        if let (Some(brush), Some((grid, cell))) = (brush, self.cell_at(world)) {
            let stroke = self.stroke();
            let mut done = HashSet::new();
            self.stroke_cells(grid, &[cell], stroke, &brush, &mut done, true);
            self.drag = Drag::Stroke(StrokeDrag { stroke, grid, last: cell, brush, done });
        }
    }

    /// The eraser was pressed: a stroke at a point, or the shape it has in the toolbar.
    pub(super) fn erase_down(&mut self, world: (f32, f32)) {
        match self.tooling.toolbox.options.erase_shape {
            crate::toolbox::Shape::Point => self.stroke_down(world),
            crate::toolbox::Shape::Rectangle => self.rectangle_down(world),
            crate::toolbox::Shape::Line => self.shape_down(world, shapes::Shape::Line),
            crate::toolbox::Shape::Circle => self.shape_down(world, shapes::Shape::Circle),
        }
    }

    /// The fill tool was pressed: sets every cell of the area of the same tile under it, as one
    /// edit. With an entity or a decal it puts one on each cell of the area that does not hold
    /// one already, up to `SHAPE_LIMIT` cells, and only on a floor, since an area of space has
    /// no edge of its own.
    pub(super) fn fill_down(&mut self, world: (f32, f32)) {
        let Some(brush) = self.brush_for_press() else { return };
        let Some((grid, cell)) = self.cell_at(world) else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        if let Brush::Tile(tile) = &brush {
            match doc.fill_cells(grid, cell, FILL_LIMIT) {
                Some(cells) => self.set_cells(grid, &cells, tile, "Fill tiles", None),
                None => self.set_status(format!("Not filled: the area has more than {} tiles", thousands(FILL_LIMIT))),
            }
            return;
        }
        if doc.tile_at(grid, cell) == SPACE {
            self.set_status("Not filled: entities and decals fill a floor, not space");
            return;
        }
        match doc.fill_cells(grid, cell, SHAPE_LIMIT) {
            Some(cells) => self.put_shape(grid, &cells, &brush, ("Fill tiles", "Fill entities", "Fill decals"), "area"),
            None => self.set_status(format!("Not filled: the area has more than {} cells", thousands(SHAPE_LIMIT))),
        }
    }

    /// The rectangle tool, or the eraser in that shape, was pressed: begins a rectangle at the
    /// cell under it.
    pub(super) fn rectangle_down(&mut self, world: (f32, f32)) {
        // With nothing it can put down chosen the rectangle is dragged out all the same, and
        // sets nothing when it is let go.
        let brush = self.brush_for_press();
        if let Some((grid, cell)) = self.cell_at(world) {
            self.drag = Drag::Rectangle(RectangleDrag { grid, from: cell, to: cell, brush });
        }
    }

    /// A stroke was dragged on: puts the brush down on every cell crossed since the last one
    /// it reached.
    pub(super) fn stroke_dragged(&mut self, world: (f32, f32), drag: StrokeDrag) -> Drag {
        let StrokeDrag { stroke, grid, last, brush, mut done } = drag;
        let cell = self.doc.as_ref().and_then(|doc| doc.grid(grid)).map_or(last, |grid| grid.cell_at(world));
        if cell != last {
            let cells = cells_between(last, cell);
            self.stroke_cells(grid, &cells[1..], stroke, &brush, &mut done, false);
        }
        Drag::Stroke(StrokeDrag { stroke, grid, last: cell, brush, done })
    }

    /// A rectangle was dragged on: its far corner follows the cursor.
    pub(super) fn rectangle_dragged(&self, world: (f32, f32), drag: RectangleDrag) -> Drag {
        let to = self.doc.as_ref().and_then(|doc| doc.grid(drag.grid)).map_or(drag.to, |grid| grid.cell_at(world));
        Drag::Rectangle(RectangleDrag { to, ..drag })
    }

    /// A rectangle was let go: puts the brush it was pressed with down on every cell from
    /// corner to corner, as one edit.
    pub(super) fn rectangle_up(&mut self, drag: RectangleDrag) {
        let RectangleDrag { grid, from, to, brush } = drag;
        let Some(brush) = brush else { return };
        let cells = rectangle_cells(from, to);
        self.put_shape(grid, &cells, &brush, ("Rectangle of tiles", "Rectangle of entities", "Rectangle of decals"), "rectangle");
    }

    /// The outline of a rectangle being dragged out.
    pub(super) fn rectangle_outlines(&self, drag: &RectangleDrag, outlines: &mut Vec<Outline>) {
        let RectangleDrag { grid, from, to, .. } = drag;
        if let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(*grid)) {
            outlines.push(Outline { corners: grid.cells_outline(*from, *to), style: OutlineStyle::Drag });
        }
    }

    /// The size of a rectangle being dragged out, over the middle of its top edge.
    pub(super) fn rectangle_label(&self, drag: &RectangleDrag) -> Option<((f32, f32), String)> {
        let RectangleDrag { grid, from, to, .. } = drag;
        let grid = self.doc.as_ref()?.grid(*grid)?;
        let (width, height) = ((from.0 - to.0).abs() + 1, (from.1 - to.1).abs() + 1);
        let (left, top) = (from.0.min(to.0) as f32, from.1.max(to.1) as f32 + 1.0);
        Some((grid.to_world((left + width as f32 / 2.0, top)), format!("{width}x{height}")))
    }

    /// The ghost of the chosen tile, half seen through, on the cell under the cursor, for
    /// `overlay_quads`: where a press of a tool that puts tiles down would begin. Not while a
    /// drag is live, since the tiles it asked for are drawn ahead, and not for a tile whose
    /// picture is not known. The brush asks for no variant, so the ghost is the first, as the
    /// tiles drawn ahead are.
    pub(super) fn tile_ghost(&mut self, under: &mut Vec<f32>) {
        let shows = self.tool.puts_tiles() && matches!(self.drag, Drag::None);
        let (true, Some(world), Some(tile)) = (shows, self.ghost_hover(), self.chosen_tile()) else { return };
        let Some(sprite) = self.tile_sprites.get(tile).cloned().flatten() else { return };
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid_at(world)) else { return };
        let cell = grid.cell_at(world);
        let corners = grid.cells_outline(cell, cell);
        let texture = self.lists.local_texture(&sprite, 0, 32, 32);
        for corner in corners {
            under.extend_from_slice(&[corner.0, corner.1]);
        }
        under.extend_from_slice(&[1.0, 1.0, 1.0, 0.5]);
        under.push(texture as f32);
        under.extend_from_slice(&[0.0, 0.0, 32.0, 32.0]);
    }

    /// The outline of the cell under the cursor, for every tile tool.
    pub(super) fn tile_hover_outlines(&self, world: (f32, f32), doc: &Doc, outlines: &mut Vec<Outline>) {
        if let Some(grid) = doc.grid_at(world) {
            let cell = grid.cell_at(world);
            outlines.push(Outline { corners: grid.cells_outline(cell, cell), style: OutlineStyle::Hover });
        }
    }
}
