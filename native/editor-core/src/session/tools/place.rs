//! The place tool: choosing a prototype, how it looks before it is placed, and placing it.

use super::*;

/// Placing one entity per cell crossed.
pub(in crate::session) struct PlaceDrag {
    stroke: u64,
    placed: HashSet<(i32, i32, i32)>,
    /// Whether it lays a row, or places only where it was pressed.
    row: bool,
}

/// The web editor's green for the ghost's label, and its light blue with Shift.
const GHOST_GREEN: [u8; 4] = [0, 255, 100, 204];
const GHOST_BLUE: [u8; 4] = [100, 200, 255, 204];

/// The longest name, in letters, that goes on a second line under the ghost's id.
const NAME_FITS: usize = 32;

/// What the interface writes and draws by the ghost of the chosen entity.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacePreview {
    /// Where its middle would be, on the map.
    pub at: (f32, f32),
    /// The prototype's id.
    pub id: String,
    /// While the engine has yet to send its picture, the square to dash in its place, as the
    /// web editor does: the cell, or with Shift a cell's square round the cursor.
    pub square: Option<[(f32, f32); 4]>,
    /// The text under the ghost: the id, and " (free)" after it where Shift puts it at the
    /// cursor, which it does not for a prototype anchored by default. None while a tile
    /// is 16 points wide or less on screen, as the web editor hides it.
    pub label: Option<String>,
    /// The entity's name for a second line under the label, where it is not the id and is
    /// short enough.
    pub name: Option<String>,
    /// The middle of the lowest edge of the ghost and its cell, on the map. The label hangs under it.
    pub below: (f32, f32),
    /// The colour of the label, the square and the arrow, as RGBA.
    pub colour: [u8; 4],
    /// The way it would face on the map, as a direction one long, if it is turned from the
    /// way it faces by default.
    pub facing: Option<(f32, f32)>,
}

/// Where a press of the tool in hand would put the chosen entity, for its ghost.
struct GhostTarget {
    index: usize,
    /// Its middle, on the map.
    at: (f32, f32),
    /// The square of a cell it goes in, on the map.
    square: [(f32, f32); 4],
    /// Which of the engine's four pictures, and how far to turn that one on the map.
    turns: i32,
    angle: f32,
    /// How it is turned on the map, if it is turned from the way it faces by default.
    facing: Option<f32>,
    /// Whether it goes where the cursor is exactly.
    free: bool,
}

/// What the place tool keeps from one drag to the next: how it is turned, and which
/// prototypes' looks the engine has been asked for.
#[derive(Default)]
pub(in crate::session) struct Placing {
    /// How what is placed next is turned, in radians from 0 up to a whole turn.
    rotation: f64,
    /// Which prototypes' looks have been asked for (false) or have arrived (true). The second
    /// map is the same for all four directions.
    previews: HashMap<usize, bool>,
    previews_turned: HashMap<usize, bool>,
}

impl Placing {
    /// Turns what is placed next by a step in radians.
    pub(super) fn turn(&mut self, step: f64) {
        self.rotation = (self.rotation + step).rem_euclid(std::f64::consts::TAU);
    }

    /// The engine has said how these prototypes look, from one side or from all four.
    pub(in crate::session) fn arrived(&mut self, keys: &[i32], directions: bool) {
        for &key in keys {
            self.previews.insert(key as usize, true);
            if directions {
                self.previews_turned.insert(key as usize, true);
            }
        }
    }
}

impl Session {
    /// Chooses the entity the place tool and the brushes put down, and asks the engine how the
    /// prototype looks from each side. It goes to the place tool, or takes it in hand again,
    /// unless the tool in hand keeps the choice (`Tool::keeps_entity_choice`). A wall the Room
    /// tool can build with becomes its wall (`set_room_wall`). With the Room tool in hand such
    /// a wall is that alone: the tile in hand and the tool stay.
    pub fn choose_entity(&mut self, index: usize) {
        let wall = self.prototypes.get(index).is_some_and(is_room_wall);
        if wall {
            self.set_room_wall(index);
        }
        self.hint_answered();
        if wall && self.tool == Tool::Room {
            let name = self.prototypes.get(index).map(shown_name).unwrap_or_default();
            self.set_status(format!("Room walls: {name}"));
            self.want_previews(&[index]);
            return;
        }
        self.palette = Palette::Entity(index);
        if !self.tool.keeps_entity_choice() {
            self.set_tool(Tool::Place);
        }
        if !self.placing.previews_turned.contains_key(&index) {
            self.placing.previews_turned.insert(index, false);
            if let Some(prototype) = self.prototypes.get(index) {
                self.send(ToEngine::Previews { items: vec![(index as i32, prototype.id.clone())], directions: true });
            }
        }
    }

