//! Laying a tile under new walls, and the wall the Room tool builds with.
//!
//! A brush that puts an entity which needs a floor (`needs_floor`) on a cell with no tile
//! lays a tile there first, in the same edit, when the mapper has it switched on, as it is by
//! default. The tile is Plating unless the mapper picks another. It goes on the grid the tile
//! tools use, as `tiles.rs` paints, so a wall drawn into space next to a grid extends it. A cell
//! that has a tile keeps it. The switch, the tile and the Room tool's wall are kept per fork in
//! the settings file.

use super::tiles::SHAPE_LIMIT;
use super::*;
use crate::forks::settings::{ForkPlacing, Settings};
use crate::validate::ALLOWED_WALLS;

/// How faint the ghost of a tile a press would lay is.
const FLOOR_GHOST: f32 = 0.45;

/// The wall the Room tool builds with until one is chosen, where the fork has it.
const FIRST_WALL: &str = "WallSolid";

/// The walls the Room tool's list starts with, in this order.
const FIRST_WALLS: &[&str] = &[FIRST_WALL, "WallReinforced"];

/// Whether a wall is rock: an asteroid or mineral rock, or another of the walls Validate Map
/// lets stand on any tile (`ALLOWED_WALLS`). The Room tool's list puts them last.
fn is_rock(id: &str) -> bool {
    let rock_word = id.match_indices("Rock").any(|(at, _)| !id[at + 4..].starts_with(|next: char| next.is_ascii_lowercase()));
    ALLOWED_WALLS.contains(&id) || id.starts_with("AsteroidRock") || rock_word
}

/// Whether an entity of this prototype needs a floor under it, so that a brush lays a tile on
/// a cell with no tile before it puts one there. These do:
///
/// - anything with the Wall tag, as Validate Map reads it, diagonal walls too, since they
///   stand on a tile as well,
/// - anything anchored by default with the Window tag (windows),
/// - anything anchored by default with the CanBuildWindowOnTop component (grilles),
/// - anything anchored by default with the Door component (airlocks, firelocks, windoors,
///   shutters, blast doors, and the game's other doors, curtains and fences).
///
/// Nothing else does. The tags and components are the prototype's own, as the engine tells
/// them.
pub fn needs_floor(prototype: &Prototype) -> bool {
    let tagged = |tag: &str| prototype.tags.iter().any(|known| &**known == tag);
    prototype.is_wall() || (prototype.anchored && (tagged("Window") || prototype.has("CanBuildWindowOnTop") || prototype.has("Door")))
}

/// Whether the Room tool can build with this prototype: it has the Wall tag and not the
/// Diagonal tag, and a mapper can place it.
pub fn is_room_wall(prototype: &Prototype) -> bool {
    let tagged = |tag: &str| prototype.tags.iter().any(|known| &**known == tag);
    prototype.is_wall() && !tagged("Diagonal") && prototype.placeable()
}

/// How walls are placed, and where that is kept.
#[derive(Default)]
pub(in crate::session) struct Flooring {
    placing: ForkPlacing,
    /// The settings file it is kept in. None keeps it in the session only.
    kept: Option<PathBuf>,
}

impl Flooring {
    /// How a settings file says a fork's walls are placed, kept there again at each change.
    pub(in crate::session) fn load(file: PathBuf, fork: &str) -> Flooring {
        Flooring { placing: Settings::load(&file).placing(fork), kept: Some(file) }
    }

    /// Writes it into the settings file, which is read again first so that nothing else in it
    /// is lost.
    fn save(&self) -> Result<(), String> {
        let Some(file) = &self.kept else { return Ok(()) };
        Settings::update(file, |settings| settings.set_placing(self.placing.clone())).map(|_| ())
    }
}

impl Session {
    /// Whether a brush lays a tile under a wall it puts on a cell with no tile.
    pub fn floor_under_walls(&self) -> bool {
        self.flooring.placing.floor_under_walls
    }

    /// Switches laying a tile under new walls on or off, and keeps that for the fork.
    pub fn set_floor_under_walls(&mut self, on: bool) {
        self.flooring.placing.floor_under_walls = on;
        self.save_flooring();
    }

