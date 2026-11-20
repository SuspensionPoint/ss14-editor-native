//! An editing session, with no interface attached: the engine on its own thread, the open
//! document as the engine reports it, the camera, the palettes, the tools and the layer switches.
//!
//! This is what an interface toolkit sits on top of. A toolkit draws the panels, hands mouse
//! and keyboard input here, calls [`Session::pump`] before drawing a frame, and draws the map
//! with [`crate::renderer::MapRenderer`]. Every edit is a request to the engine; what the
//! session shows is what the engine answered.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::air::{AirAnswer, AirView, GridAirJson, GridsOf};
use crate::backdrop::{self, Backdrop, Thumb};
use crate::drawlist::{quad_pixel_at, DrawLists, FLOATS_PER_QUAD, PREVIEWS, WHITE_SOURCE, WHITE_TEXTURE};
use crate::engine::Engine;
use crate::fields::FieldsAnswer;
use crate::forks::cache::Prepared;
use crate::links::{EntityPorts, LinkMirror, LinksAnswer, Ports, PortsJson};
use crate::loading::{Loading, Step};
use crate::locale::Locale;
use crate::model::{cells_between, share_names, Container, DecalPrototype, Delta, Doc, EngineTiming, Entity, EntityContainers, Prototype, SceneJson, SPACE};
use crate::draw_depths::{DepthGroups, DrawDepth};
use crate::renderer::{group, group_of, Camera, GridLines};
use crate::textures::BakedTextures;

mod air;
mod air_fix;
mod animate;
mod backdrops;
mod engine_thread;
mod entity;
mod favourites;
mod find;
mod grids;
mod palettes;
mod problems;
mod tools;
mod zoom;

use engine_thread::{engine_thread, FromEngine, Reply, ToEngine};
pub use air_fix::{FixAirDialog, FixPreview, ADD_FIRST, FIX_AIR, FIX_AIR_DOES};
pub use backdrops::{BackdropEntry, GAME_DEFAULT};
pub use engine_thread::Timing;
pub use entity::{ComponentEntry, ComponentRow, FieldRow, RESERVED_COMPONENTS};
pub use favourites::{FavouriteRow, FavouriteState, FAVOURITE_KEYS};
pub use crate::forks::settings::{Favourite, FavouriteKind};
pub use grids::{GridRow, GRID_FILE, MAP_GROUND};
pub use palettes::{shown_name, InHand};
pub use problems::PLATING;
pub use tools::{
    arrows_reach_map, decal_colour_bytes, decal_colour_text, escape_to, Arrow, Button, CableKind, Clipboard, CopiedTile, DecalField, DecalGroup, DecalSettings, DecalSummary, EntityLinkRows, EscapeTo, Held, HoverLook, LinkChooser, LinkKind, LinkLine, MenuAction, MenuEntry, Outline,
    OutlineStyle, PickList, Pickable, is_room_wall, needs_floor, PipeNetwork, PipePieces, PlacePreview, PointerLook, StackList, Tool, ToolEntry, CONTROLS, PIPE_COLOURS,
};
use tools::{Copying, Decaling, Drag, Flooring, Hint, Laying, Linking, Painting, Pasting, Picking, Placing, Prefabs, Region, Stepping, Turning};

/// Where a session's pack and textures come from.
enum Content {
    /// The pack the command line gave. The interface opens the textures it gave.
    Given(String),
    /// The cache's, made first where missing.
    Cached(Prepared),
}

/// A tile the palette offers, as the engine resolved it.
#[derive(Clone, Debug)]
pub struct Tile {
    pub id: String,
    /// The tile's image, a rooted resource path, if it has one.
    pub sprite: Option<String>,
    /// Whether the game draws it turned and mirrored by a tile's rotation byte. Only such a
    /// tile has its byte turned when a region is turned.
    pub allows_rotation: bool,
    /// Whether a crowbar lifts it (the game's canCrowbar), as trash left on it by an open
    /// disposal pipe does too.
    pub can_crowbar: bool,
}

/// What the brushes put down: the one thing chosen in the palettes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Palette {
    /// Nothing is chosen, as before the engine has said what tiles there are.
    None,
    /// A tile's id.
    Tile(String),
    /// An index into [`Session::prototypes`].
    Entity(usize),
    /// A decal prototype's id.
    Decal(String),
}

/// The layer switches of the web editor's Layers panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers {
    pub subfloor: bool,
    pub floor_objects: bool,
    pub structures: bool,
    pub objects: bool,
    pub doors: bool,
    pub markers: bool,
    pub decals: bool,
    /// Starlight's plumbing: its ducts and plumbing machines, which the engine tells apart.
    pub plumbing: bool,
    /// Disposal tubes under the floor. The disposal units on it stay with the structures.
    pub disposals: bool,
    /// Show what lies under floor tiles (the web editor's "T-Ray").
    pub t_ray: bool,
    /// The web editor's Show Entities: off hides every entity whatever its layer's switch,
    /// and leaves tiles and decals.
    pub entities: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers { subfloor: true, floor_objects: true, structures: true, objects: true, doors: true, markers: true, decals: true, plumbing: true, disposals: true, t_ray: true, entities: true }
    }
}

impl Layers {
    /// The switched-off layers as the renderer takes them: one bit per [`group`]. Hiding a
    /// layer is the GPU leaving its rectangles out; the engine is not asked. The one switch
    /// that is the engine's is `t_ray`, since what shows under a floor is the game's decision.
    pub fn hidden_mask(&self) -> u32 {
        let bit = |shown: bool, group: u32| if shown { 0 } else { 1 << group };
        let entity = |shown: bool, group: u32| bit(shown && self.entities, group);
        entity(self.subfloor, group::SUBFLOOR)
            | entity(self.floor_objects, group::FLOOR_OBJECTS)
            | entity(self.structures, group::STRUCTURES)
            | entity(self.objects, group::OBJECTS)
            | entity(self.doors, group::DOORS)
            | entity(self.markers, group::MARKERS)
            | entity(self.plumbing, group::PLUMBING)
            | entity(self.disposals, group::DISPOSALS)
            | bit(self.decals, group::DECALS)
    }
}