    /// Asks the engine how these prototypes look, for those not asked about yet. The answer
    /// arrives as items of the draw list [`PREVIEWS`].
    pub fn want_previews(&mut self, indices: &[usize]) {
        let mut items = Vec::new();
        for &index in indices {
            if !self.placing.previews.contains_key(&index) {
                if let Some(prototype) = self.prototypes.get(index) {
                    self.placing.previews.insert(index, false);
                    items.push((index as i32, prototype.id.clone()));
                }
            }
        }
        if !items.is_empty() {
            self.send(ToEngine::Previews { items, directions: false });
        }
    }

    /// How a prototype looks as placed, as rectangles around the origin, if the engine has
    /// answered. `turns` is quarter turns; anything but 0 needs [`Self::choose_entity`].
    pub fn preview(&self, index: usize, turns: i32) -> Option<&[f32]> {
        let plain = self.placing.previews.get(&index) == Some(&true);
        let turned = self.placing.previews_turned.get(&index) == Some(&true);
        if !(turned || (plain && turns.rem_euclid(4) == 0)) {
            return None;
        }
        let item = self.lists.documents.get(&PREVIEWS)?.items.get(&(index as i32 * 4 + turns.rem_euclid(4)))?;
        Some(&item.quads)
    }

    /// How the place tool is turned, in quarter turns from 0 to 3, to the nearest.
    pub(super) fn place_turns(&self) -> i32 {
        ((self.placing.rotation / std::f64::consts::FRAC_PI_2).round() as i32).rem_euclid(4)
    }

    /// What is left of the place tool's turn past `place_turns`, in radians, from minus to
    /// plus an eighth of a turn.
    pub(super) fn place_turn_past_quarters(&self) -> f32 {
        let quarter = std::f64::consts::FRAC_PI_2;
        (self.placing.rotation - (self.placing.rotation / quarter).round() * quarter) as f32
    }

    /// Where the place tool would put the chosen prototype for a cursor at `world`: the
    /// parent (a grid, or the map), the position in the parent's coordinates, and on the map,
    /// and how the parent is turned. With Shift held it is the cursor, unless the prototype is
    /// anchored by default, which goes on the middle of its cell all the same. Off every
    /// grid's tiles it is the map, except for a prototype that a tile is laid under
    /// (`floor_for`), which goes on the grid the tile tools would paint.
    pub(super) fn place_target(&self, world: (f32, f32)) -> Option<(i32, (f32, f32), (f32, f32), f32)> {
        let doc = self.doc.as_ref()?;
        let prototype = self.chosen_prototype()?;
        let snaps = if self.held.shift { prototype.anchored } else { prototype.snaps() };
        let snap = |local: (f32, f32)| if snaps { (local.0.floor() + 0.5, local.1.floor() + 0.5) } else { local };
        let floored = || self.floor_for(prototype).and_then(|_| doc.grid_at(world));
        Some(match doc.grid_with_tile_at(world).or_else(floored) {
            Some(grid) => {
                let local = snap(grid.to_local(world));
                (grid.uid, local, grid.to_world(local), grid.rotation)
            }
            None => {
                let local = snap(world);
                (doc.map_uid, local, local, 0.0)
            }
        })
    }

