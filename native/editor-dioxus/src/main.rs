//! SS14 Map Editor's interface, made with Dioxus Native.
//!
//!   ss14-editor <engine folder>            the start screen chooses the fork (start.rs)
//!   ss14-editor <engine folder> <fork repo> <kernel pack .zip> <textures file> [map .yml]
//!   ss14-editor --stamp-engine | --build-engine | --bake ...   see editor-core's shell.rs
//!
//! Everything that decides anything about the map lives in editor-core's session. This crate is
//! only the interface: Dioxus components with the web editor's class names, styled by the web
//! editor's own built stylesheet and drawn by Blitz, with no browser. The map is a custom widget.
//! The same renderer draws it into a texture that the page shows.
//!
//! This file holds the editor's state, start-up, the keyboard and the window layout. The panels
//! and dialogs have their own files.

mod air_fix;
mod backdrops;
mod bars;
mod canvas;
mod components;
mod dialogs;
mod find;
mod icons;
mod links;
mod loading;
mod options;
mod palettes;
mod placing;
mod prefabs;
mod run;
mod selection;
mod start;
mod start_view;
mod theme;
mod thumbs;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use dioxus_native::prelude::*;
use dioxus_native::winit::event::{ElementState, WindowEvent};
use dioxus_native::winit::keyboard::{Key as WinitKey, KeyCode, NamedKey as WinitNamed, PhysicalKey};
use editor_core::session::{arrows_reach_map, escape_to, Arrow, EscapeTo, Session};
use editor_core::toolbox::{Target, TOOL_KEYS};
use editor_core::shell::{Launch, ShellArgs};
use editor_core::textures::BakedTextures;
use editor_core::validate::Finding;
use futures_util::StreamExt;

use dialogs::{Discarding, FileAsk, Folders};
use palettes::PaletteTab;

const TITLE: &str = "SS14 Map Editor";

/// The web editor's built stylesheet (Tailwind's output, copied from its `dist`), and what this
/// interface adds for what the web editor styled inline or does not have.
const WEB_CSS: &str = include_str!("web.css");
const LOCAL_CSS: &str = include_str!("local.css");

/// The editor's state: the session and what the interface keeps beside it. It lives on the
/// interface's thread, and components, event handlers and the map widget reach it through
/// [`with`]. None of them reaches it while another has it.
pub(crate) struct Editor {
    pub(crate) session: Session,
    pub(crate) textures: Arc<BakedTextures>,
    pub(crate) thumbs: thumbs::Thumbs,
    pub(crate) tab: PaletteTab,
    /// The target the palette tabs last followed (`options::follow_target`).
    pub(crate) seen_target: Option<Target>,
    pub(crate) tile_search: String,
    pub(crate) entity_search: String,
    pub(crate) decal_search: String,
    /// The categories of the entity palette's tree that are open, by their paths.
    pub(crate) open_categories: HashSet<String>,
    /// The decal groups that are open, by their tags.
    pub(crate) open_decal_groups: HashSet<String>,
    /// The tiles in the palette's order, and how many tiles there were when it was worked out.
    pub(crate) tile_order: (usize, Vec<usize>),
    /// The prototypes the entity search shows, and what they were worked out for.
    pub(crate) entity_matches: Vec<usize>,
    pub(crate) entity_matches_for: Option<(String, usize, bool)>,
    pub(crate) entity_count: usize,
    /// Set when a choice made away from the palettes asks the palette to scroll to it.
    pub(crate) scroll_to_chosen: bool,
    /// The Prefabs and Grids tabs and their dialogs, and the entity panel's typing.
    pub(crate) prefabs: prefabs::PrefabsPanel,
    pub(crate) grids: prefabs::GridsPanel,
    pub(crate) entity_panel: components::EntityPanel,
    /// The colour the decal panel's Recolor All gives, as typed.
    pub(crate) recolor: String,
    /// The menu of the menu bar that is open.
    pub(crate) menu: Option<&'static str>,
    pub(crate) file_ask: Option<FileAsk>,
    pub(crate) folders: Folders,
    pub(crate) confirm: Option<Discarding>,
    pub(crate) find: find::FindDialog,
    pub(crate) validation: Option<Vec<Finding>>,
    /// What the findings were found for: the document's revision and the air's, so that they
    /// are found again when either changes while the list is open, as after its Fix.
    pub(crate) validated: (i32, u64),
    pub(crate) show_controls: bool,
    pub(crate) show_performance: bool,
    /// Set once the window may close: the user agreed to lose unsaved changes, or there were none.
    pub(crate) quitting: bool,
    /// The mouse button a press on the map began with, and where the pointer last was on the
    /// map, in CSS pixels from its top left corner.
    pub(crate) pressing: Option<editor_core::session::Button>,
    pub(crate) last_pointer: Option<(f32, f32)>,
    /// Where the map is in the window, in CSS pixels, learned from the mouse events on it.
    pub(crate) origin: (f32, f32),
    /// The map area in device pixels and the device pixels to a CSS pixel, as last painted.
    pub(crate) view: (u32, u32),
    pub(crate) scale: f32,
    /// Whether a text field has the keyboard. Keys are then the field's.
    pub(crate) typing: bool,
    /// Whether Space is down on the map. The modifiers come from the window.
    pub(crate) space: bool,
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) title: String,
    /// The pointer's look last given to the window.
    pub(crate) cursor: Option<editor_core::session::PointerLook>,
    /// The engine side the editor came with, which a switch of fork starts the editor again
    /// with. The session may run on one built for the fork's code.
    pub(crate) engine_folder: PathBuf,
    /// The fork's name and branch, at the right of the menu bar.
    pub(crate) fork_label: String,
    /// A warning the start left, shown as a line under the menu bar until it is closed.
    pub(crate) notice: Option<String>,
    /// The map file last noted in the settings as the fork's last and recent map.
    pub(crate) recorded_map: Option<PathBuf>,
}