/// What is known of the open map's backdrop once it has been read.
#[derive(Clone, Debug)]
pub struct BackdropInfo {
    /// The game's name for it.
    pub name: String,
    pub layers: usize,
    /// Layers the game has that are not shown.
    pub skipped: u32,
    /// Whether it drifts by itself, so that frames are wanted while nothing is done.
    pub moves: bool,
    /// How long the game's generator took to paint layers for it, if it had to.
    pub paint_ms: u64,
    /// Whether the map names it, and is not left to the game's default.
    pub chosen: bool,
    /// Whether the map is a planet's, which the game draws no backdrop behind.
    pub planet: bool,
}

/// What a performance display shows. What a frame costs is the toolkit's to measure.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Performance {
    pub entities: usize,
    /// Rectangles of the open document's draw list.
    pub rectangles: usize,
    /// The engine's own time for the last change, and the time from asking to showing it.
    pub last_engine_ms: Option<f64>,
    pub last_change_ms: Option<f64>,
}

impl Performance {
    /// The lines of the display, given what a frame costs in milliseconds, if the toolkit has
    /// measured one.
    pub fn lines(&self, frame_ms: Option<f64>) -> [String; 4] {
        let frame = frame_ms.map_or("-".to_owned(), |ms| format!("{ms:.1} ms"));
        let engine = match (self.last_engine_ms, self.last_change_ms) {
            (Some(engine), Some(change)) => format!("{engine:.1} ms   last edit, of {change:.1} ms from asking to showing"),
            _ => "-".to_owned(),
        };
        [
            format!("frame      {frame}"),
            format!("engine     {engine}"),
            format!("drawn      {} rectangles", thousands(self.rectangles)),
            format!("entities   {}", thousands(self.entities)),
        ]
    }
}

/// The verbs edits are named with, and how a status bar says them once the edit is made.
const DONE: [(&str, &str); 23] = [
    ("Add", "Added"),
    ("Auto-link", "Auto-linked"),
    ("Build", "Built"),
    ("Delete", "Deleted"),
    ("Edit", "Edited"),
    ("Erase", "Erased"),
    ("Fill", "Filled"),
    ("Lay", "Laid"),
    ("Link", "Linked"),
    ("Move", "Moved"),
    ("Paint", "Painted"),
    ("Paste", "Pasted"),
    ("Place", "Placed"),
    ("Recolour", "Recoloured"),
    ("Redo", "Redone"),
    ("Remove", "Removed"),
    ("Rename", "Renamed"),
    ("Reset", "Reset"),
    ("Rotate", "Rotated"),
    ("Turn", "Turned"),
    ("Undo", "Undone"),
    ("Unlink", "Unlinked"),
    ("Cut", "Cut"),
];

/// What a status bar says once the engine has made an edit, short as the web editor's
/// messages: the edit's name with its verb in the past, so "Pasted 12 entities" for "Paste 12
/// entities". A name that begins with no verb of [`DONE`] is said as it is.
pub fn edit_message(what: &str) -> String {
    let (verb, rest) = what.split_once(' ').unwrap_or((what, ""));
    match DONE.iter().find(|(now, _)| *now == verb) {
        Some((_, done)) if rest.is_empty() => (*done).to_owned(),
        Some((_, done)) => format!("{done} {rest}"),
        None => what.to_owned(),
    }
}

/// A count with a comma before each group of three digits.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

