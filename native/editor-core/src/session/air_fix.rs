//! Fix grid atmos (Map > Fix grid atmos...): the game's fixgridatmos run by the engine on the
//! grids chosen, as one edit and so one step to undo (`air.fix` in the kernel's AirFix.cs).
//!
//! A small dialog asks first: all grids, or the one under the mouse when it opened (else the
//! Grids tab's), what the fix does, and how many tiles it will change, which the engine counts
//! when the dialog opens. The game's fixgridatmos refuses a grid with no atmosphere. So the
//! dialog lists such grids with a choice, ticked at first, to add one first as the game's
//! atmos:add does (`air.add`), in the same edit. Unticked, they are left alone. A planet's own
//! ground is offered none, as the game gives it none: its air is the map's. A map with no grid
//! that has an atmosphere or can be given one cannot be fixed at all. Validate Map's "Floors
//! With No Air" and "Grids With No Atmosphere" fix their grid at once (`fix_grid_air`). The
//! engine reads the air of the grids again after the fix, as after any edit that changed it,
//! and the overlay, the rooms and Validate Map follow.

use super::*;

/// The edit's name in the history.
pub const FIX_AIR: &str = "Fix grid atmos";

/// What the fix does, in one sentence for the dialog.
pub const FIX_AIR_DOES: &str = "Every floor gets standard station air, walls get none, and markers get their own mix. Any air you set by hand on floors is replaced.";

/// The dialog's choice for grids with no atmosphere.
pub const ADD_FIRST: &str = "Add an atmosphere first, like the game's atmos:add";

/// Why a grid with no atmosphere is left alone when the choice to add one is not ticked.
const NOT_ADDED: &str = "It has no atmosphere, and none will be added first.";

/// Why the game's fixgridatmos would refuse a grid with no atmosphere.
const NO_ATMOSPHERE: &str = "It has no atmosphere, so the game's fixgridatmos refuses it.";

/// What the engine says a fix of one grid's air would change (`air.fix.preview`).
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FixPreview {
    pub grid: i32,
    /// Why the game would refuse to fix it, if it would.
    pub refused: Option<String>,
    /// How many of its tiles, and of the cells off its edge, would store other air.
    pub tiles: usize,
    pub outside: usize,
    /// It has no atmosphere and can be given one first. The counts are then what adding one
    /// and fixing would change.
    #[serde(default)]
    pub addable: bool,
}

/// The dialog's state, while it is open.
#[derive(Default)]
pub(super) struct FixingAir {
    open: bool,
    /// Whether all grids are chosen, or the one grid.
    all: bool,
    /// The one grid offered beside all of them, on a map with more than one.
    one: Option<i32>,
    /// Whether the grids chosen with no atmosphere are given one first.
    add_first: bool,
    /// What the engine counted when the dialog opened, or why it did not.
    preview: Option<Result<Vec<FixPreview>, String>>,
}

/// The dialog as the interface shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct FixAirDialog {
    /// "All grids (3)", and the one grid offered beside them, if there is one.
    pub all_label: String,
    pub one_label: Option<String>,
    /// Whether all grids are chosen.
    pub all: bool,
    pub does: &'static str,
    /// The grids chosen that have no atmosphere and can be given one. The choice to add one
    /// first is shown only if there are some.
    pub without: Vec<String>,
    /// Whether that choice is ticked, and its words.
    pub add_first: bool,
    pub add_label: &'static str,
    /// How many tiles the fix will change, or that they are being counted.
    pub count: String,
    /// The grids chosen that will be left alone, each with why.
    pub left_alone: Vec<String>,
    /// Why the fix cannot be run as chosen, if it cannot.
    pub blocked: Option<String>,
}

/// The grids chosen, as the fix will treat them.
struct Choice {
    /// Every grid to fix, those given an atmosphere first among them.
    fixed: Vec<i32>,
    /// The grids to give an atmosphere first.
    added: Vec<i32>,
    /// The grids chosen with no atmosphere that can be given one, ticked or not.
    without: Vec<i32>,
    /// The grids left alone, each with why.
    left: Vec<(i32, String)>,
}

/// "1 tile" or "n tiles", with commas in a large count.
fn tiles(count: usize, one: &str, many: &str) -> String {
    if count == 1 { format!("1 {one}") } else { format!("{} {many}", thousands(count)) }
}