impl Editor {
    /// The editor around a session that has started, with its textures, or empty ones that
    /// the session hands over once made (`Session::take_textures`).
    pub(crate) fn new(mut session: Session, textures: Arc<BakedTextures>, engine_folder: PathBuf, fork_label: String, notice: Option<String>) -> Editor {
        first_tool(&mut session);
        Editor {
            session,
            textures,
            thumbs: thumbs::Thumbs::default(),
            tab: palettes::first_tab(),
            seen_target: None,
            tile_search: String::new(),
            entity_search: String::new(),
            decal_search: String::new(),
            open_categories: HashSet::new(),
            open_decal_groups: HashSet::new(),
            tile_order: (0, Vec::new()),
            entity_matches: Vec::new(),
            entity_matches_for: None,
            entity_count: 0,
            scroll_to_chosen: false,
            prefabs: prefabs::PrefabsPanel::default(),
            grids: prefabs::GridsPanel::default(),
            entity_panel: components::EntityPanel::default(),
            recolor: "#FFFFFFFF".to_owned(),
            menu: None,
            file_ask: None,
            folders: Folders::default(),
            confirm: None,
            find: find::FindDialog::default(),
            validation: None,
            validated: (0, 0),
            show_controls: false,
            show_performance: false,
            quitting: false,
            pressing: None,
            last_pointer: None,
            origin: (0.0, 0.0),
            view: (0, 0),
            scale: 1.0,
            typing: false,
            space: false,
            shift: false,
            ctrl: false,
            title: String::new(),
            cursor: None,
            engine_folder,
            fork_label,
            notice,
            recorded_map: None,
        }
    }

    pub(crate) fn dirty(&self) -> bool {
        self.session.doc.as_ref().is_some_and(|doc| doc.is_dirty())
    }

    /// Whether a dialog is open. The keys are then the dialog's.
    pub(crate) fn dialog_open(&self) -> bool {
        self.file_ask.is_some()
            || self.confirm.is_some()
            || self.show_controls
            || self.find.open
            || self.validation.is_some()
            || self.prefabs.dialog_open()
            || self.grids.dialog_open()
            || self.session.backdrops_open()
            || self.session.fix_air_open()
    }

