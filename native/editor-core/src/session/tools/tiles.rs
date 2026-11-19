//! The tile tools: setting cells of a grid, the brush, and the tiles drawn ahead of the engine's answer.

use super::*;

/// A fill larger than this is refused rather than sent.
const FILL_LIMIT: usize = 50_000;

/// A shape of entities of more cells than this is refused rather than sent.
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

/// What a drag puts down, read from the palette once, at the press. A change of the palette
/// during the drag changes nothing of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::session) enum Brush {
    /// A tile's id. The eraser's is space.
    Tile(String),
    /// An index into [`Session::prototypes`].
    Entity(usize),
    /// A decal prototype's id. It is put down with the placement settings of the moment.
    Decal(String),
}

/// A stroke of the paint or erase brush on one grid.
pub(in crate::session) struct StrokeDrag {
    stroke: u64,
    grid: i32,
    last: (i32, i32),
    brush: Brush,
    /// Whether it is the eraser's. With an entity it deletes that prototype.
    erase: bool,
    /// The cells it has put an entity on or erased entities from, since the mirror is behind
    /// the engine.
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
        let Some(Brush::Entity(index)) = self.brush else { return None };
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
    /// unturned, in the form the place tool sends. A cell that already holds an entity of the
    /// prototype or is in `placed` is skipped. So is a cell with no tile, unless the prototype
    /// needs a floor and laying one is switched on (`floor_for`): then the tile is laid there
    /// first, in the same edit. The cells it puts one on go into `placed`.
    fn put_entities(&mut self, grid_uid: i32, cells: &[(i32, i32)], prototype: usize, label: &'static str, stroke: Option<u64>, placed: &mut HashSet<(i32, i32)>) {
        let Some(prototype) = self.prototypes.get(prototype) else { return };
        let (id, floor) = (prototype.id.clone(), self.floor_for(prototype));
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
            spawns.push(json!({ "kind": "spawn", "prototype": id, "parent": grid_uid, "x": x, "y": y, "rotation": 0.0 }));
        }
        // The tiles go first, so that no entity is put on a cell with no tile.
        ops.extend(spawns);
        let seq = self.edit(ops, label, stroke, false);
        let sprite = floor.and_then(|tile| self.tile_sprites.get(&tile).cloned().flatten());
        self.painting.lay_ahead(seq, laid, sprite);
    }

    /// Asks for the entities of a prototype on these cells of a grid to be deleted, and leaves
    /// the tiles. Only children of the grid that are inside nothing.
    fn delete_entities_on(&mut self, grid_uid: i32, cells: &[(i32, i32)], prototype: usize, label: &'static str, stroke: Option<u64>) {
        let Some(id) = self.prototypes.get(prototype).map(|prototype| prototype.id.as_str()) else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        let cells: HashSet<(i32, i32)> = cells.iter().copied().collect();
        let mut uids: Vec<i32> = doc
            .entities
            .values()
            .filter(|entity| entity.parent == grid_uid && !entity.contained && entity.prototype == id && cells.contains(&(entity.x.floor() as i32, entity.y.floor() as i32)))
            .map(|entity| entity.uid)
            .collect();
        uids.sort_unstable();
        let ops = uids.into_iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        self.edit(ops, label, stroke, false);
    }

    /// Puts the brush down on cells of a grid as one edit with no stroke, for a shape. A
    /// shape of entities of more than `SHAPE_LIMIT` cells is refused, with a line on the status
    /// bar that names the shape.
    pub(super) fn put_shape(&mut self, grid: i32, cells: &[(i32, i32)], brush: &Brush, labels: (&'static str, &'static str, &'static str), shape: &str) {
        match brush {
            Brush::Tile(tile) => self.set_cells(grid, cells, tile, labels.0, None),
            Brush::Entity(_) | Brush::Decal(_) if cells.len() > SHAPE_LIMIT => self.set_status(format!("Not placed: the {shape} has more than {} cells", thousands(SHAPE_LIMIT))),
            Brush::Entity(prototype) => self.put_entities(grid, cells, *prototype, labels.1, None, &mut HashSet::new()),
            Brush::Decal(id) => self.put_decals(grid, cells, id, labels.2, None, &mut HashSet::new()),
        }
    }

    /// The grid and cell a tile tool works on at a map position.
    pub(super) fn cell_at(&self, world: (f32, f32)) -> Option<(i32, (i32, i32))> {
        let grid = self.doc.as_ref()?.grid_at(world)?;
        Some((grid.uid, grid.cell_at(world)))
    }

    /// Chooses the tile the tile tools put down, and goes to the paint tool unless a tool that
    /// puts tiles down is in hand already.
    pub fn choose_tile(&mut self, id: &str) {
        self.palette = Palette::Tile(id.to_owned());
        if !self.tool.puts_tiles() {
            self.set_tool(Tool::Paint);
        }
        self.hint_answered();
    }

    /// What the tool in hand puts down for a press, from the palette. An entity or a decal is
    /// the brush of a tool that works with one (`Tool::brushes_entities`). The eraser's brush is
    /// otherwise space, whatever is chosen. Where the tool cannot use what is chosen a hint on
    /// the status line asks for a tile, since nothing else comes of the press.
    pub(super) fn brush_for_press(&mut self) -> Option<Brush> {
        let brush = match &self.palette {
            Palette::Entity(index) if self.tool.brushes_entities() => Some(Brush::Entity(*index)),
            Palette::Decal(id) if self.tool.brushes_entities() => Some(Brush::Decal(id.clone())),
            _ if self.tool == Tool::Erase => Some(Brush::Tile(SPACE.to_owned())),
            Palette::Tile(id) => Some(Brush::Tile(id.clone())),
            Palette::None | Palette::Entity(_) | Palette::Decal(_) => None,
        };
        if brush.is_none() {
            self.hint = Some(Hint::Tile);
        }
        brush
    }

    /// Puts a stroke's brush down on cells it has reached, as a part of its stroke.
    fn stroke_cells(&mut self, grid: i32, cells: &[(i32, i32)], stroke: u64, brush: &Brush, erase: bool, done: &mut HashSet<(i32, i32)>) {
        match brush {
            Brush::Tile(tile) => self.set_cells(grid, cells, tile, if tile == SPACE { "Erase tiles" } else { "Paint tiles" }, Some(stroke)),
            Brush::Entity(prototype) if erase => {
                // A cell crossed again is not asked about twice.
                let cells: Vec<(i32, i32)> = cells.iter().copied().filter(|&cell| done.insert(cell)).collect();
                self.delete_entities_on(grid, &cells, *prototype, "Erase entities", Some(stroke));
            }
            Brush::Entity(prototype) => self.put_entities(grid, cells, *prototype, "Paint entities", Some(stroke), done),
            Brush::Decal(id) if erase => {
                let cells: Vec<(i32, i32)> = cells.iter().copied().filter(|&cell| done.insert(cell)).collect();
                self.delete_decals_on(grid, &cells, id, "Erase decals", Some(stroke));
            }
            Brush::Decal(id) => self.put_decals(grid, cells, id, "Paint decals", Some(stroke), done),
        }
    }

    /// The paint or erase tool was pressed: puts the brush down on the cell under it and
    /// begins a stroke. A decal placed freely (`decal_free`) is one decal at the cursor, and
    /// begins no stroke.
    pub(super) fn stroke_down(&mut self, world: (f32, f32)) {
        let erase = self.tool == Tool::Erase;
        let brush = self.brush_for_press();
        if let (Some(Brush::Decal(id)), false, true) = (&brush, erase, self.decal_free()) {
            self.place_decal_freely(world, id);
            return;
        }
        if let (Some(brush), Some((grid, cell))) = (brush, self.cell_at(world)) {
            let stroke = self.stroke();
            let mut done = HashSet::new();
            self.stroke_cells(grid, &[cell], stroke, &brush, erase, &mut done);
            self.drag = Drag::Stroke(StrokeDrag { stroke, grid, last: cell, brush, erase, done });
        }
    }

    /// The fill tool was pressed: sets every cell of the area under it, as one edit.
    pub(super) fn fill_down(&mut self, world: (f32, f32)) {
        // Fill is a tile tool alone, so `brush_for_press` gives it a tile or nothing.
        let Some(Brush::Tile(tile)) = self.brush_for_press() else { return };
        if let Some((grid, cell)) = self.cell_at(world) {
            match self.doc.as_ref().and_then(|doc| doc.fill_cells(grid, cell, FILL_LIMIT)) {
                Some(cells) => self.set_cells(grid, &cells, &tile, "Fill tiles", None),
                None => self.set_status(format!("Not filled: the area has more than {} tiles", thousands(FILL_LIMIT))),
            }
        }
    }

    /// The rectangle tool was pressed: begins a rectangle at the cell under it.
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
        let StrokeDrag { stroke, grid, last, brush, erase, mut done } = drag;
        let cell = self.doc.as_ref().and_then(|doc| doc.grid(grid)).map_or(last, |grid| grid.cell_at(world));
        if cell != last {
            let cells = cells_between(last, cell);
            self.stroke_cells(grid, &cells[1..], stroke, &brush, erase, &mut done);
        }
        Drag::Stroke(StrokeDrag { stroke, grid, last: cell, brush, erase, done })
    }

    /// A rectangle was dragged on: its far corner follows the cursor.
    pub(super) fn rectangle_dragged(&self, world: (f32, f32), drag: RectangleDrag) -> Drag {
        let RectangleDrag { grid, from, to, brush } = drag;
        let to = self.doc.as_ref().and_then(|doc| doc.grid(grid)).map_or(to, |grid| grid.cell_at(world));
        Drag::Rectangle(RectangleDrag { grid, from, to, brush })
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
