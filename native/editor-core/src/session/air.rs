//! The air overlay in the session: the air the engine read taken into the mirror, the
//! overlay's rectangles and its rooms' labels built again only when the air, the view, a
//! grid's place or the strength of the marks changes, and the box the mouse shows over a tile
//! (`crate::air` has the rules, `crate::air_draw` the drawing). Tests in `tools/tests/air.rs`.

use super::*;
use crate::air::{self as rules, AirMirror, Legend, TileAir};
use crate::air_draw::{self, AirLabel};

/// What the overlay's rectangles and labels were built from: the document, the view, the
/// mirror's revision and where each grid is.
#[derive(Clone, Debug, PartialEq)]
struct Built {
    document: i32,
    view: AirView,
    revision: u64,
    grids: Vec<(i32, [u32; 3])>,
}

/// The overlay's rectangles as last built, with how strongly their marks were drawn and a
/// number that changes whenever they do, and the rooms' labels.
#[derive(Default)]
pub(super) struct AirDrawn {
    built: Option<Built>,
    marks: Option<u32>,
    quads: Vec<f32>,
    version: u64,
    labels: Vec<AirLabel>,
}

impl Session {
    /// The engine read the air of a document's grids. One that is no longer open is passed over.
    pub(super) fn air_arrived(&mut self, document: i32, asked: Option<Vec<i32>>, answer: Result<AirAnswer, String>) {
        let Some(doc) = self.doc.as_mut().filter(|doc| doc.id == document) else { return };
        match answer {
            Ok(answer) => doc.air.get_or_insert_with(AirMirror::default).update(asked.as_deref(), answer),
            Err(error) if asked.is_none() => self.set_status(format!("The engine couldn't read the air: {}", error.lines().next().unwrap_or(""))),
            // The air of the last read stays shown. The next change reads it again.
            Err(_) => {}
        }
    }

    /// Builds the rectangles and labels again if what they show has changed. The labels do
    /// not change with the marks' strength.
    fn build_air(&mut self, tile_pixels: f32) {
        let built = match (self.show_air, self.doc.as_ref()) {
            (true, Some(doc)) => doc.air.as_ref().map(|mirror| Built {
                document: doc.id,
                view: self.air_view,
                revision: mirror.revision,
                grids: doc.grids.iter().map(|grid| (grid.uid, [grid.x.to_bits(), grid.y.to_bits(), grid.rotation.to_bits()])).collect(),
            }),
            _ => None,
        };
        let strength = air_draw::mark_strength(tile_pixels);
        let marks = Some((strength * 4.0) as u32);
        let changed = built != self.air_drawn.built;
        if !changed && marks == self.air_drawn.marks {
            return;
        }
        let mirror = self.doc.as_ref().filter(|_| built.is_some()).and_then(|doc| Some((doc, doc.air.as_ref()?)));
        self.air_drawn.quads = mirror.map_or_else(Vec::new, |(doc, mirror)| air_draw::quads(&doc.grids, mirror, self.air_view, strength));
        if changed {
            self.air_drawn.labels = mirror.map_or_else(Vec::new, |(doc, mirror)| air_draw::labels(&doc.grids, mirror, self.air_view));
        }
        self.air_drawn.built = built;
        self.air_drawn.marks = marks;
        self.air_drawn.version += 1;
    }

    /// The overlay's rectangles while it is shown and the engine has read the air, and none
    /// otherwise, for `MapRenderer::set_air`, with a number that changes when they do.
    /// `tile_pixels` is how wide a tile is on screen, in the interface's pixels, for the marks'
    /// strength. The rectangles are built again only when something they show changes, so
    /// panning builds nothing, and zooming only as the marks fade.
    pub fn air_overlay(&mut self, tile_pixels: f32) -> (u64, &[f32]) {
        self.build_air(tile_pixels);
        (self.air_drawn.version, &self.air_drawn.quads)
    }

    /// The labels of the rooms shown at this size of tile on screen (`air_draw::label_shown`),
    /// with their places on the map, while the overlay is on.
    pub fn air_labels(&mut self, tile_pixels: f32) -> Vec<AirLabel> {
        self.build_air(tile_pixels);
        self.air_drawn.labels.iter().filter(|label| air_draw::label_shown(label.tiles, tile_pixels)).cloned().collect()
    }

    /// What the tile under the mouse holds, while the overlay is on: a grid with a tile there
    /// first, then any grid that stores air there, as off the edge of a grid.
    fn air_under_mouse(&self) -> Option<(TileAir<'_>, &[String])> {
        if !self.show_air {
            return None;
        }
        let world = self.hover?;
        let doc = self.doc.as_ref()?;
        let mirror = doc.air.as_ref()?;
        let found = doc.grid_with_tile_at(world).into_iter().chain(&doc.grids).map(|grid| mirror.at(grid.uid, grid.cell_at(world))).find(|air| *air != TileAir::Nothing);
        Some((found.unwrap_or(TileAir::Nothing), &mirror.gases))
    }

    /// The lines of the box the mouse shows over a tile while the overlay is on: what the tile
    /// is, its pressure and temperature, and each gas with its share. In the Rooms view, the
    /// tile's room. None over a tile with nothing to tell.
    pub fn air_hover(&self) -> Option<Vec<String>> {
        if self.show_air && self.air_view == AirView::Rooms {
            return self.room_under_mouse();
        }
        let (air, gases) = self.air_under_mouse()?;
        rules::hover_text(air, gases)
    }

    /// The icon that leads the box by the mouse, with the colour of its circle: the kind of the
    /// tile under it. None in the Rooms view.
    pub fn air_hover_icon(&self) -> Option<(rules::Icon, [u8; 3])> {
        if self.air_view == AirView::Rooms {
            return None;
        }
        let (air, gases) = self.air_under_mouse()?;
        rules::hover_icon(air, gases)
    }

    /// The room under the mouse: a grid with a tile there first, then any grid with a room or
    /// a wall there.
    fn room_under_mouse(&self) -> Option<Vec<String>> {
        let world = self.hover?;
        let doc = self.doc.as_ref()?;
        let mirror = doc.air.as_ref()?;
        doc.grid_with_tile_at(world).into_iter().chain(&doc.grids).find_map(|grid| rules::room_hover(mirror, grid.uid, grid.cell_at(world)))
    }

    /// The legend of the view shown, while the overlay is on.
    pub fn air_legend(&self) -> Option<Legend> {
        self.show_air.then(|| rules::legend(self.air_view, self.doc.as_ref().and_then(|doc| doc.air.as_ref())))
    }
}
