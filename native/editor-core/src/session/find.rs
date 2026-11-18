//! The entity search and Validate Map, as the session offers them: the search and the rules
//! are `crate::search` and `crate::validate`, and this is jumping to what they found.
//!
//! A jump to a search result takes the select tool in hand, selects the entity, centres the
//! view on it and zooms to three, as the web editor does. A jump to a finding centres the
//! view on its cell, zooms to three if the view is further out than two, and outlines the cell
//! for three seconds (`OutlineStyle::Found`). It selects nothing, as in the web editor.

use std::time::Duration;

use super::*;
use crate::search::{find_entities, Found};
use crate::validate::{validate, Finding};

/// How close a jump zooms, the web editor's.
const JUMP_ZOOM: f32 = 3.0;

/// How long a finding's cell is outlined after a jump to it.
const FOUND_FOR: Duration = Duration::from_secs(3);

/// A cell a jump outlines: its grid, or 0 for the map itself, the cell, and when it began.
#[derive(Clone, Copy, Debug)]
pub(super) struct Highlight {
    grid: i32,
    cell: (i32, i32),
    since: Instant,
}

impl Session {
    /// The placed entities a search box finds for a query (`crate::search::find_entities`).
    pub fn find_entities(&self, query: &str) -> Vec<Found> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        find_entities(doc, query, |id| self.prototype(id).map(|prototype| prototype.name.as_str()))
    }

    /// Takes the select tool in hand, selects an entity, and centres the view on it close up.
    pub fn jump_to_entity(&mut self, uid: i32) {
        let Some(world) = self.doc.as_ref().and_then(|doc| doc.world_of(uid)) else { return };
        self.set_tool(Tool::Select);
        self.selection = vec![uid];
        self.selected_decals.clear();
        self.stop_zoom();
        self.camera = Camera { x: world.0, y: world.1, zoom: JUMP_ZOOM };
    }

    /// The open map checked by every rule (`crate::validate`), in the order of the rules.
    pub fn validate(&self) -> Vec<Finding> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let decal_ids: HashSet<&str> = self.decal_prototypes.iter().map(|decal| decal.id.as_str()).collect();
        let pryable: HashSet<&str> = self.tiles.iter().filter(|tile| tile.can_crowbar).map(|tile| tile.id.as_str()).collect();
        validate(doc, |id| self.prototype(id), &decal_ids, &pryable)
    }

    /// What Validate Map's findings rest on that can change while its list is open: the
    /// document's revision and the revision of its air, which the engine reads again after an
    /// edit, as after a fix of the air. Findings found for another are found again.
    pub fn validation_key(&self) -> (i32, u64) {
        self.doc.as_ref().map_or((0, 0), |doc| (doc.history.revision, doc.air.as_ref().map_or(0, |air| air.revision)))
    }

    /// Whether the engine has listed the open map's links, so that the rules of links are checked.
    pub fn links_known(&self) -> bool {
        self.doc.as_ref().is_some_and(|doc| doc.links.is_some())
    }

    /// Centres the view on a finding's cell, closer if it is far out, and outlines the cell.
    pub fn jump_to_finding(&mut self, finding: &Finding) {
        self.jump_to_cell(finding.grid, finding.cell, Instant::now());
    }

    fn jump_to_cell(&mut self, grid: i32, cell: (i32, i32), now: Instant) {
        let Some(doc) = self.doc.as_ref() else { return };
        let middle = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
        let world = doc.grid(grid).map_or(middle, |grid| grid.to_world(middle));
        self.stop_zoom();
        self.camera.x = world.0;
        self.camera.y = world.1;
        if self.camera.zoom < 2.0 {
            self.camera.zoom = JUMP_ZOOM;
        }
        self.highlight = Some(Highlight { grid, cell, since: now });
    }

    /// Whether a cell a jump outlined is still outlined, so that the interface draws frames
    /// until it is not.
    pub fn highlight_live(&self) -> bool {
        self.highlight.is_some_and(|highlight| highlight.since.elapsed() < FOUND_FOR)
    }

    /// The outline of the cell a jump outlined, while it lasts.
    pub(in crate::session) fn found_outlines(&self, outlines: &mut Vec<Outline>) {
        let (Some(highlight), Some(doc)) = (self.highlight, self.doc.as_ref()) else { return };
        if highlight.since.elapsed() >= FOUND_FOR {
            return;
        }
        let (x, y) = (highlight.cell.0 as f32, highlight.cell.1 as f32);
        let corners = match doc.grid(highlight.grid) {
            Some(grid) => grid.cells_outline(highlight.cell, highlight.cell),
            None => [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)],
        };
        outlines.push(Outline { corners, style: OutlineStyle::Found });
    }
}

#[cfg(test)]
#[path = "../tests/session/find.rs"]
mod tests;
