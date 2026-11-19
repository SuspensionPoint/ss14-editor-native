//! The Problems overlay and the fixes of Validate Map's rules of walls, inside the session.
//!
//! While the overlay is on, the cheap rules of Validate Map (`validate::quick`) run again
//! whenever the engine has answered a change, and a badge marks each cell they find something
//! on (`crate::problems`). The mouse over a badge shows what is wrong there, as the air
//! overlay's box does. The switch is kept per fork in the settings file.
//!
//! Validate Map's buttons: "Remove extras" deletes all but one of what is stacked on a cell, and
//! "Lay plating" puts plating under a wall, window or door with no tile. Each makes one edit,
//! and so one step to undo, for one finding or for a whole group. A delete goes through the
//! kernel's delete op, which clears what referred to the deleted. Something off every grid is
//! selected instead, since it has no grid to lay a tile on. Tests in `tools/tests/problems.rs`.

use super::*;
use crate::forks::settings::Settings;
use crate::problems::{self as draw, Badge};
use crate::validate::{extras, quick, Finding, Fix};

/// The tile laid under a wall with no floor.
pub const PLATING: &str = "Plating";

/// The overlay's state: the switch, where it is kept, and the badges as last found and drawn.
#[derive(Default)]
pub(super) struct Problems {
    shown: bool,
    /// The settings file and the fork the switch is kept under. None keeps it in the session.
    kept: Option<(PathBuf, String)>,
    /// What the findings were found for: the document, the number of changes the engine had
    /// sent, when the map was shown and how many prototypes were known.
    checked: Option<(i32, u64, Option<u64>, usize)>,
    badges: Vec<Badge>,
    /// How long the rules took the last time, in milliseconds.
    took_ms: f64,
    /// What the rectangles were drawn for: the findings' check, the badges' size and where
    /// each grid is.
    drawn: Option<(Option<(i32, u64, Option<u64>, usize)>, u32, Vec<(i32, [u32; 3])>)>,
    quads: Vec<f32>,
    version: u64,
    /// The badges' size when last drawn, in tiles, for the mouse.
    size: f32,
}

impl Problems {
    /// The switch as a settings file keeps it for a fork, kept there again at each change.
    pub(super) fn load(file: PathBuf, fork: &str) -> Problems {
        let shown = Settings::load(&file).problems_shown(fork);
        Problems { shown, kept: Some((file, fork.to_owned())), size: draw::BADGE_TILES, ..Problems::default() }
    }
}

impl Session {
    /// Whether the Problems overlay is on.
    pub fn problems_shown(&self) -> bool {
        self.problems.shown
    }

    /// Turns the Problems overlay on or off, and keeps the choice for the fork.
    pub fn show_problems(&mut self, on: bool) {
        self.problems.shown = on;
        if let Some((file, fork)) = &self.problems.kept {
            if let Err(error) = Settings::update(file, |settings| settings.show_problems(fork, on)) {
                self.set_status(format!("Couldn't save the Problems switch: {error}"));
            }
        }
    }

    /// Runs the cheap rules again if the map changed since they last ran, while the overlay is on.
    fn check_problems(&mut self) {
        let key = self.doc.as_ref().filter(|_| self.problems.shown).map(|doc| (doc.id, self.changes, self.map_shown_ms, self.prototypes.len()));
        if key == self.problems.checked {
            return;
        }
        self.problems.checked = key;
        let begun = Instant::now();
        self.problems.badges = match self.doc.as_ref().filter(|_| key.is_some()) {
            Some(doc) => draw::badges(&quick(doc, |id| self.prototype(id))),
            None => Vec::new(),
        };
        self.problems.took_ms = begun.elapsed().as_secs_f64() * 1000.0;
    }

    /// How long the cheap rules took the last time they ran, in milliseconds, and how many
    /// cells they marked.
    pub fn problems_checked(&self) -> (f64, usize) {
        (self.problems.took_ms, self.problems.badges.len())
    }

