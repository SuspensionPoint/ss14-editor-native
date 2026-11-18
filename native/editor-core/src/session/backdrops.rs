//! The backdrop gallery: choosing the map's backdrop from the fork's, each with a picture, and
//! the one under the mouse shown behind the map before it is chosen.
//!
//! The gallery lists the game's default first and then the fork's backdrops by their ids. The
//! engine paints their pictures one at a time on a thread of its own, in the order asked
//! (`PaintAsk`), and the interface draws a placeholder for each until it is here. The one under
//! the mouse is asked for whole and first. It is shown in place of the map's own until the
//! mouse leaves, and nothing is edited. Choosing one is one edit of the map entity's Parallax
//! component, and the game's default takes the component away.

use super::*;
use crate::backdrop::Thumb;
use engine_thread::PaintAsk;

/// How many of the fork's backdrops are kept whole for showing behind the map, beside the
/// map's own. Each is some tens of megabytes of pictures.
const KEPT: usize = 3;

/// The backdrop the game draws when the map names none.
pub const GAME_DEFAULT: &str = "Default";

/// One choice of the gallery.
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropEntry {
    /// What the gallery calls it: "Game default", or the backdrop's id.
    pub label: String,
    /// The backdrop whose picture it shows.
    pub shows: String,
    /// The map has it now.
    pub current: bool,
    pub hovered: bool,
}

/// The gallery's state.
#[derive(Default)]
pub(super) struct Gallery {
    open: bool,
    /// The fork's backdrops once the engine has listed them, or why it would not.
    names: Option<Result<Vec<String>, String>>,
    names_asked: bool,
    /// The choice under the mouse.
    hovered: Option<usize>,
    /// The pictures that are here, by backdrop, or why one could not be painted.
    thumbs: HashMap<String, Result<Thumb, String>>,
    thumbs_asked: HashSet<String>,
    /// When the first pictures were asked for, until all are here.
    painting_since: Option<Instant>,
    /// The fork's backdrops kept whole, the one last used at the end.
    pub(super) whole: VecDeque<(String, Arc<Backdrop>)>,
    whole_asked: HashSet<String>,
    /// The map's own backdrop as it was last read, to show again when the mouse leaves.
    pub(super) own: Option<Arc<Backdrop>>,
    /// The edit that chose a backdrop, until the map's backdrop is read again. What the mouse
    /// was over stays shown till then.
    choosing: Option<u64>,
    /// Whether what is shown behind the map drifts by itself.
    shown_moves: bool,
}