    fn close_dialogs(&mut self) {
        self.session.cancel_backdrops();
        self.session.cancel_fix_air();
        self.confirm = None;
        self.show_controls = false;
        self.find.open = false;
        self.validation = None;
        self.prefabs.close_dialogs();
        self.grids.close_dialogs();
    }

    /// Tells the session which modifiers and keys are held.
    fn hand_on_held(&mut self) {
        let elsewhere = self.typing || self.dialog_open();
        self.session.dialog_open = self.dialog_open();
        self.session.held = editor_core::session::Held { shift: self.shift, ctrl: self.ctrl, space: self.space && !elsewhere };
    }
}

/// A developer's switch for looking at a tool's options without a click: `SS14_EDITOR_TOOL`
/// holds tool keys, which are pressed in turn as the editor opens. `S` opens on Select with
/// Tiles, `P` on Paint placing entities.
fn first_tool(session: &mut Session) {
    for key in std::env::var("SS14_EDITOR_TOOL").unwrap_or_default().chars().filter(|key| TOOL_KEYS.contains(&key.to_ascii_uppercase())) {
        session.key_down(key, false);
        session.key_up(key);
    }
}

thread_local! {
    static EDITOR: RefCell<Option<Editor>> = const { RefCell::new(None) };
    /// The signal every component reads, and that anything which changes the editor bumps.
    static TICK: Cell<Option<Signal<u64>>> = const { Cell::new(None) };
    /// The signal the map and the status bar read beside it, which the mouse moving over the
    /// map bumps, so that a move does not build the side panel again.
    static MAP_TICK: Cell<Option<Signal<u64>>> = const { Cell::new(None) };
}

/// The window, once it is open, for asking it to draw again and setting its title.
static WINDOW: OnceLock<Arc<dyn dioxus_native::winit::window::Window>> = OnceLock::new();
/// What wakes the interface from other threads: the engine, the file dialogs and the timer.
static WAKE: OnceLock<futures_channel::mpsc::UnboundedSender<()>> = OnceLock::new();

/// Runs `f` with the editor. Not to be called from inside another call of it, nor while the
/// start screen shows, when there is no editor yet.
pub(crate) fn with<R>(f: impl FnOnce(&mut Editor) -> R) -> R {
    EDITOR.with(|editor| f(editor.borrow_mut().as_mut().expect("no editor")))
}

/// Runs `f` with the editor if there is one: None while the start screen shows.
pub(crate) fn try_with<R>(f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    EDITOR.with(|editor| editor.borrow_mut().as_mut().map(f))
}

/// Whether the editor is open, and not the start screen.
pub(crate) fn editor_open() -> bool {
    EDITOR.with(|editor| editor.borrow().is_some())
}

/// Puts the editor in place of the start screen.
pub(crate) fn set_editor(editor: Editor) {
    EDITOR.with(|cell| *cell.borrow_mut() = Some(editor));
}

/// Makes the component that calls it draw again whenever the editor changes.
pub(crate) fn watch() -> u64 {
    // A component drawing means the document will change (`run::settle`).
    drawn();
    TICK.with(|tick| tick.get().map_or(0, |tick| *tick.read()))
}

/// Notes that a component which does not follow the editor drew (`run::settle`).
pub(crate) fn drawn() {
    run::STALE.with(|stale| stale.set(true));
    // Blitz sets a text field's new value without asking for a frame (`set_attribute` in its
    // mutator returns before marking anything), so a value that came from the engine stayed
    // unpainted until something else drew. A component drawing asks for one more frame.
    redraw();
}

/// Makes the component that calls it draw again whenever the mouse moves on the map.
pub(crate) fn watch_map() -> u64 {
    drawn();
    MAP_TICK.with(|tick| tick.get().map_or(0, |tick| *tick.read()))
}

/// Says that only what the map and the status bar show changed, as when the mouse moves.
pub(crate) fn bump_map() {
    if let Some(mut tick) = MAP_TICK.with(Cell::get) {
        *tick.write() += 1;
    }
    redraw();
}