    /// The badges' rectangles while the overlay is on, and none otherwise, with a number that
    /// changes when they do. `tile_pixels` is how wide a tile is on screen, in the interface's
    /// pixels, for the badges' size. They are drawn again only when the findings, the size or
    /// a grid's place changes.
    pub fn problem_overlay(&mut self, tile_pixels: f32) -> (u64, &[f32]) {
        self.check_problems();
        let size = draw::badge_size(tile_pixels);
        let grids = self.doc.as_ref().map_or_else(Vec::new, |doc| doc.grids.iter().map(|grid| (grid.uid, [grid.x.to_bits(), grid.y.to_bits(), grid.rotation.to_bits()])).collect());
        let drawn = Some((self.problems.checked, size.to_bits(), grids));
        if drawn != self.problems.drawn {
            self.problems.quads = match self.doc.as_ref() {
                Some(doc) if self.problems.shown => draw::quads(&doc.grids, &self.problems.badges, size),
                _ => Vec::new(),
            };
            self.problems.drawn = drawn;
            self.problems.size = size;
            self.problems.version += 1;
        }
        (self.problems.version, &self.problems.quads)
    }

    /// The lines of the box the mouse shows over a badge: what is wrong on its cell. None off
    /// every badge, and while the overlay is off.
    pub fn problem_hover(&mut self) -> Option<Vec<String>> {
        if !self.problems.shown {
            return None;
        }
        self.check_problems();
        let (world, doc) = (self.hover?, self.doc.as_ref()?);
        draw::badge_at(&doc.grids, &self.problems.badges, world, self.problems.size).map(Badge::lines)
    }

    /// Does what a finding's button in Validate Map does (`Finding::fix`).
    pub fn fix_finding(&mut self, finding: &Finding) {
        match finding.fix() {
            Some(Fix::Air(grid)) => self.fix_grid_air(grid),
            Some(Fix::Select(uid)) => self.jump_to_entity(uid),
            Some(Fix::RemoveExtras | Fix::LayPlating) => self.fix_findings(std::slice::from_ref(finding)),
            None => {}
        }
    }

    /// Removes the extras and lays the plating these findings ask for, as one edit, as the map
    /// is now. Findings with another fix, or none, are passed over.
    pub fn fix_findings(&mut self, findings: &[Finding]) {
        let Some(doc) = self.doc.as_ref() else { return };
        let mut deleted: Vec<i32> = Vec::new();
        let mut laid: Vec<(i32, (i32, i32))> = Vec::new();
        // What stands loose on the cells laid, to anchor once the tile is there.
        let mut anchored: Vec<i32> = Vec::new();
        for finding in findings {
            match finding.fix() {
                Some(Fix::RemoveExtras) => {
                    for uid in extras(doc, |id| self.prototype(id), finding) {
                        if !deleted.contains(&uid) {
                            deleted.push(uid);
                        }
                    }
                }
                Some(Fix::LayPlating) => {
                    if doc.tile_at(finding.grid, finding.cell) == SPACE && !laid.contains(&(finding.grid, finding.cell)) {
                        laid.push((finding.grid, finding.cell));
                    }
                    let loose = finding.entity.filter(|uid| doc.entities.get(uid).is_some_and(|entity| entity.parent == finding.grid && !entity.anchored));
                    anchored.extend(loose.filter(|uid| !anchored.contains(uid)));
                }
                _ => {}
            }
        }
        if !laid.is_empty() && !self.tiles.iter().any(|tile| tile.id == PLATING) {
            self.set_status("Plating not laid: this fork has no Plating tile.");
            (laid, anchored) = (Vec::new(), Vec::new());
        }
        let mut ops: Vec<Value> = deleted.iter().map(|uid| json!({ "kind": "delete", "uid": uid })).collect();
        ops.extend(laid.iter().map(|(grid, cell)| json!({ "kind": "tile", "grid": grid, "x": cell.0, "y": cell.1, "tile": PLATING })));
        // The game takes anything anchored off a cell with no tile, so a wall there is loose.
        // A tile alone leaves it loose: the kernel's anchor op fixes it to the new tile.
        ops.extend(anchored.iter().map(|uid| json!({ "kind": "anchor", "uid": uid })));
        if ops.is_empty() {
            return;
        }
        let count = |count: usize, one: &str, many: &str| if count == 1 { format!("1 {one}") } else { format!("{count} {many}") };
        let (label, says) = match (deleted.len(), laid.len()) {
            (removed, 0) => ("Remove extras", format!("Removed {}", count(removed, "extra", "extras"))),
            (0, cells) => ("Lay plating", format!("Laid plating on {}", count(cells, "cell", "cells"))),
            (removed, cells) => ("Fix stacked walls", format!("Removed {} and laid plating on {}", count(removed, "extra", "extras"), count(cells, "cell", "cells"))),
        };
        let seq = self.edit(ops, label, None, false);
        self.edit_says = Some((seq, says));
    }
}