    /// Places the chosen prototype for a cursor at `world`, unless `placed` already has its
    /// cell. On a cell of a grid with no tile it lays the tile `floor_for` gives first, in the
    /// same edit.
    fn place_at(&mut self, world: (f32, f32), stroke: u64, placed: &mut HashSet<(i32, i32, i32)>) {
        let Some((parent, local, _, _)) = self.place_target(world) else { return };
        let Some(prototype) = self.chosen_prototype() else { return };
        let cell = (local.0.floor() as i32, local.1.floor() as i32);
        // One to a cell while dragging; a click always places.
        if !placed.insert((parent, cell.0, cell.1)) {
            return;
        }
        let mut ops = Vec::new();
        let mut laid = None;
        let doc = self.doc.as_ref().expect("a target is on the open map");
        if let (Some(tile), Some(grid)) = (self.floor_for(prototype), doc.grid(parent)) {
            if doc.tile_at(parent, cell) == SPACE {
                ops.push(json!({ "kind": "tile", "grid": parent, "x": cell.0, "y": cell.1, "tile": tile }));
                laid = Some((grid.cells_outline(cell, cell), tile));
            }
        }
        ops.push(json!({ "kind": "spawn", "prototype": prototype.id, "parent": parent, "x": local.0, "y": local.1, "rotation": self.placing.rotation }));
        self.last_put = Some(format!("placed {} at ({:.3}, {:.3}) of {parent}, {} held", prototype.id, local.0, local.1, self.held.words()));
        let seq = self.edit(ops, "Place entity", Some(stroke), false);
        if let Some((corners, tile)) = laid {
            let sprite = self.tile_sprites.get(&tile).cloned().flatten();
            self.painting.lay_ahead(seq, vec![corners], sprite);
        }
    }

    /// Whether Shift is held and puts the chosen prototype where the cursor is. One that is
    /// anchored by default goes on the middle of its cell all the same.
    fn shift_frees(&self) -> bool {
        self.held.shift && self.chosen_prototype().is_some_and(|prototype| !prototype.anchored)
    }

    /// The place tool was pressed: places the chosen prototype under it and begins a row,
    /// which goes on only for a prototype that sits on cells, pressed without Shift. A press
    /// with Shift of one that is anchored by default says on the status line why it keeps to
    /// its cell.
    pub(super) fn place_down(&mut self, world: (f32, f32)) {
        if self.chosen_entity().is_none() {
            self.hint = Some(Hint::Entity);
        }
        let stroke = self.stroke();
        let mut placed = HashSet::new();
        self.place_at(world, stroke, &mut placed);
        if let Some(prototype) = self.chosen_prototype().filter(|prototype| self.held.shift && prototype.anchored) {
            let status = format!("{} is anchored, so it goes in the middle of its cell", prototype.id);
            self.set_status(status);
        }
        let row = !self.held.shift && self.chosen_prototype().is_some_and(Prototype::snaps);
        self.drag = Drag::Place(PlaceDrag { stroke, placed, row });
    }

    /// A place drag went on: places one more under the cursor, only in a row and only once
    /// in each cell.
    pub(super) fn place_dragged(&mut self, world: (f32, f32), drag: PlaceDrag) -> Drag {
        let PlaceDrag { stroke, mut placed, row } = drag;
        if row {
            self.place_at(world, stroke, &mut placed);
        }
        Drag::Place(PlaceDrag { stroke, placed, row })
    }

    /// Where a press of the tool in hand would put the chosen entity, for its ghost. The place
    /// tool puts it where `place_target` says, turned as it is turned. Paint, Rectangle, Line
    /// and Circle put it on the middle of the cell under the cursor, unturned, which is the
    /// first cell of a press. None during a shape's drag, which shows its cells, and where
    /// `ghost_hover` is None.
    fn ghost_target(&self) -> Option<GhostTarget> {
        let brush = match self.tool {
            Tool::Place => false,
            Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle => true,
            Tool::Pan | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Room | Tool::Area | Tool::Select | Tool::Link | Tool::Cable | Tool::Pipe => return None,
        };
        if !matches!(self.drag, Drag::None | Drag::Stroke(_) | Drag::Place(_)) {
            return None;
        }
        let world = self.ghost_hover()?;
        let index = self.chosen_entity()?;
        let doc = self.doc.as_ref()?;
        if brush {
            let grid = doc.grid_at(world)?;
            let cell = grid.cell_at(world);
            let at = grid.to_world((cell.0 as f32 + 0.5, cell.1 as f32 + 0.5));
            return Some(GhostTarget { index, at, square: grid.cells_outline(cell, cell), turns: 0, angle: grid.rotation, facing: None, free: false });
        }
        let (parent, local, at, grid_turned) = self.place_target(world)?;
        Some(GhostTarget {
            index,
            at,
            square: place_square(doc, parent, local, at, self.shift_frees()),
            turns: self.place_turns(),
            angle: grid_turned + self.place_turn_past_quarters(),
            facing: (self.placing.rotation != 0.0).then(|| self.placing.rotation as f32 + grid_turned),
            free: self.shift_frees(),
        })
    }