/// Says the editor changed: every component draws again, and the window with them.
pub(crate) fn bump() {
    canvas::forget_hover_label();
    if let Some(mut tick) = TICK.with(Cell::get) {
        *tick.write() += 1;
    }
    redraw();
}

/// Asks the window to draw again, as when only the map changed.
pub(crate) fn redraw() {
    if let Some(window) = WINDOW.get() {
        window.request_redraw();
    }
}

/// Wakes the interface from any thread: it reads the editor again on its own thread.
pub(crate) fn wake() {
    if let Some(wake) = WAKE.get() {
        let _ = wake.unbounded_send(());
    }
}

fn main() {
    let launch = ShellArgs::parse_launch();
    let (send, woken) = futures_channel::mpsc::unbounded::<()>();
    let _ = WAKE.set(send);
    match launch {
        // The start scripts' way with a map: the fork, its pack and its textures given.
        Launch::Fork(mut args) => match args.choose_engine() {
            Ok(notice) => start::open_given(&args, notice),
            Err(refused) => start::show(args.home_engine.clone(), Some(refused)),
        },
        Launch::StartScreen { engine_folder } => start::show(engine_folder, None),
    }
    WOKEN.with(|cell| *cell.borrow_mut() = Some(woken));
    run::launch(app);
    // The engine's threads would keep the process alive; there is nothing left to save.
    std::process::exit(0);
}

thread_local! {
    static WOKEN: RefCell<Option<futures_channel::mpsc::UnboundedReceiver<()>>> = const { RefCell::new(None) };
}

/// The keyboard, from the window's own events: Blitz hands keys only to the element with the
/// keyboard, and with none to the root, where no component listens.
fn key_event(event: &WindowEvent) {
    // The start screen takes no keys of its own.
    if !editor_open() {
        return;
    }
    match event {
        WindowEvent::ModifiersChanged(modifiers) => {
            let state = modifiers.state();
            with(|editor| {
                editor.shift = state.shift_key();
                editor.ctrl = state.control_key();
                editor.hand_on_held();
            });
            bump();
        }
        WindowEvent::Focused(false) => {
            // Keys that come up while another window has the keyboard are not seen.
            with(|editor| {
                editor.space = false;
                editor.session.key_lost('R');
                editor.hand_on_held();
            });
            bump();
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let pressed = event.state == ElementState::Pressed;
            let letter = match event.physical_key {
                PhysicalKey::Code(code) => letter_of(code),
                _ => None,
            };
            let changed = with(|editor| {
                let (ctrl, shift) = (editor.ctrl, editor.shift);
                if pressed && event.logical_key == WinitKey::Named(WinitNamed::Escape) {
                    match escape_to(editor.menu.is_some(), editor.dialog_open(), editor.typing) {
                        EscapeTo::Menu => editor.menu = None,
                        EscapeTo::Dialog => editor.close_dialogs(),
                        // The field being typed into has it, and the selection stays.
                        EscapeTo::Field => return false,
                        EscapeTo::Map => {
                            // A failed open goes back to the map that was open.
                            if !loading::dismiss(editor) {
                                editor.session.escape();
                            }
                        }
                    }
                    return true;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) {
                    let was = editor.space;
                    editor.space = pressed && !editor.typing && !editor.dialog_open();
                    editor.hand_on_held();
                    if was != editor.space {
                        return true;
                    }
                }
                // No key below reaches the map, the tools or the file from behind a dialog, or
                // while a field is typed into. R that was held on the map is lost to it.
                if editor.typing || editor.dialog_open() {
                    editor.session.key_lost('R');
                    return false;
                }
                if pressed && ctrl {
                    match letter {
                        Some('Z') if shift => editor.session.redo(),
                        Some('Y') => editor.session.redo(),
                        Some('Z') => editor.session.undo(),
                        Some('S') if shift => dialogs::show_save_dialog(editor),
                        Some('S') => dialogs::save(editor),
                        Some('O') => dialogs::show_open_dialog(editor),
                        Some('N') => dialogs::discard_for(editor, Discarding::New),
                        Some('F') => editor.find.show(&editor.session),
                        Some('C') => editor.session.copy(),
                        Some('X') => editor.session.cut(),
                        Some('V') => editor.session.begin_paste(),
                        _ => return false,
                    }
                    return true;
                }
                // Enter closes a polygon of Select being clicked out.
                if pressed && event.logical_key == WinitKey::Named(WinitNamed::Enter) && editor.session.enter_key() {
                    return true;
                }
                if pressed && matches!(event.logical_key, WinitKey::Named(WinitNamed::Delete | WinitNamed::Backspace)) {
                    editor.session.delete_key();
                    return true;
                }
                // The arrow keys nudge the selection, at each press and each repeat of a held
                // key, when no menu has them. Shift is a fine step. They never pan.
                let arrow = match &event.logical_key {
                    WinitKey::Named(WinitNamed::ArrowLeft) => Some(Arrow::Left),
                    WinitKey::Named(WinitNamed::ArrowRight) => Some(Arrow::Right),
                    WinitKey::Named(WinitNamed::ArrowUp) => Some(Arrow::Up),
                    WinitKey::Named(WinitNamed::ArrowDown) => Some(Arrow::Down),
                    _ => None,
                };
                if let Some(arrow) = arrow {
                    if pressed && !ctrl && arrows_reach_map(editor.menu.is_some(), editor.dialog_open(), editor.typing) {
                        editor.hand_on_held();
                        editor.session.nudge(arrow, shift);
                    }
                    return pressed;
                }
                if pressed && event.text.as_deref() == Some("?") {
                    editor.show_controls = true;
                    return true;
                }
                if pressed && letter == Some('F') && !shift && !ctrl {
                    let view = editor.view;
                    editor.session.fit(view);
                    return true;
                }
                // The number keys choose the first nine favourites, the row above the letters
                // and the number pad alike, once a press and not as the keyboard repeats it.
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(number) = number_of(code) {
                        return pressed && !event.repeat && !shift && !ctrl && editor.session.favourite_key(number);
                    }
                }
                // Tool keys: each handed on as it goes down and as it comes up, and not as the
                // keyboard repeats it.
                let Some(key) = letter.filter(|key| TOOL_KEYS.contains(key)) else { return false };
                if event.repeat {
                    return false;
                }
                if !pressed {
                    editor.session.key_up(key);
                } else if !ctrl {
                    editor.session.key_down(key, shift);
                }
                true
            });
            if changed {
                bump();
            }
        }
        _ => {}
    }
}

