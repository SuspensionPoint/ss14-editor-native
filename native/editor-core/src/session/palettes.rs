//! What the palettes hold as an interface shows it: what is in hand, and whether a choice made
//! away from the palettes asks them to bring it into view.

use super::*;
use crate::palette::{scan_categories, EntityTree};

/// The entity palette's tree, and what it is made from: the category of each prototype by its
/// id, read from the fork's files on a thread of its own as the session starts.
#[derive(Default)]
pub(in crate::session) struct Palettes {
    scanning: Option<Receiver<HashMap<String, String>>>,
    categories: Option<HashMap<String, String>>,
    tree: Option<EntityTree>,
}

impl Palettes {
    /// Starts reading the fork's prototype files for their categories. `wake` is called when
    /// they have been read.
    pub(in crate::session) fn scan(fork_repo: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Palettes {
        let (send, scanning) = mpsc::channel();
        let fork = PathBuf::from(fork_repo);
        let spawned = std::thread::Builder::new().name("categories".into()).spawn(move || {
            let _ = send.send(scan_categories(&fork));
            wake();
        });
        Palettes { scanning: spawned.ok().map(|_| scanning), ..Default::default() }
    }
}

/// What the brushes would put down, as one fixed place of the interface shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum InHand {
    /// A tile, by its id.
    Tile(String),
    /// An entity prototype: its index in [`Session::prototypes`], the name the palette shows
    /// it by and its id.
    Entity { index: usize, name: String, id: String },
    /// A decal, by its prototype's id.
    Decal(String),
}

impl InHand {
    /// The name it is shown by.
    pub fn name(&self) -> &str {
        match self {
            InHand::Tile(id) | InHand::Decal(id) => id,
            InHand::Entity { name, .. } => name,
        }
    }

    /// What kind of thing it is, in a word.
    pub fn kind(&self) -> &'static str {
        match self {
            InHand::Tile(_) => "Tile",
            InHand::Entity { .. } => "Entity",
            InHand::Decal(_) => "Decal",
        }
    }
}

/// The name a palette shows a prototype by: its name, or its id if it has none, with its
/// suffix after it in brackets.
pub fn shown_name(prototype: &Prototype) -> String {
    let name = if prototype.name.is_empty() { &prototype.id } else { &prototype.name };
    match &prototype.suffix {
        Some(suffix) if !suffix.is_empty() => format!("{name} ({suffix})"),
        _ => name.clone(),
    }
}

impl Session {
    /// What the palettes hold, for the interface to show in one place. None before the engine
    /// has said what tiles there are, and for a prototype it does not know.
    pub fn in_hand(&self) -> Option<InHand> {
        match &self.palette {
            Palette::None => None,
            Palette::Tile(id) => Some(InHand::Tile(id.clone())),
            Palette::Decal(id) => Some(InHand::Decal(id.clone())),
            Palette::Entity(index) => {
                let prototype = self.prototypes.get(*index)?;
                Some(InHand::Entity { index: *index, name: shown_name(prototype), id: prototype.id.clone() })
            }
        }
    }

    /// Whether a choice was made away from the palettes since the last call, as by the pick
    /// tool, so that the palette of its kind should show it. True once for each such choice.
    pub fn take_reveal(&mut self) -> bool {
        std::mem::take(&mut self.reveal)
    }

    /// The entity palette's tree, once the fork's files have been read and the engine has
    /// listed the prototypes.
    pub fn entity_tree(&self) -> Option<&EntityTree> {
        self.palettes.tree.as_ref()
    }

    /// The prototypes a search of the entity palette finds, as indices into
    /// [`Self::prototypes`]: over the tree once it is made, in its order, and over every
    /// prototype that can be placed till then.
    pub fn entity_search(&self, words: &[&str]) -> Vec<usize> {
        match self.entity_tree() {
            Some(tree) => tree.search(self.prototypes(), words),
            None => self.placeable_matching(words),
        }
    }

    /// The entity palette's category of a prototype by its id, as "Structures/Walls", which the
    /// selection panel shows as the web editor's Entity Info does. A prototype whose file was
    /// not read is "Other", as in the tree. None until the fork's files have been read.
    pub fn palette_category(&self, id: &str) -> Option<&str> {
        let categories = self.palettes.categories.as_ref()?;
        Some(categories.get(id).map_or("Other", String::as_str))
    }

    /// Takes the categories if the fork's files have been read since the last call.
    pub(super) fn poll_categories(&mut self) -> bool {
        let Some(scanning) = self.palettes.scanning.as_ref() else { return false };
        match scanning.try_recv() {
            Ok(categories) => {
                self.palettes.scanning = None;
                self.categories_arrived(categories);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.palettes.scanning = None;
                false
            }
        }
    }

    /// Takes the category of each prototype by its id, and makes the tree if the prototypes
    /// are known.
    pub(super) fn categories_arrived(&mut self, categories: HashMap<String, String>) {
        self.palettes.categories = Some(categories);
        self.build_tree();
    }

    /// Makes the entity palette's tree again from the prototypes and their categories, once
    /// both are known.
    pub(super) fn build_tree(&mut self) {
        self.palettes.tree = match &self.palettes.categories {
            Some(categories) if !self.prototypes.is_empty() => Some(EntityTree::build(&self.prototypes, categories)),
            _ => None,
        };
    }
}