pub struct Session {
    to_engine: Sender<ToEngine>,
    from_engine: Receiver<FromEngine>,
    /// How many commands have been sent to the engine's thread, and how many it has finished.
    sent: u64,
    handled: Arc<AtomicU64>,
    pub started: Instant,
    pub fork_repo: String,
    pub lists: DrawLists,
    pub document: Option<i32>,
    /// The open document, as the engine reported it.
    pub doc: Option<Doc>,
    /// The grid the tile tools last worked on: its uid, and where its origin is in the world.
    pub grid: (i32, f32, f32),
    pub camera: Camera,
    /// The zoom the wheel asked for that the frames have yet to give.
    glide: zoom::ZoomGlide,
    /// The tool in hand. Only [`Self::set_tool`] changes it.
    tool: Tool,
    pub tiles: Vec<Tile>,
    tile_sprites: HashMap<String, Option<String>>,
    /// Every entity prototype the engine knows, in the order of their ids. Those the game keeps
    /// out of its spawn menu are among them, and a palette leaves them out. It is set once, when
    /// the engine is ready, and never reordered: the palette, the preview caches and the
    /// engine's preview keys hold indices into it.
    prototypes: Vec<Prototype>,
    /// Where each prototype is in that list, by its id.
    prototype_indices: HashMap<String, usize>,
    /// Every decal prototype the engine knows, in the order of their ids. Those the game keeps
    /// out of its own decal menu are among them.
    pub decal_prototypes: Vec<DecalPrototype>,
    /// What the engine told of device ports once, when it was ready.
    ports: Ports,
    /// The fork's English text, for the names of ports, read on a thread as the session starts.
    locale: Locale,
    /// What is chosen in the palettes. Choosing a tile takes the place of a chosen entity, and
    /// the other way round. The engine being ready chooses the first tile. After that only
    /// [`Self::choose_tile`], [`Self::choose_entity`] and [`Self::choose_decal`] change it.
    palette: Palette,
    /// Whether a choice was made away from the palettes that they have not yet been told of.
    reveal: bool,
    /// The entity palette's tree of categories.
    palettes: palettes::Palettes,
    /// The favourites, kept in the settings file under the fork.
    favourites: favourites::Favourites,
    /// Whether a tile is laid under new walls and which, and the Room tool's wall, kept in
    /// the settings file under the fork.
    flooring: Flooring,
    /// Selected entities, by uid.
    pub selection: Vec<i32>,
    /// Selected decals, by their grid's uid and their id on it. The select tool selects them
    /// beside entities.
    pub selected_decals: Vec<(i32, i32)>,
    /// The component data of the one selected entity, once the engine has sent it.
    pub details: Option<(i32, Value)>,
    /// What is kept of the one selected entity while it is edited: text, drafts, the last
    /// refusal and its containers.
    inspecting: entity::Inspecting,
    /// The number of the last grid added, until the engine has answered it.
    grid_adding: Option<u64>,
    pub layers: Layers,
    /// The lines between cells, and the game's backdrop behind the map.
    pub show_grid: bool,
    pub show_backdrop: bool,
    /// Whether the device links are drawn as lines over the map, the web editor's Connections.
    pub show_connections: bool,
    /// The air overlay, and which of its views is shown.
    pub show_air: bool,
    pub air_view: AirView,
    /// The overlay's rectangles as last built, and what they were built from.
    air_drawn: air::AirDrawn,
    /// The Problems overlay: its switch and its badges.
    problems: problems::Problems,
    /// Whether animated sprites move (View > Animate sprites).
    animate: animate::Animate,
    /// The open map's backdrop, once it has been read; a map the game gives none has no layers.
    pub backdrop: Option<BackdropInfo>,
    /// A backdrop that has arrived and has not been handed to whoever draws yet.
    backdrop_arrived: Option<Arc<Backdrop>>,
    /// The backdrop gallery.
    gallery: backdrops::Gallery,
    /// The Fix grid atmos dialog.
    fixing_air: air_fix::FixingAir,
    /// Whether the engine was last told to show what lies under floors; None before the first time.
    sent_t_ray: Option<bool>,
    /// The last message for a status bar. Only [`Self::set_status`] changes it, and a status
    /// bar shows [`Self::status_line`].
    status: String,
    /// What a press last asked the mapper to choose, until a choice answers it or a message
    /// takes its place.
    hint: Option<Hint>,
    /// The load under way for a loading screen, or the last one, until the interface lets it go.
    pub loading: Option<Loading>,
    /// Whether the engine's thread stopped with an error. Nothing asked of it is answered.
    engine_gone: bool,
    pub engine_ready_ms: Option<u64>,
    pub warm_up_ms: Option<u64>,
    pub map_shown_ms: Option<u64>,
    /// How long the last edit took from asking until its draw list was here, in milliseconds.
    pub last_change_ms: Option<f64>,
    /// How long the engine itself worked on that edit, in milliseconds.
    pub last_engine_ms: Option<f64>,
    /// Where the time of the last edit went, for the performance display. The status bar
    /// says only what the edit did.
    pub last_breakdown: Option<String>,
    /// A message for the status bar to give in the place of [`edit_message`] when the edit
    /// of this number is answered.
    edit_says: Option<(u64, String)>,
    /// How many edits and layer changes have come back from the engine.
    pub changes: u64,
    /// Where the cursor is on the map, if it is over it.
    pub hover: Option<(f32, f32)>,
    /// The modifiers and Space, which change what the mouse does while they are down. The
    /// interface sets it each frame, before it hands on that frame's presses.
    pub held: Held,
    /// What the last place or move sent, with what was held, and where the engine said the
    /// thing then was. The performance display shows them, so that a mapper can see whether
    /// Shift reached the press and where the position went.
    pub last_put: Option<String>,
    pub last_put_answer: Option<String>,
    /// Whether a dialog is open over the editor. The interface sets it each frame, and the
    /// ghosts of what a press would put down are not shown while it is.
    pub dialog_open: bool,
    drag: Drag,
    /// What the tile tools and the place tool keep from one drag to the next.
    painting: Painting,
    placing: Placing,
    /// What decals are placed with.
    decaling: Decaling,
    /// What the cable and pipe tools lay.
    laying: Laying,
    /// The link tool's source, its chooser and the ports the engine told.
    linking: Linking,
    /// The hold of R that the entity tools turn things with.
    turning: Turning,
    /// The list of things on one cell that the select tool's wheel steps through.
    stepping: Stepping,
    /// The list of what could be picked on one cell that the pick tool's wheel steps through.
    picking: Picking,
    /// The cells the area tool has selected.
    region: Option<Region>,
    /// Where on the map the region's menu was opened, while it is open.
    region_menu: Option<(f32, f32)>,
    /// What was last copied, and a copy asked for and not yet answered. A map opened keeps both.
    clipboard: Option<Clipboard>,
    copying: Option<Copying>,
    /// The select tool's paste mode.
    pasting: Pasting,
    /// The prefabs folder, its listing, and the prefab on the clipboard.
    prefabs: Prefabs,
    /// The number of the last move or turn of a region, until the engine has answered it.
    /// The mirror is behind the engine till then, and a region is moved, turned and deleted
    /// by what the mirror holds.
    region_waiting: Option<u64>,
    /// The number of the last edit, undo or redo sent, and the highest number the engine has
    /// answered. While the first is above the second the mirror is behind the engine, and the
    /// arrow keys move nothing (`nudge.rs`).
    last_edit: u64,
    last_answered: u64,
    next_seq: u64,
    next_stroke: u64,
    textures: Option<Arc<BakedTextures>>,
    /// Textures made or opened on the engine's thread that the interface has not taken yet.
    textures_arrived: Option<Arc<BakedTextures>>,
    /// The folder the open dialog starts in, when it is not the fork's `Resources/Maps`.
    maps_folder: Option<PathBuf>,
    /// Decoded frames, for telling a click on a sprite from a click on its empty corner.
    frames: HashMap<u32, Option<(u32, u32, Vec<u8>)>>,
    needs_fit: bool,
    /// The cell the last jump to a finding outlines, for a while.
    highlight: Option<find::Highlight>,
}