/// The letter of a key by where it is on the keyboard.
fn letter_of(code: KeyCode) -> Option<char> {
    let letters = [
        (KeyCode::KeyA, 'A'), (KeyCode::KeyB, 'B'), (KeyCode::KeyC, 'C'), (KeyCode::KeyD, 'D'), (KeyCode::KeyE, 'E'), (KeyCode::KeyF, 'F'),
        (KeyCode::KeyG, 'G'), (KeyCode::KeyH, 'H'), (KeyCode::KeyI, 'I'), (KeyCode::KeyJ, 'J'), (KeyCode::KeyK, 'K'), (KeyCode::KeyL, 'L'),
        (KeyCode::KeyM, 'M'), (KeyCode::KeyN, 'N'), (KeyCode::KeyO, 'O'), (KeyCode::KeyP, 'P'), (KeyCode::KeyQ, 'Q'), (KeyCode::KeyR, 'R'),
        (KeyCode::KeyS, 'S'), (KeyCode::KeyT, 'T'), (KeyCode::KeyU, 'U'), (KeyCode::KeyV, 'V'), (KeyCode::KeyW, 'W'), (KeyCode::KeyX, 'X'),
        (KeyCode::KeyY, 'Y'), (KeyCode::KeyZ, 'Z'),
    ];
    letters.iter().find(|(key, _)| *key == code).map(|(_, letter)| *letter)
}