impl Session {
    /// The grids of the open map whose air says this, in the order of the grid list, once the
    /// engine has read the air.
    fn grids_whose_air(&self, test: impl Fn(&crate::air::GridAir) -> bool) -> Vec<i32> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let Some(mirror) = doc.air.as_ref() else { return Vec::new() };
        doc.grids.iter().filter(|grid| mirror.grids.get(&grid.uid).is_some_and(&test)).map(|grid| grid.uid).collect()
    }

    /// The grids whose air can be fixed as they are: those with an atmosphere.
    fn fixable_grids(&self) -> Vec<i32> {
        self.grids_whose_air(|air| air.atmosphere)
    }

    /// The grids with no atmosphere that can be given one first.
    fn addable_grids(&self) -> Vec<i32> {
        self.grids_whose_air(|air| air.addable)
    }

    /// A grid's name and uid, as the dialog and the status bar say it.
    fn grid_label(&self, uid: i32) -> String {
        let name = self.doc.as_ref().and_then(|doc| doc.grid(uid)).map_or("grid", |grid| grid.name.as_str());
        format!("{name} ({uid})")
    }

    /// Why the air cannot be fixed now, or None if it can.
    pub fn fix_air_unavailable(&self) -> Option<String> {
        let Some(doc) = self.doc.as_ref() else { return Some("No map is open.".to_owned()) };
        if doc.air.is_none() {
            return Some("The map's air is still loading.".to_owned());
        }
        if !self.fixable_grids().is_empty() || !self.addable_grids().is_empty() {
            return None;
        }
        let planet = self.backdrop.as_ref().is_some_and(|backdrop| backdrop.planet);
        Some(if planet {
            "This planet map has no grid with an atmosphere. Its ground uses the map's air, and the game gives it none.".to_owned()
        } else {
            "This map has no grid with an atmosphere, and none that can take one.".to_owned()
        })
    }

    /// Opens the dialog, and asks the engine to count what a fix would change. The one grid
    /// offered is the one under the mouse, or the Grids tab's. Says why in the status bar if
    /// it cannot open.
    pub fn open_fix_air(&mut self) -> bool {
        if let Some(reason) = self.fix_air_unavailable() {
            self.set_status(reason);
            return false;
        }
        let Some(doc) = self.doc.as_ref() else { return false };
        let under = self.hover.and_then(|world| doc.grid_with_tile_at(world)).map(|grid| grid.uid);
        let one = (doc.grids.len() > 1).then(|| under.unwrap_or(doc.active_grid)).filter(|uid| doc.grid(*uid).is_some());
        self.fixing_air = FixingAir { open: true, all: true, one, add_first: true, preview: None };
        self.send(ToEngine::AirFixPreview);
        true
    }

    pub fn fix_air_open(&self) -> bool {
        self.fixing_air.open
    }

    /// Chooses all grids, or the one grid offered.
    pub fn choose_fix_air_grids(&mut self, all: bool) {
        if self.fixing_air.one.is_some() || all {
            self.fixing_air.all = all;
        }
    }

    /// Ticks or unticks the choice to give the grids with no atmosphere one first.
    pub fn set_fix_air_add_first(&mut self, add: bool) {
        self.fixing_air.add_first = add;
    }

    /// The grids chosen, and what the fix does with each. A grid's own count, once the engine
    /// has given it, says whether it can be given an atmosphere, and else the air read last.
    fn fix_air_choice(&self) -> Choice {
        let fixable = self.fixable_grids();
        let addable = self.addable_grids();
        let chosen: Vec<i32> = match (self.fixing_air.all, self.fixing_air.one) {
            (false, Some(one)) => vec![one],
            _ => self.doc.as_ref().map_or_else(Vec::new, |doc| doc.grids.iter().map(|grid| grid.uid).collect()),
        };
        let told: HashMap<i32, &FixPreview> = match &self.fixing_air.preview {
            Some(Ok(previews)) => previews.iter().map(|preview| (preview.grid, preview)).collect(),
            _ => HashMap::new(),
        };
        let mut choice = Choice { fixed: Vec::new(), added: Vec::new(), without: Vec::new(), left: Vec::new() };
        for grid in chosen {
            let has = fixable.contains(&grid);
            let can_add = !has && told.get(&grid).map_or_else(|| addable.contains(&grid), |preview| preview.addable);
            if can_add {
                choice.without.push(grid);
                if self.fixing_air.add_first {
                    choice.added.push(grid);
                    choice.fixed.push(grid);
                } else {
                    choice.left.push((grid, NOT_ADDED.to_owned()));
                }
            } else if let Some(why) = told.get(&grid).and_then(|preview| preview.refused.as_deref()) {
                choice.left.push((grid, why.to_owned()));
            } else if !has {
                choice.left.push((grid, NO_ATMOSPHERE.to_owned()));
            } else {
                choice.fixed.push(grid);
            }
        }
        choice
    }

    /// The dialog as it stands, while it is open.
    pub fn fix_air_dialog(&self) -> Option<FixAirDialog> {
        if !self.fixing_air.open {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let Choice { fixed, without, left, .. } = self.fix_air_choice();
        let count = match &self.fixing_air.preview {
            None => "Counting the tiles it will change...".to_owned(),
            Some(Err(error)) => format!("The engine couldn't count the tiles: {}", error.lines().next().unwrap_or("")),
            Some(Ok(previews)) => {
                let of = |grid: &i32| previews.iter().find(|preview| preview.grid == *grid);
                let changed: usize = fixed.iter().filter_map(of).map(|preview| preview.tiles).sum();
                let outside: usize = fixed.iter().filter_map(of).map(|preview| preview.outside).sum();
                let mut count = match changed {
                    0 => "No tile's air will change.".to_owned(),
                    _ => format!("The air of {} will change.", tiles(changed, "tile", "tiles")),
                };
                if outside > 0 {
                    count += &format!(" {} off the grid will lose the air stored there.", tiles(outside, "cell", "cells"));
                }
                count
            }
        };
        let blocked = match (fixed.is_empty(), left.first()) {
            (true, Some((grid, why))) => Some(format!("{}: {why}", self.grid_label(*grid))),
            (true, None) => Some("No grid chosen.".to_owned()),
            (false, _) => None,
        };
        Some(FixAirDialog {
            all_label: format!("All grids ({})", doc.grids.len()),
            one_label: self.fixing_air.one.map(|one| self.grid_label(one)),
            all: self.fixing_air.all || self.fixing_air.one.is_none(),
            does: FIX_AIR_DOES,
            without: without.iter().map(|grid| self.grid_label(*grid)).collect(),
            add_first: self.fixing_air.add_first,
            add_label: ADD_FIRST,
            count,
            left_alone: left.iter().map(|(grid, why)| format!("{}: {why}", self.grid_label(*grid))).collect(),
            blocked,
        })
    }

    /// Fixes the air of the grids chosen, as one edit, and closes the dialog.
    pub fn confirm_fix_air(&mut self) {
        if !self.fixing_air.open {
            return;
        }
        let Choice { fixed, added, .. } = self.fix_air_choice();
        self.fixing_air.open = false;
        self.fix_air_of(&fixed, &added);
    }

    /// Closes the dialog with nothing changed.
    pub fn cancel_fix_air(&mut self) {
        self.fixing_air.open = false;
    }

    /// Fixes the air of one grid at once, with no dialog: Validate Map's Fix. A grid with no
    /// atmosphere that can be given one is given one first, in the same edit.
    pub fn fix_grid_air(&mut self, grid: i32) {
        if self.fixable_grids().contains(&grid) {
            return self.fix_air_of(&[grid], &[]);
        }
        if self.addable_grids().contains(&grid) {
            return self.fix_air_of(&[grid], &[grid]);
        }
        let why = self.fix_air_unavailable().unwrap_or_else(|| format!("{} has no atmosphere, so the game's fixgridatmos refuses it.", self.grid_label(grid)));
        self.set_status(format!("Air not fixed: {why}"));
    }

    /// The one edit that gives these grids an atmosphere and then fixes the air of those, and
    /// what the status bar says when it is done.
    fn fix_air_of(&mut self, grids: &[i32], added: &[i32]) {
        if grids.is_empty() {
            return;
        }
        let ops = added.iter().map(|grid| json!({ "kind": "air.add", "grid": grid })).chain(grids.iter().map(|grid| json!({ "kind": "air.fix", "grid": grid }))).collect();
        let says = match (grids, added.len()) {
            ([one], 0) => format!("Fixed the air of {}", self.grid_label(*one)),
            ([one], _) => format!("Gave {} an atmosphere and fixed its air", self.grid_label(*one)),
            (_, 0) => format!("Fixed the air of {} grids", grids.len()),
            (_, 1) => format!("Fixed the air of {} grids, 1 given an atmosphere first", grids.len()),
            (_, given) => format!("Fixed the air of {} grids, {given} given an atmosphere first", grids.len()),
        };
        let seq = self.edit(ops, FIX_AIR, None, false);
        self.edit_says = Some((seq, says));
    }

    /// The engine counted what a fix would change. A count for a map no longer open is passed over.
    pub(super) fn air_fix_previewed(&mut self, document: i32, previews: Result<Vec<FixPreview>, String>) {
        if self.doc.as_ref().is_some_and(|doc| doc.id == document) && self.fixing_air.open {
            self.fixing_air.preview = Some(previews);
        }
    }

    /// Another map opened: the dialog closes.
    pub(super) fn fix_air_reset(&mut self) {
        self.fixing_air = FixingAir::default();
    }
}