    /// The tile laid under new walls, and on the edge of a room.
    pub fn floor_tile(&self) -> &str {
        &self.flooring.placing.floor_tile
    }

    /// Picks the tile laid under new walls, and keeps it for the fork.
    pub fn set_floor_tile(&mut self, id: &str) {
        self.flooring.placing.floor_tile = id.to_owned();
        self.save_flooring();
    }

    fn save_flooring(&mut self) {
        if let Err(error) = self.flooring.save() {
            self.set_status(format!("Placement settings not saved: {error}"));
        }
    }

    /// The tile a brush lays under an entity of this prototype on a cell with no tile, if it
    /// lays one at all.
    pub(super) fn floor_for(&self, prototype: &Prototype) -> Option<String> {
        (self.floor_under_walls() && needs_floor(prototype)).then(|| self.floor_tile().to_owned())
    }

    /// The tile a press of the tool in hand would lay under the chosen entity, if it would
    /// lay one: the tool is the place tool or a brush that puts entities down.
    fn floor_in_hand(&self) -> Option<String> {
        let brush = matches!(self.tool, Tool::Place | Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Lasso | Tool::Polygon);
        self.chosen_prototype().filter(|_| brush).and_then(|prototype| self.floor_for(prototype))
    }

    /// What the status bar notes while a press would lay a tile under the chosen entity.
    pub fn floor_note(&self) -> Option<String> {
        self.floor_in_hand().map(|tile| format!("Lays {tile} where there's no tile"))
    }

    /// The wall the Room tool builds with, as an index into [`Self::prototypes`]: the one
    /// chosen last, or else WallSolid where the fork has it. None where there is neither.
    pub fn room_wall(&self) -> Option<usize> {
        let chosen = self.flooring.placing.room_wall.as_deref().and_then(|id| self.prototype_indices.get(id).copied());
        chosen.or_else(|| self.prototype_indices.get(FIRST_WALL).copied()).filter(|&index| is_room_wall(&self.prototypes[index]))
    }

    /// Picks the wall the Room tool builds with, and keeps it for the fork. A prototype that
    /// is no wall the tool can build with is not taken.
    pub fn set_room_wall(&mut self, index: usize) {
        let Some(prototype) = self.prototypes.get(index).filter(|prototype| is_room_wall(prototype)) else { return };
        if self.flooring.placing.room_wall.as_deref() != Some(prototype.id.as_str()) {
            self.flooring.placing.room_wall = Some(prototype.id.clone());
            self.save_flooring();
        }
    }

    /// Whether the Room tool joins a new room to the rooms it overlaps (its Intersection
    /// switch), or keeps them separate.
    pub fn room_intersection(&self) -> bool {
        self.flooring.placing.room_intersection
    }

    /// Switches the Room tool's Intersection on or off, and keeps that for the fork.
    pub fn set_room_intersection(&mut self, on: bool) {
        if self.flooring.placing.room_intersection != on {
            self.flooring.placing.room_intersection = on;
            self.save_flooring();
        }
    }

    /// The shape the Room tool builds rooms in.
    pub fn room_shape(&self) -> crate::toolbox::RoomShape {
        self.flooring.placing.room_shape
    }

    /// Sets the shape the Room tool builds rooms in, and keeps it for the fork. A room being
    /// drawn is dropped.
    pub fn set_room_shape(&mut self, shape: crate::toolbox::RoomShape) {
        self.cancel_room();
        if self.flooring.placing.room_shape != shape {
            self.flooring.placing.room_shape = shape;
            self.save_flooring();
        }
    }