impl Session {
    /// Starts the engine and opens a map, or an empty one. `wake` is called from the engine's
    /// thread whenever there is something new, so the interface can draw a frame. The pack is
    /// given, and the interface opens the textures itself.
    pub fn start(engine_folder: PathBuf, fork_repo: String, kernel_pack: String, map: Option<String>, wake: impl Fn() + Send + Sync + 'static) -> Session {
        Session::launch(engine_folder, fork_repo, Content::Given(kernel_pack), map, Arc::new(wake))
    }

    /// Starts the engine with the fork's pack and textures from the cache, made first if they
    /// are not there (`forks::check` worked out which). The textures come to the interface by
    /// [`Self::take_textures`] before the map is drawn.
    pub fn start_prepared(engine_folder: PathBuf, fork_repo: String, prepared: Prepared, map: Option<String>, wake: impl Fn() + Send + Sync + 'static) -> Session {
        Session::launch(engine_folder, fork_repo, Content::Cached(prepared), map, Arc::new(wake))
    }

    fn launch(engine_folder: PathBuf, fork_repo: String, content: Content, map: Option<String>, wake: Arc<dyn Fn() + Send + Sync>) -> Session {
        let (to_engine, commands) = mpsc::channel::<ToEngine>();
        let (replies, from_engine) = mpsc::channel::<FromEngine>();
        let fork = fork_repo.clone();
        let done = Arc::new(AtomicU64::new(0));
        // Made before the thread, so that its clock starts before any of the engine's work.
        let mut session = Session::with_channels(to_engine, from_engine, done.clone(), fork_repo);
        let before = match &content {
            Content::Given(_) => Vec::new(),
            Content::Cached(prepared) => [(Step::Pack, &prepared.pack_stage), (Step::Bake, &prepared.textures_stage)]
                .into_iter()
                .filter_map(|(step, words)| words.clone().map(|words| (step, words)))
                .collect(),
        };
        session.loading = Some(Loading::first_start_after(before, map.as_deref(), session.started));
        session.palettes = palettes::Palettes::scan(&fork, wake.clone());
        session.locale = Locale::read(&fork, wake.clone());
        session.favourites = favourites::Favourites::load(crate::forks::settings::Settings::file(), &fork);
        session.problems = problems::Problems::load(crate::forks::settings::Settings::file(), &fork);
        session.animate = animate::Animate::load(crate::forks::settings::Settings::file());
        session.flooring = Flooring::load(crate::forks::settings::Settings::file(), &fork);
        std::thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                let send: Reply = Arc::new(move |reply| {
                    let _ = replies.send(reply);
                    wake();
                });
                let pack = match content {
                    Content::Given(pack) => Ok(pack),
                    Content::Cached(prepared) => engine_thread::prepare(&engine_folder, &fork, &prepared, &send),
                };
                if let Err(error) = pack.and_then(|pack| engine_thread(&engine_folder, &fork, &pack, map, commands, &done, &send)) {
                    send(FromEngine::Failed(error));
                }
            })
            .expect("no engine thread");
        session
    }

    /// A session around the two ends of the channels to an engine's thread, with nothing open
    /// yet. It counts on that thread opening the first map without being asked, and so starts
    /// with one command sent.
    fn with_channels(to_engine: Sender<ToEngine>, from_engine: Receiver<FromEngine>, handled: Arc<AtomicU64>, fork_repo: String) -> Session {
        Session {
            to_engine,
            from_engine,
            // The engine's thread opens the first map without being asked.
            sent: 1,
            handled,
            started: Instant::now(),
            fork_repo,
            lists: DrawLists::default(),
            document: None,
            doc: None,
            grid: (0, 0.0, 0.0),
            camera: Camera { x: 0.0, y: 0.0, zoom: 1.0 },
            glide: zoom::ZoomGlide::default(),
            tool: Tool::Paint,
            tiles: Vec::new(),
            tile_sprites: HashMap::new(),
            prototypes: Vec::new(),
            prototype_indices: HashMap::new(),
            decal_prototypes: Vec::new(),
            ports: Ports::default(),
            locale: Locale::default(),
            palette: Palette::None,
            reveal: false,
            palettes: palettes::Palettes::default(),
            favourites: favourites::Favourites::default(),
            flooring: Flooring::default(),
            selection: Vec::new(),
            selected_decals: Vec::new(),
            details: None,
            inspecting: entity::Inspecting::default(),
            grid_adding: None,
            layers: Layers::default(),
            show_grid: true,
            show_backdrop: true,
            show_connections: false,
            show_air: false,
            air_view: AirView::default(),
            air_drawn: air::AirDrawn::default(),
            problems: problems::Problems::default(),
            animate: animate::Animate::default(),
            backdrop: None,
            backdrop_arrived: None,
            gallery: backdrops::Gallery::default(),
            fixing_air: air_fix::FixingAir::default(),
            sent_t_ray: None,
            status: "Engine starting".into(),
            hint: None,
            loading: None,
            engine_gone: false,
            engine_ready_ms: None,
            warm_up_ms: None,
            map_shown_ms: None,
            last_change_ms: None,
            last_engine_ms: None,
            last_breakdown: None,
            edit_says: None,
            changes: 0,
            hover: None,
            held: Held::default(),
            last_put: None,
            last_put_answer: None,
            dialog_open: false,
            drag: Drag::None,
            painting: Painting::default(),
            placing: Placing::default(),
            decaling: Decaling::default(),
            laying: Laying::default(),
            linking: Linking::default(),
            turning: Turning::default(),
            stepping: Stepping::default(),
            picking: Picking::default(),
            region: None,
            region_menu: None,
            region_waiting: None,
            last_edit: 0,
            last_answered: 0,
            clipboard: None,
            copying: None,
            pasting: Pasting::default(),
            prefabs: Prefabs::default(),
            next_seq: 1,
            next_stroke: 1,
            textures: None,
            textures_arrived: None,
            maps_folder: None,
            frames: HashMap::new(),
            needs_fit: false,
            highlight: None,
        }
    }

    fn send(&mut self, command: ToEngine) {
        self.sent += 1;
        let _ = self.to_engine.send(command);
    }

    /// True when the engine has finished everything it was asked. Its answers may still be
    /// waiting for the next [`Self::pump`].
    pub fn idle(&self) -> bool {
        self.handled.load(Ordering::Acquire) >= self.sent
    }

    /// The one line a status bar shows: a hint that stands, or else the last message.
    pub fn status_line(&self) -> &str {
        match self.hint {
            Some(hint) => hint.text(),
            None => &self.status,
        }
    }

    /// Takes a message for the status bar, in the place of a hint that stands.
    fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.hint = None;
    }

    /// The tool in hand.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// What is chosen in the palettes.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The tile the palette holds, if it holds a tile.
    pub fn chosen_tile(&self) -> Option<&str> {
        match &self.palette {
            Palette::Tile(id) => Some(id),
            _ => None,
        }
    }

    /// Every entity prototype the engine knows, in the order of their ids. An entity palette
    /// lists those that are [`Prototype::placeable`], each by its index in this list.
    pub fn prototypes(&self) -> &[Prototype] {
        &self.prototypes
    }

    /// The prototype of this id, if the engine knows one. One hidden from the spawn menu is
    /// found as any other, since a map may hold entities of it.
    pub fn prototype(&self, id: &str) -> Option<&Prototype> {
        self.prototype_indices.get(id).and_then(|&index| self.prototypes.get(index))
    }

    /// The prototypes an entity palette offers for a search: those a mapper can place whose id
    /// or name has every one of the words in it, in either case, as indices into
    /// [`Self::prototypes`] in its order. No words is all of them.
    pub fn placeable_matching(&self, words: &[&str]) -> Vec<usize> {
        let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
        self.prototypes
            .iter()
            .enumerate()
            .filter(|(_, prototype)| prototype.placeable() && words.iter().all(|word| prototype.search.contains(word.as_str())))
            .map(|(index, _)| index)
            .collect()
    }

    /// Takes the engine's list of prototypes, and notes where each one is by its id.
    fn set_prototypes(&mut self, prototypes: Vec<Prototype>) {
        self.prototype_indices = prototypes.iter().enumerate().map(|(index, prototype)| (prototype.id.clone(), index)).collect();
        self.prototypes = prototypes;
        self.build_tree();
    }

    /// The prototype the palette holds, as an index into [`Self::prototypes`], if it holds an entity.
    pub fn chosen_entity(&self) -> Option<usize> {
        match self.palette {
            Palette::Entity(index) => Some(index),
            _ => None,
        }
    }

    /// The prototype the palette holds, if it holds an entity.
    pub fn chosen_prototype(&self) -> Option<&Prototype> {
        self.chosen_entity().and_then(|index| self.prototypes.get(index))
    }

    /// The numbers a performance display shows, as they are now.
    pub fn performance(&self) -> Performance {
        Performance {
            entities: self.doc.as_ref().map_or(0, |doc| doc.entities.len()),
            rectangles: self.document.and_then(|id| self.lists.documents.get(&id)).map_or(0, |list| list.quad_count()),
            last_engine_ms: self.last_engine_ms,
            last_change_ms: self.last_change_ms,
        }
    }

    /// The size in cells of the box around the active grid's tiles, across and up. None
    /// without a document, or with a grid that has no tiles.
    pub fn grid_size(&self) -> Option<(i32, i32)> {
        let doc = self.doc.as_ref()?;
        let grid = doc.grid(doc.active_grid)?;
        let mut cells = grid.tiles.keys();
        let first = *cells.next()?;
        let (l, b, r, t) = cells.fold((first.0, first.1, first.0, first.1), |(l, b, r, t), &(x, y)| (l.min(x), b.min(y), r.max(x), t.max(y)));
        Some((r - l + 1, t - b + 1))
    }

    /// A backdrop that has arrived since the last call, for whoever draws to put on the GPU
    /// (`MapRenderer::set_backdrop`). An empty one says the map before had one and this map
    /// has none, or none yet. While the backdrop gallery is open it may be the one under the mouse.
    pub fn take_backdrop(&mut self) -> Option<Arc<Backdrop>> {
        self.backdrop_arrived.take()
    }

    /// Where the lines between cells go: along the grid the tools last worked on.
    pub fn grid_lines(&self) -> GridLines {
        match self.doc.as_ref().and_then(|doc| doc.grid(self.grid.0)) {
            Some(grid) => GridLines { origin: (grid.x, grid.y), rotation: grid.rotation },
            None => GridLines { origin: (self.grid.1, self.grid.2), rotation: 0.0 },
        }
    }

    /// Gives the session the baked textures, so that a click can tell a sprite's picture from
    /// the empty parts of its frame. Without them the whole frame counts.
    pub fn set_textures(&mut self, textures: Arc<BakedTextures>) {
        self.textures = Some(textures);
    }

    /// The textures of a session started with [`Self::start_prepared`], once, when they have
    /// been made or opened. The session has them already. Whoever draws puts them in the place
    /// of the empty ones it started with.
    pub fn take_textures(&mut self) -> Option<Arc<BakedTextures>> {
        self.textures_arrived.take()
    }

    /// Takes in whatever the engine has sent since the last call. Returns true if anything
    /// changed. `view` is the map area's size in device pixels, for fitting a newly opened map
    /// into it.
    pub fn pump(&mut self, view: (u32, u32)) -> bool {
        let mut changed = self.poll_categories();
        changed |= self.locale.poll();
        while let Ok(reply) = self.from_engine.try_recv() {
            changed = true;
            match reply {
                FromEngine::Progress(text) => self.set_status(text),
                FromEngine::Reached(step) => self.reached(step),
                FromEngine::Textures(textures) => {
                    self.textures = Some(textures.clone());
                    self.textures_arrived = Some(textures);
                    self.frames.clear();
                }
                FromEngine::NotOpened(error) => {
                    if let Some(loading) = self.loading.as_mut() {
                        loading.fail(&error, Instant::now());
                    }
                    self.set_status(format!("Not opened: {}", error.lines().next().unwrap_or("")));
                }
                FromEngine::Ready { document_kernel_ms, view_kernel_ms, warm_up_ms, tiles, prototypes, decal_prototypes, ports, depths } => {
                    self.ports = ports;
                    self.lists.depths = depths;
                    self.engine_ready_ms = Some(self.started.elapsed().as_millis() as u64);
                    self.warm_up_ms = Some(warm_up_ms);
                    self.set_status(format!(
                        "Engine ready (document kernel {document_kernel_ms} ms, view kernel {view_kernel_ms} ms, warm-up {warm_up_ms} ms). Opening the map"
                    ));
                    self.palette = tiles.iter().find(|t| t.id == "FloorSteel").or(tiles.first()).map_or(Palette::None, |t| Palette::Tile(t.id.clone()));
                    self.tile_sprites = tiles.iter().map(|tile| (tile.id.clone(), tile.sprite.clone())).collect();
                    self.tiles = tiles;
                    self.set_prototypes(prototypes);
                    self.decal_prototypes = decal_prototypes;
                    self.reached(Step::Prototypes);
                }
                FromEngine::Opened { scene, list, timing, path, links } => {
                    if let Some(old) = self.document.take() {
                        self.lists.documents.remove(&old);
                    }
                    let entities = scene.entities.len();
                    let mut doc = Doc::from_scene(*scene, path.map(PathBuf::from));
                    if self.lists.apply(&list).is_ok() {
                        self.document = Some(doc.id);
                        self.needs_fit = true;
                    }
                    self.grid = doc.grid(doc.active_grid).map_or((0, 0.0, 0.0), |grid| (grid.uid, grid.x, grid.y));
                    match links {
                        Ok(links) => {
                            doc.links = Some(LinkMirror::from_answer(links));
                            self.set_status(format!("{}: {entities} entities, opened by the engine in {:.0} ms", doc.source, timing.total_ms));
                        }
                        Err(error) => self.set_status(format!("{}: {entities} entities. The engine couldn't list the links: {}", doc.source, error.lines().next().unwrap_or(""))),
                    }
                    self.doc = Some(doc);
                    self.selection.clear();
                    self.selected_decals.clear();
                    self.details = None;
                    self.inspect(None);
                    self.grid_adding = None;
                    self.painting.clear();
                    self.picking.close();
                    self.drag = Drag::None;
                    self.region = None;
                    self.region_waiting = None;
                    self.last_answered = self.last_edit;
                    self.highlight = None;
                    self.linking = Linking::default();
                    // The last map's backdrop goes; this one's is on its way.
                    self.backdrop = None;
                    self.backdrops_reset();
                    self.fix_air_reset();
                    self.map_shown_ms = Some(self.started.elapsed().as_millis() as u64);
                    self.reached(Step::Picture);
                }
                FromEngine::Changed { list, delta, what, seq, asked, timing } => {
                    // The answer has been ready since `timing.ready`; this is the interface's
                    // next frame picking it up.
                    let waited = timing.ready.map_or(0.0, |ready| ready.elapsed().as_secs_f64() * 1000.0);
                    let reading = Instant::now();
                    if let Some(list) = list {
                        let _ = self.lists.apply(&list);
                    }
                    // What of the selected entity to ask for again, and the first thing made.
                    let (mut again, mut created, mut fresh) = ((false, false), None, false);
                    if let Some(delta) = delta {
                        again = self.entity_changes(&delta, what);
                        fresh = delta.details.iter().any(|details| self.selection == [details.uid]);
                        created = delta.created.first().copied();
                        if let (true, Some(entity)) = (what == "Place entity" || what == "Move", delta.entities.first()) {
                            let anchored = if entity.anchored { ", anchored" } else { "" };
                            self.last_put_answer = Some(format!("engine has {} {} at ({:.3}, {:.3}) of {}{anchored}", entity.prototype, entity.uid, entity.x, entity.y, entity.parent));
                        }
                        if let Some(doc) = self.doc.as_mut() {
                            // The components shown for the selected entity, if the command changed them.
                            if let Some(fresh) = delta.details.iter().find(|details| self.selection == [details.uid]) {
                                self.details = Some((fresh.uid, serde_json::from_str(&fresh.components).unwrap_or(Value::Null)));
                            }
                            doc.apply(*delta);
                            self.selection.retain(|uid| doc.entities.contains_key(uid));
                            self.selected_decals.retain(|&(grid, id)| doc.grid(grid).is_some_and(|grid| grid.decals.contains_key(&id)));
                        }
                    }
                    self.region_grid_checked();
                    self.linking_checked();
                    self.last_answered = self.last_answered.max(seq);
                    self.answered(seq);
                    self.region_answered(seq, false);
                    self.entity_answered(seq, None);
                    self.link_answered(seq, None);
                    self.grids_answered(seq, created);
                    // Only for an entity still selected: one that is gone is asked nothing.
                    self.ask_again(again.0, again.1);
                    // New data of the selected entity: its forms are asked for again too.
                    if fresh && !again.0 {
                        self.ask_fields();
                    }
                    let read = reading.elapsed().as_secs_f64() * 1000.0;
                    let ms = asked.elapsed().as_secs_f64() * 1000.0;
                    self.last_change_ms = Some(ms);
                    self.last_engine_ms = Some(timing.engine);
                    self.changes += 1;
                    self.last_breakdown = Some(format!(
                        "{what}: {ms:.1} ms from asking to this frame = {:.1} waiting for the engine's thread + {:.1} engine (view {:.0}, recording {:.0}) + {waited:.1} waiting for a frame + {read:.1} reading the answer",
                        timing.queued, timing.engine, timing.view, timing.record
                    ));
                    let message = match self.edit_says.take_if(|(waiting, _)| *waiting <= seq) {
                        Some((waiting, says)) if waiting == seq => says,
                        _ => edit_message(what),
                    };
                    self.set_status(message);
                }
                FromEngine::Refused { seq, what, error } => {
                    self.last_answered = self.last_answered.max(seq);
                    self.answered(seq);
                    self.region_answered(seq, true);
                    self.copy_refused(seq);
                    self.entity_answered(seq, Some(&error));
                    self.link_answered(seq, Some(&error));
                    self.grids_answered(seq, None);
                    self.backdrop_refused(seq);
                    self.edit_says.take_if(|(waiting, _)| *waiting <= seq);
                    self.set_status(format!("{what}: the engine refused: {}", error.lines().next().unwrap_or("")));
                }
                FromEngine::Saved { path, ok, failure, milliseconds, bytes, revision } => {
                    if ok {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.saved_revision = revision;
                            doc.path = Some(PathBuf::from(&path));
                            doc.source = Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                        }
                        self.set_status(format!("Saved {path}: {bytes} bytes, written by the engine in {milliseconds:.0} ms"));
                    } else {
                        self.set_status(format!("Not saved: {}", failure.unwrap_or_else(|| "the engine gave no reason".into())));
                    }
                }
                FromEngine::Previews { list, keys, directions } => {
                    if let Some(list) = list {
                        let _ = self.lists.apply(&list);
                        if let Some(previews) = self.lists.documents.get_mut(&PREVIEWS) {
                            previews.take_changes();
                        }
                    }
                    self.placing.arrived(&keys, directions);
                }
                FromEngine::Copied { seq, text, roots, entities, from } => self.copied(seq, text, roots, entities, from),
                FromEngine::PortsOf { uids, ports } => self.ports_arrived(&uids, ports),
                FromEngine::Details { uid, components } => self.details_arrived(uid, components),
                FromEngine::Fields { uid, answer } => self.fields_arrived(uid, answer),
                FromEngine::Containers { uid, containers } => self.containers_arrived(uid, containers),
                FromEngine::Backdrop { document, backdrop } => {
                    if self.document != Some(document) {
                        continue;
                    }
                    match backdrop {
                        Ok(backdrop) => self.map_backdrop_arrived(backdrop),
                        Err(error) => self.set_status(format!("Couldn't read the backdrop: {}", error.lines().next().unwrap_or(""))),
                    }
                }
                FromEngine::BackdropNames(names) => self.backdrop_names_arrived(names),
                FromEngine::BackdropPainted { name, thumb, whole } => self.backdrop_painted(name, thumb, whole),
                FromEngine::Air { document, asked, answer } => self.air_arrived(document, asked, answer),
                FromEngine::AirFixPreview { document, previews } => self.air_fix_previewed(document, previews),
                FromEngine::Failed(error) => {
                    self.engine_gone = true;
                    if let Some(loading) = self.loading.as_mut() {
                        loading.fail(&error, Instant::now());
                    }
                    self.set_status(format!("The engine failed: {error}"));
                }
            }
        }
        if self.needs_fit && view.0 > 0 && view.1 > 0 {
            self.needs_fit = false;
            self.fit(view);
        }

        // The one selected entity's component data and containers, asked for once.
        let one = match self.selection[..] {
            [uid] => Some(uid),
            _ => None,
        };
        if one != self.inspecting_uid() {
            self.details = None;
            self.inspect(one);
        }
        changed
    }

    // ---- the view ----

    /// Shows the main body of the document in a view of the given size. A zoom still owed is dropped.
    pub fn fit(&mut self, view: (u32, u32)) {
        self.stop_zoom();
        let bounds = self.document.and_then(|d| self.lists.documents.get(&d)).and_then(|d| d.bounds());
        let Some((left, bottom, right, top)) = bounds else {
            // Nothing drawn yet (a new map): a comfortable size around the origin.
            self.camera = Camera { x: 0.0, y: 0.0, zoom: 2.0 };
            return;
        };
        let zoom = (view.0 as f32 / ((right - left) * 32.0)).min(view.1 as f32 / ((top - bottom) * 32.0)) * 0.95;
        self.camera = Camera { x: (left + right) / 2.0, y: (bottom + top) / 2.0, zoom: zoom.clamp(0.02, 4.0) };
    }

    /// The world position under a point of the map area, both in device pixels from its top left.
    pub fn world_at(&self, point: (f32, f32), view: (u32, u32)) -> (f32, f32) {
        self.camera.world_at(point, view)
    }

    /// Moves the view by a drag of this many device pixels.
    pub fn pan(&mut self, delta: (f32, f32)) {
        let scale = 32.0 * self.camera.zoom;
        self.camera.x -= delta.0 / scale;
        self.camera.y += delta.1 / scale;
    }

    /// Zooms by wheel steps, keeping the point under the cursor where it is.
    pub fn zoom(&mut self, steps: f32, cursor: (f32, f32), view: (u32, u32)) {
        let before = self.world_at(cursor, view);
        self.camera.zoom = (self.camera.zoom * 1.15f32.powf(steps)).clamp(0.02, 16.0);
        let after = self.world_at(cursor, view);
        self.camera.x += before.0 - after.0;
        self.camera.y += before.1 - after.1;
    }

    // ---- documents ----

    /// Where the open dialog starts: the folder given for the fork, or where the fork keeps its maps.
    pub fn maps_folder(&self) -> PathBuf {
        self.maps_folder.clone().unwrap_or_else(|| Path::new(&self.fork_repo).join("Resources").join("Maps"))
    }

    /// The folder the open dialog starts in, as the settings remember it for the fork.
    pub fn set_maps_folder(&mut self, folder: PathBuf) {
        self.maps_folder = Some(folder);
    }

    /// Opens a map file in place of the open document.
    pub fn open(&mut self, path: &str) {
        self.set_status(format!("Opening {path}"));
        self.cut_waits_no_more();
        self.load(Some(path));
        self.send(ToEngine::Open { path: Some(path.to_owned()) });
    }

    /// Starts an empty map in place of the open document.
    pub fn new_map(&mut self) {
        self.set_status("Starting an empty map");
        self.cut_waits_no_more();
        self.load(None);
        self.send(ToEngine::Open { path: None });
    }

    /// A new load for the loading screen, of a map or an empty one. One asked for while the
    /// engine still starts keeps the start's stages. An engine that stopped answers nothing,
    /// and its failure stays on the screen.
    fn load(&mut self, map: Option<&str>) {
        if self.engine_gone {
            return;
        }
        let now = Instant::now();
        self.loading = Some(match self.loading.as_ref() {
            Some(loading) if loading.current().is_some() => loading.reopen(map),
            _ => Loading::open(map, now),
        });
    }

    /// A stage of the load finished.
    fn reached(&mut self, step: Step) {
        if let Some(loading) = self.loading.as_mut() {
            loading.finish(step, Instant::now());
        }
    }

    /// The interface drew the open map with its sprites on the GPU: the last stage of a load.
    pub fn map_drawn(&mut self) {
        if self.loading.as_ref().is_some_and(|loading| loading.current_step() == Some(Step::Textures)) {
            self.reached(Step::Textures);
        }
    }

    /// Lets go of a load that has finished or failed, once its screen is gone.
    pub fn loading_done(&mut self) {
        self.loading = None;
    }

    /// Saves to the file the document came from. False if it has none yet (use `save_as`).
    /// A cut whose copy has not been answered would delete after a map now asked for is open,
    /// in that map. It is a copy alone from here.
    fn cut_waits_no_more(&mut self) {
        if let Some(copying) = self.copying.as_mut() {
            copying.cut_no_more();
        }
    }

    pub fn save(&mut self) -> bool {
        let Some(path) = self.doc.as_ref().and_then(|doc| doc.path.clone()) else { return false };
        self.save_as(&path.to_string_lossy());
        true
    }

    pub fn save_as(&mut self, path: &str) {
        if self.doc.is_some() {
            self.set_status(format!("Saving {path}"));
            self.send(ToEngine::Save { path: path.to_owned() });
        }
    }

    /// Undo and redo drop the area tool's region, which may not hold what it selected any more.
    pub fn undo(&mut self) {
        if self.doc.is_some() {
            self.clear_region();
            let seq = self.seq();
            self.last_edit = seq;
            self.send(ToEngine::Undo { seq, asked: Instant::now() });
        }
    }

    pub fn redo(&mut self) {
        if self.doc.is_some() {
            self.clear_region();
            let seq = self.seq();
            self.last_edit = seq;
            self.send(ToEngine::Redo { seq, asked: Instant::now() });
        }
    }

    fn seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }

    fn stroke(&mut self) -> u64 {
        self.next_stroke += 1;
        self.next_stroke
    }

    fn edit(&mut self, ops: Vec<Value>, label: &'static str, stroke: Option<u64>, coalesce: bool) -> u64 {
        let seq = self.seq();
        if !ops.is_empty() {
            self.last_edit = seq;
            self.send(ToEngine::Edit { ops, label, stroke, coalesce, seq, asked: Instant::now() });
        }
        seq
    }

    /// Sends the one layer switch that is the engine's, if it changed. The others take effect
    /// in the renderer ([`Layers::hidden_mask`]).
    pub fn sync_layers(&mut self) {
        if self.sent_t_ray != Some(self.layers.t_ray) && self.engine_ready_ms.is_some() {
            self.sent_t_ray = Some(self.layers.t_ray);
            self.send(ToEngine::SubFloor { show: self.layers.t_ray, asked: Instant::now() });
        }
    }
}

#[cfg(test)]
impl Session {
    /// A session with nothing behind it: what it asks of the engine can be read from the
    /// receiver, and nothing answers. It has the scene's mirror and FloorSteel chosen, but no
    /// draw list, so nothing on the map can be clicked until a test gives it one. It is never
    /// [`Self::idle`].
    fn offline(scene: &str) -> (Session, Receiver<ToEngine>) {
        let (to_engine, asked) = mpsc::channel();
        let (_, from_engine) = mpsc::channel();
        let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
        let scene: SceneJson = serde_json::from_str(scene).expect("scene");
        session.grid = (scene.grids[0].uid, scene.grids[0].x, scene.grids[0].y);
        session.doc = Some(Doc::from_scene(scene, None));
        session.palette = Palette::Tile("FloorSteel".into());
        (session, asked)
    }
}

#[cfg(test)]
#[path = "../tests/session.rs"]
mod tests;