    /// The ghost of the chosen entity as rectangles for the renderer, half seen through, where
    /// a press of the tool in hand would put it (`ghost_target`). None while the engine has
    /// yet to send its picture.
    pub(super) fn entity_ghost_quads(&self) -> Option<Vec<f32>> {
        let target = self.ghost_target()?;
        let quads = self.preview(target.index, target.turns)?;
        let (sin, cos) = target.angle.sin_cos();
        let mut over = Vec::with_capacity(quads.len());
        for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
            for corner in quad[..8].chunks_exact(2) {
                over.extend_from_slice(&[target.at.0 + corner[0] * cos - corner[1] * sin, target.at.1 + corner[0] * sin + corner[1] * cos]);
            }
            over.extend_from_slice(&[quad[8], quad[9], quad[10], quad[11] * 0.5]);
            over.extend_from_slice(&quad[12..]);
        }
        Some(over)
    }

    /// What a press would put down where the cursor is, for the label, the arrow and the
    /// dashed square by the ghost, as the web editor draws them. `pixels_per_point` is the
    /// interface's, for how wide a tile is on screen. None where there is no ghost
    /// (`ghost_target`).
    pub fn place_preview(&self, pixels_per_point: f32) -> Option<PlacePreview> {
        let target = self.ghost_target()?;
        let prototype = self.prototypes.get(target.index)?;
        let ghost = self.entity_ghost_quads();
        let corners = ghost.iter().flat_map(|quads| quads.chunks_exact(FLOATS_PER_QUAD).flat_map(|quad| [quad[1], quad[3], quad[5], quad[7]]));
        let bottom = corners.chain(target.square.iter().map(|corner| corner.1)).fold(f32::MAX, f32::min);
        let shown = 32.0 * self.camera.zoom / pixels_per_point > 16.0;
        let label = shown.then(|| if target.free { format!("{} (free)", prototype.id) } else { prototype.id.clone() });
        let name = &prototype.name;
        let name = (shown && !name.is_empty() && !name.eq_ignore_ascii_case(&prototype.id) && name.chars().count() <= NAME_FITS).then(|| name.clone());
        // Unturned, a thing faces down the map, as the game draws it.
        let facing = target.facing.map(|angle| {
            let (sin, cos) = angle.sin_cos();
            (sin, -cos)
        });
        Some(PlacePreview {
            at: target.at,
            id: prototype.id.clone(),
            square: ghost.is_none().then_some(target.square),
            label,
            name,
            below: (target.at.0, bottom),
            colour: if target.free { GHOST_BLUE } else { GHOST_GREEN },
            facing,
        })
    }

    /// The outline of where the place tool would place for a cursor at `world`.
    pub(super) fn place_hover_outlines(&self, world: (f32, f32), doc: &Doc, outlines: &mut Vec<Outline>) {
        if let Some((parent, local, at, _)) = self.place_target(world) {
            outlines.push(Outline { corners: place_square(doc, parent, local, at, self.shift_frees()), style: OutlineStyle::Hover });
        }
    }
}

/// The square of a cell where the place tool would place, on the map, from what
/// `place_target` says: the cell on a grid, and off the grids a square round the place. Where
/// Shift frees it, it goes where the cursor is, and the square is round that.
fn place_square(doc: &Doc, parent: i32, local: (f32, f32), at: (f32, f32), free: bool) -> [(f32, f32); 4] {
    match doc.grid(parent) {
        Some(grid) if free => boxed((local.0 - 0.5, local.1 - 0.5, local.0 + 0.5, local.1 + 0.5)).map(|corner| grid.to_world(corner)),
        Some(grid) => grid.cells_outline((local.0.floor() as i32, local.1.floor() as i32), (local.0.floor() as i32, local.1.floor() as i32)),
        None => boxed((at.0 - 0.5, at.1 - 0.5, at.0 + 0.5, at.1 + 0.5)),
    }
}