    /// The walls the Room tool can build with whose id or name has every one of the words in
    /// it, in either case, as indices into [`Self::prototypes`]. No words is all of them. The
    /// walls rooms are built from come first: WallSolid, WallReinforced, then the other walls
    /// of the palette's Structures/Walls, then the rest, with the rocks last, each part by name.
    pub fn room_walls(&self, words: &[&str]) -> Vec<usize> {
        let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
        let mut found: Vec<usize> = self
            .prototypes
            .iter()
            .enumerate()
            .filter(|(_, prototype)| is_room_wall(prototype) && words.iter().all(|word| prototype.search.contains(word.as_str()) || prototype.id.to_lowercase().contains(word.as_str())))
            .map(|(index, _)| index)
            .collect();
        let rank = |prototype: &Prototype| {
            let station = self.palette_category(&prototype.id).is_some_and(|category| category.starts_with("Structures/Walls"));
            match FIRST_WALLS.iter().position(|first| *first == prototype.id) {
                Some(at) => at,
                None if is_rock(&prototype.id) => FIRST_WALLS.len() + 2,
                None if station => FIRST_WALLS.len(),
                None => FIRST_WALLS.len() + 1,
            }
        };
        found.sort_by_cached_key(|&index| {
            let prototype = &self.prototypes[index];
            (rank(prototype), shown_name(prototype).to_lowercase(), prototype.id.clone())
        });
        found
    }

    /// Puts a tile's picture on cells of a grid, faint, as rectangles for the renderer. A tile
    /// with no picture known is a faint square.
    pub(super) fn faint_tiles(&mut self, grid_uid: i32, cells: &[(i32, i32)], tile: &str, out: &mut Vec<f32>) {
        let Some(grid) = self.doc.as_ref().and_then(|doc| doc.grid(grid_uid)) else { return };
        let corners: Vec<[(f32, f32); 4]> = cells.iter().map(|&cell| grid.cells_outline(cell, cell)).collect();
        let (texture, colour, source) = match self.tile_sprites.get(tile).cloned().flatten() {
            Some(sprite) => (self.lists.local_texture(&sprite, 0, 32, 32), [1.0, 1.0, 1.0, FLOOR_GHOST], [0.0, 0.0, 32.0, 32.0]),
            None => (WHITE_TEXTURE, [1.0, 1.0, 1.0, FLOOR_GHOST / 2.0], WHITE_SOURCE),
        };
        for square in corners {
            for corner in square {
                out.extend_from_slice(&[corner.0, corner.1]);
            }
            out.extend_from_slice(&colour);
            out.push(texture as f32);
            out.extend_from_slice(&source);
        }
    }

    /// The ghost of the tiles a press of the tool in hand would lay under the chosen entity,
    /// for `overlay_quads`: on the cell under the cursor before a press, and on the cells of a
    /// rectangle, line or circle while it is dragged, where they have no tile.
    pub(super) fn floor_ghost(&mut self, under: &mut Vec<f32>) {
        let (grid, cells, tile) = match &self.drag {
            // A shape lays what the entity it was pressed with calls for.
            Drag::Rectangle(drag) => {
                let Some((grid, cells, index)) = drag.entity_cells() else { return };
                let Some(tile) = self.prototypes.get(index).and_then(|prototype| self.floor_for(prototype)) else { return };
                (grid, cells, tile)
            }
            Drag::Shape(drag) => {
                let Some((grid, cells, index)) = drag.entity_cells() else { return };
                let Some(tile) = self.prototypes.get(index).and_then(|prototype| self.floor_for(prototype)) else { return };
                (grid, cells, tile)
            }
            Drag::None | Drag::Stroke(_) | Drag::Place(_) => {
                let Some(tile) = self.floor_in_hand() else { return };
                let Some(world) = self.ghost_hover() else { return };
                let found = if self.tool == Tool::Place {
                    self.place_target(world).and_then(|(parent, local, ..)| self.doc.as_ref()?.grid(parent).map(|_| (parent, (local.0.floor() as i32, local.1.floor() as i32))))
                } else {
                    self.cell_at(world)
                };
                let Some((grid, cell)) = found else { return };
                (grid, vec![cell], tile)
            }
            _ => return,
        };
        if cells.len() > SHAPE_LIMIT {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let bare: Vec<(i32, i32)> = cells.into_iter().filter(|&cell| doc.tile_at(grid, cell) == SPACE).collect();
        self.faint_tiles(grid, &bare, &tile, under);
    }
}