impl Session {
    /// Why the map's backdrop cannot be chosen now, or None if it can.
    pub fn backdrop_unavailable(&self) -> Option<&'static str> {
        let Some(doc) = self.doc.as_ref() else { return Some("No map is open.") };
        if doc.is_grid_file() {
            return Some("A grid file has no backdrop of its own. The game draws the backdrop of whatever map it's loaded onto.");
        }
        match &self.backdrop {
            None => Some("The map's backdrop is still loading."),
            Some(info) if info.planet => Some("A planet map draws its biome instead of a backdrop."),
            Some(_) => None,
        }
    }

    /// Opens the gallery, and asks for the list and the pictures that are not here yet. Says
    /// why in the status bar if it cannot open.
    pub fn open_backdrops(&mut self) -> bool {
        if let Some(reason) = self.backdrop_unavailable() {
            self.set_status(reason);
            return false;
        }
        self.gallery.open = true;
        self.gallery.hovered = None;
        if self.gallery.names.is_none() && !self.gallery.names_asked {
            self.gallery.names_asked = true;
            self.send(ToEngine::BackdropNames);
        }
        self.ask_thumbs();
        true
    }

    pub fn backdrops_open(&self) -> bool {
        self.gallery.open
    }

    /// The gallery's choices: the game's default, then the fork's backdrops. Empty until the
    /// engine has listed them.
    pub fn backdrop_entries(&self) -> Vec<BackdropEntry> {
        let current = self.current_backdrop();
        let entry = |index: usize, label: &str, shows: &str| BackdropEntry {
            label: label.to_owned(),
            shows: shows.to_owned(),
            current: current == Some(index),
            hovered: self.gallery.hovered == Some(index),
        };
        let Some(Ok(names)) = &self.gallery.names else { return Vec::new() };
        std::iter::once(entry(0, "Game default", GAME_DEFAULT)).chain(names.iter().enumerate().map(|(index, name)| entry(index + 1, name, name))).collect()
    }

    /// Why the engine would not list the fork's backdrops, if it would not.
    pub fn backdrop_list_failure(&self) -> Option<&str> {
        match &self.gallery.names {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }

    /// A backdrop's picture: None while it is painted, an error if it could not be.
    pub fn backdrop_thumb(&self, name: &str) -> Option<Result<&Thumb, &str>> {
        self.gallery.thumbs.get(name).map(|thumb| thumb.as_ref().map_err(String::as_str))
    }

    /// The choice the map has now: 0 for the game's default. None if the map names a backdrop
    /// the fork does not have, or before the list is here.
    fn current_backdrop(&self) -> Option<usize> {
        let info = self.backdrop.as_ref()?;
        if !info.chosen {
            return Some(0);
        }
        let Some(Ok(names)) = &self.gallery.names else { return None };
        names.iter().position(|name| *name == info.name).map(|index| index + 1)
    }

    /// The backdrop a choice shows, and the name the map would be given: None for the game's default.
    fn backdrop_choice(&self, index: usize) -> Option<(String, Option<String>)> {
        if index == 0 {
            return Some((GAME_DEFAULT.to_owned(), None));
        }
        let Some(Ok(names)) = &self.gallery.names else { return None };
        names.get(index - 1).map(|name| (name.clone(), Some(name.clone())))
    }

    /// The mouse is over a choice: its backdrop is shown behind the map, once it is here.
    /// Returns whether that changed anything.
    pub fn hover_backdrop(&mut self, index: usize) -> bool {
        if !self.gallery.open || self.gallery.hovered == Some(index) {
            return false;
        }
        let Some((shows, _)) = self.backdrop_choice(index) else { return false };
        self.gallery.hovered = Some(index);
        match self.gallery.whole.iter().position(|(name, _)| *name == shows) {
            Some(at) => {
                // The one last used goes to the end, and is kept the longest.
                let kept = self.gallery.whole.remove(at).expect("a kept backdrop");
                self.show_backdrop(kept.1.clone());
                self.gallery.whole.push_back(kept);
            }
            None if self.gallery.whole_asked.insert(shows.clone()) => self.send(ToEngine::PaintBackdrop(PaintAsk { name: shows, whole: true })),
            None => {}
        }
        true
    }

    /// The mouse left the choices: the map's own backdrop is shown again. Returns whether that
    /// changed anything.
    pub fn leave_backdrops(&mut self) -> bool {
        let left = self.gallery.hovered.take().is_some();
        if left && self.gallery.choosing.is_none() {
            self.show_own_backdrop();
        }
        left
    }

    /// Gives the map the backdrop of a choice, as one edit, and closes the gallery. The one
    /// the map has already closes it and changes nothing.
    pub fn choose_backdrop(&mut self, index: usize) {
        let Some((shows, name)) = self.backdrop_choice(index) else { return };
        if self.current_backdrop() == Some(index) || self.backdrop_unavailable().is_some() {
            self.cancel_backdrops();
            return;
        }
        let Some(map) = self.doc.as_ref().map(|doc| doc.map_uid) else { return };
        let seq = self.seq();
        let component = name.as_ref().map(|name| json!({ "type": "Parallax", "parallax": name }));
        self.send(ToEngine::SetComponent { uid: map, kind: "Parallax", component, label: "Set backdrop", seq, asked: Instant::now() });
        self.edit_says = Some((seq, format!("Backdrop set to {}", name.as_deref().unwrap_or("the game's default"))));
        self.gallery.choosing = Some(seq);
        self.gallery.open = false;
        self.gallery.hovered = None;
        // What was chosen stays behind the map until the map's own is read again.
        if let Some((_, whole)) = self.gallery.whole.iter().find(|(name, _)| *name == shows) {
            self.show_backdrop(whole.clone());
        }
    }

    /// Closes the gallery with nothing changed.
    pub fn cancel_backdrops(&mut self) {
        self.gallery.open = false;
        self.leave_backdrops();
    }

    /// Whether what is shown behind the map drifts by itself, so that frames are wanted while
    /// nothing is done.
    pub fn backdrop_moves(&self) -> bool {
        self.gallery.shown_moves
    }

    /// Asks for the picture of each choice that has none yet, in the gallery's order.
    fn ask_thumbs(&mut self) {
        let Some(Ok(names)) = &self.gallery.names else { return };
        let wanted: Vec<String> = std::iter::once(GAME_DEFAULT).chain(names.iter().map(String::as_str)).map(str::to_owned).collect();
        for name in wanted {
            if !self.gallery.thumbs.contains_key(&name) && self.gallery.thumbs_asked.insert(name.clone()) {
                self.gallery.painting_since.get_or_insert_with(Instant::now);
                self.send(ToEngine::PaintBackdrop(PaintAsk { name, whole: false }));
            }
        }
    }

    /// Hands a backdrop to whoever draws.
    fn show_backdrop(&mut self, backdrop: Arc<Backdrop>) {
        self.gallery.shown_moves = backdrop.moves();
        self.backdrop_arrived = Some(backdrop);
    }

    fn show_own_backdrop(&mut self) {
        let own = self.gallery.own.clone().unwrap_or_default();
        self.show_backdrop(own);
    }

    // ---- what the engine answers ----

    /// The map's own backdrop was read, after it opened or after a change to it.
    pub(super) fn map_backdrop_arrived(&mut self, backdrop: Backdrop) {
        self.backdrop = Some(BackdropInfo {
            name: backdrop.name.clone(),
            layers: backdrop.layers.len(),
            skipped: backdrop.skipped,
            moves: backdrop.moves(),
            paint_ms: backdrop.paint_ms,
            chosen: backdrop.chosen,
            planet: backdrop.planet,
        });
        let backdrop = Arc::new(backdrop);
        self.gallery.own = Some(backdrop.clone());
        self.gallery.choosing = None;
        // Not while the mouse shows another in the gallery.
        if self.gallery.hovered.is_none() {
            self.show_backdrop(backdrop);
        }
    }

    pub(super) fn backdrop_names_arrived(&mut self, names: Result<Vec<String>, String>) {
        self.gallery.names_asked = false;
        if let Err(error) = &names {
            self.set_status(format!("The engine couldn't list the backdrops: {}", error.lines().next().unwrap_or("")));
        }
        self.gallery.names = Some(names);
        if self.gallery.open {
            self.ask_thumbs();
        }
    }

    pub(super) fn backdrop_painted(&mut self, name: String, thumb: Result<Thumb, String>, whole: Option<Backdrop>) {
        // One that could not be painted may be asked for again.
        if thumb.is_err() {
            self.gallery.whole_asked.remove(&name);
        }
        self.gallery.thumbs.insert(name.clone(), thumb);
        if let Some(whole) = whole {
            self.gallery.whole_asked.remove(&name);
            let whole = Arc::new(whole);
            self.gallery.whole.retain(|(kept, _)| *kept != name);
            self.gallery.whole.push_back((name.clone(), whole.clone()));
            while self.gallery.whole.len() > KEPT {
                self.gallery.whole.pop_front();
            }
            let hovered = self.gallery.hovered.and_then(|index| self.backdrop_choice(index)).map(|(shows, _)| shows);
            if self.gallery.open && hovered.as_deref() == Some(name.as_str()) {
                self.show_backdrop(whole);
            }
        }
        // All the pictures asked for are here: how long it took, once.
        if self.gallery.thumbs_asked.iter().all(|asked| self.gallery.thumbs.contains_key(asked)) {
            if let Some(since) = self.gallery.painting_since.take() {
                self.set_status(format!("{} backdrop pictures ready in {:.1} s", self.gallery.thumbs.len(), since.elapsed().as_secs_f64()));
            }
        }
    }

    /// An edit the engine refused: one that chose a backdrop leaves the map's own shown.
    pub(super) fn backdrop_refused(&mut self, seq: u64) {
        if self.gallery.choosing == Some(seq) {
            self.gallery.choosing = None;
            self.show_own_backdrop();
        }
    }

    /// Another map opened: the gallery closes, and its own backdrop is on its way.
    pub(super) fn backdrops_reset(&mut self) {
        self.gallery.open = false;
        self.gallery.hovered = None;
        self.gallery.choosing = None;
        self.gallery.own = None;
        self.show_backdrop(Arc::new(Backdrop::default()));
    }
}