/// The number of a key from 1 to 9, on the row above the letters or the number pad.
fn number_of(code: KeyCode) -> Option<usize> {
    let numbers = [
        (KeyCode::Digit1, KeyCode::Numpad1), (KeyCode::Digit2, KeyCode::Numpad2), (KeyCode::Digit3, KeyCode::Numpad3),
        (KeyCode::Digit4, KeyCode::Numpad4), (KeyCode::Digit5, KeyCode::Numpad5), (KeyCode::Digit6, KeyCode::Numpad6),
        (KeyCode::Digit7, KeyCode::Numpad7), (KeyCode::Digit8, KeyCode::Numpad8), (KeyCode::Digit9, KeyCode::Numpad9),
    ];
    numbers.iter().position(|&(row, pad)| code == row || code == pad).map(|at| at + 1)
}

fn app() -> Element {
    let tick = use_signal(|| 0u64);
    let map_tick = use_signal(|| 0u64);
    use_hook(|| {
        TICK.with(|cell| cell.set(Some(tick)));
        MAP_TICK.with(|cell| cell.set(Some(map_tick)));
    });
    let window = dioxus_native::use_window();
    use_hook(|| {
        let _ = WINDOW.set(window.clone());
    });

    // Whatever wakes the interface from another thread makes it read the editor again.
    use_hook(|| {
        let mut woken = WOKEN.with(|cell| cell.borrow_mut().take());
        spawn(async move {
            if let Some(woken) = woken.as_mut() {
                while woken.next().await.is_some() {
                    bump();
                }
            }
        });
        canvas::start_timer();
    });
    dioxus_native::use_window_event(|event, _| key_event(event));
    let _ = watch();

    // Each time it draws: the file a system dialog chose, the textures the session made, the
    // map to note in the settings, and the window's title. While the start screen shows, it
    // takes what its own threads found, and may open the editor.
    let open = editor_open() || start::poll();
    let title = match open {
        true => with(|editor| {
            dialogs::poll_file_ask(editor);
            if let Some(textures) = editor.session.take_textures() {
                editor.textures = textures;
                editor.thumbs = thumbs::Thumbs::default();
            }
            start::note_map(editor);
            editor.session.sync_layers();
            if editor.session.take_reveal() {
                palettes::reveal_choice(editor);
            }
            options::follow_target(editor);
            let title = match editor.session.doc.as_ref() {
                Some(doc) => format!("{}{} - {TITLE}", doc.source, if doc.is_dirty() { " *" } else { "" }),
                None => TITLE.to_owned(),
            };
            (title != editor.title).then(|| {
                editor.title = title.clone();
                title
            })
        }),
        false => None,
    };
    if let (Some(title), Some(window)) = (title, WINDOW.get()) {
        window.set_title(&title);
    }

    rsx! {
        style { {WEB_CSS} }
        style { {LOCAL_CSS} }
        // The web editor's page gives its root the window's height; here that is said directly.
        div { class: "flex flex-col w-full h-full", style: "height: 100vh;",
            onmounted: move |event| {
                if let Some(handle) = event.downcast::<dioxus_native::NodeHandle>() {
                    run::DOCUMENT.with(|document| *document.borrow_mut() = Some(handle.clone()));
                }
            },
            // A press off the menus closes them. The menus and the map stop their own presses.
            onmousedown: move |_| {
                let closed = try_with(|editor| {
                    let open = editor.menu.is_some() || editor.session.region_menu().is_some();
                    editor.menu = None;
                    editor.session.close_region_menu();
                    open
                });
                if closed == Some(true) {
                    bump();
                }
            },
            onmousemove: move |event| {
                if editor_open() {
                    canvas::drag_move(&event);
                }
            },
            onmouseup: move |event| {
                if editor_open() {
                    canvas::drag_up(&event);
                }
            },
            if open {
                bars::MenuBar {}
                options::OptionsBar {}
                div { class: "flex flex-1 overflow-hidden",
                    bars::ToolBar {}
                    div { class: "canvas-area flex-1 flex flex-col overflow-hidden",
                        div { class: "flex-1 relative overflow-hidden",
                            canvas::Canvas {}
                        }
                    }
                    palettes::SidePanel {}
                }
                bars::StatusBar {}
                // Over all of it while the engine starts or a map opens.
                loading::LoadingScreen {}
                dialogs::Dialogs {}
            } else {
                start::StartScreen {}
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/main.rs"]
mod tests;
