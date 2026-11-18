//! The dialogs and the actions that open them: the system's file dialogs through `rfd`, the
//! warning before unsaved changes are lost, the list of controls, and the window every dialog of
//! this interface sits in. Also the text field every panel types into.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dioxus_native::prelude::*;
use editor_core::files::{forward_slashes, save_as_start, with_yml, LastFolder};
use editor_core::session::CONTROLS;

use crate::{bump, try_with, wake, watch, with, Editor};

/// What an operating system's file dialog was opened for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Asking {
    Open,
    SaveAs,
    ImportPrefab,
}

/// An operating system's file dialog that is open, on a thread of its own so that the window
/// keeps drawing. The file chosen, or None for Cancel, comes back on `answer`.
pub(crate) struct FileAsk {
    asking: Asking,
    answer: Receiver<Option<PathBuf>>,
}

/// The folders the file dialogs last chose in, one for maps and one for prefabs.
#[derive(Default)]
pub(crate) struct Folders {
    maps: LastFolder,
    prefabs: LastFolder,
}

/// Something that throws the open document away, held back until the user agrees.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Discarding {
    New,
    Open(String),
    Quit,
    /// Back to the start screen, in a new process (`start::switch_fork`).
    SwitchFork,
}

/// Does something that replaces the open document, asking first if it has unsaved changes.
pub(crate) fn discard_for(editor: &mut Editor, action: Discarding) {
    if editor.dirty() {
        editor.confirm = Some(action);
    } else {
        discard(editor, action);
    }
}

fn discard(editor: &mut Editor, action: Discarding) {
    match action {
        Discarding::New => editor.session.new_map(),
        Discarding::Open(path) => editor.session.open(&path),
        Discarding::Quit => editor.quitting = true,
        Discarding::SwitchFork => crate::start::switch_fork(editor),
    }
}

/// Opens an operating system's file dialog on a thread of its own. Nothing else is asked
/// while one is open.
pub(crate) fn ask_file(editor: &mut Editor, asking: Asking) {
    if editor.file_ask.is_some() {
        return;
    }
    let (folder, name) = match asking {
        Asking::Open => (editor.folders.maps.start_in(&editor.session.maps_folder()), None),
        Asking::SaveAs => {
            let saved = editor.session.doc.as_ref().and_then(|doc| doc.path.clone());
            let (folder, name) = save_as_start(saved.as_deref(), &editor.folders.maps, &editor.session.maps_folder());
            (folder, Some(name))
        }
        Asking::ImportPrefab => (editor.folders.prefabs.start_in(&editor.session.prefabs_folder()), None),
    };
    let (send, answer) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("file dialog".into()).spawn(move || {
        let dialog = rfd::FileDialog::new().set_directory(&folder);
        let picked = match asking {
            Asking::Open => dialog.set_title("Open map").add_filter("Map", &["yml"]).pick_file(),
            Asking::SaveAs => dialog.set_title("Save map as").add_filter("Map", &["yml"]).set_file_name(name.unwrap_or_default()).save_file(),
            Asking::ImportPrefab => dialog.set_title("Import prefab").add_filter("Prefab", &["ss14prefab"]).add_filter("Any file", &["*"]).pick_file(),
        };
        let _ = send.send(picked);
        wake();
    });
    if spawned.is_ok() {
        editor.file_ask = Some(FileAsk { asking, answer });
    }
}

/// Takes the file an operating system's dialog chose, once it has closed.
pub(crate) fn poll_file_ask(editor: &mut Editor) {
    let Some(ask) = editor.file_ask.take() else { return };
    match ask.answer.try_recv() {
        Ok(Some(path)) => match ask.asking {
            Asking::Open => {
                editor.folders.maps.used(&path);
                discard_for(editor, Discarding::Open(forward_slashes(&path)));
            }
            Asking::SaveAs => {
                let path = with_yml(&path);
                editor.folders.maps.used(&path);
                editor.session.save_as(&forward_slashes(&path));
            }
            Asking::ImportPrefab => {
                editor.folders.prefabs.used(&path);
                editor.session.import_prefab(&path.to_string_lossy());
            }
        },
        Ok(None) | Err(TryRecvError::Disconnected) => {}
        Err(TryRecvError::Empty) => editor.file_ask = Some(ask),
    }
}

pub(crate) fn show_open_dialog(editor: &mut Editor) {
    ask_file(editor, Asking::Open);
}

pub(crate) fn save(editor: &mut Editor) {
    if !editor.session.save() {
        show_save_dialog(editor);
    }
}

pub(crate) fn show_save_dialog(editor: &mut Editor) {
    if editor.session.doc.is_some() {
        ask_file(editor, Asking::SaveAs);
    }
}

// Marks drawn with borders and fills. The font Blitz picks has few symbols (arrows, ticks and
// warning signs draw as empty boxes), so the interface draws no symbol characters but these.

/// A small triangle, pointing down when what it opens is open and right when it is shut.
#[component]
pub(crate) fn Chevron(open: bool) -> Element {
    rsx! { Triangle { towards: if open { "down" } else { "right" } } }
}

/// A small triangle in the colour of the text, pointing "up", "down", "left" or "right".
#[component]
pub(crate) fn Triangle(towards: String, #[props(default = 4)] size: u32) -> Element {
    let (across, along) = (size, size + size / 4 + 1);
    let style = match towards.as_str() {
        "down" => format!("border-left: {across}px solid transparent; border-right: {across}px solid transparent; border-top: {along}px solid currentColor;"),
        "up" => format!("border-left: {across}px solid transparent; border-right: {across}px solid transparent; border-bottom: {along}px solid currentColor;"),
        "left" => format!("border-top: {across}px solid transparent; border-bottom: {across}px solid transparent; border-right: {along}px solid currentColor;"),
        _ => format!("border-top: {across}px solid transparent; border-bottom: {across}px solid transparent; border-left: {along}px solid currentColor;"),
    };
    rsx! { span { class: "inline-block", style: "width: 0; height: 0; flex-shrink: 0; {style}" } }
}

/// A round mark in the colour of the text: filled, or a ring.
#[component]
pub(crate) fn Dot(filled: bool, #[props(default = 8)] size: u32) -> Element {
    let fill = if filled { "background: currentColor;" } else { "" };
    rsx! { span { class: "inline-block", style: "width: {size}px; height: {size}px; border-radius: 50%; border: 1.5px solid currentColor; flex-shrink: 0; box-sizing: border-box; {fill}" } }
}

/// A radio mark: a ring with a dot in it when chosen.
#[component]
pub(crate) fn Radio(chosen: bool) -> Element {
    rsx! {
        span { class: "inline-flex items-center justify-center", style: "width: 12px; height: 12px; border-radius: 50%; border: 1.5px solid currentColor; flex-shrink: 0; box-sizing: border-box;",
            if chosen {
                span { style: "width: 6px; height: 6px; border-radius: 50%; background: currentColor;" }
            }
        }
    }
}

/// A tick: a short and a long stroke at right angles, turned. Drawn as the two borders of a box.
#[component]
pub(crate) fn Tick(#[props(default = 10)] size: u32) -> Element {
    let (width, height) = (size * 2 / 5, size * 3 / 4);
    rsx! { span { class: "inline-block", style: "width: {width}px; height: {height}px; border-right: 2px solid currentColor; border-bottom: 2px solid currentColor; transform: rotate(45deg); margin: 0 3px 2px 3px; flex-shrink: 0;" } }
}

/// A favourite's star: filled in gold when it is one, an outline in grey when not. Drawn as a
/// picture at twice its size, for a sharp edge (`thumbs::star_uri`).
#[component]
pub(crate) fn Star(filled: bool, #[props(default = 14)] size: u32) -> Element {
    let picture = crate::thumbs::star_uri(filled, size * 2);
    rsx! { img { src: "{picture}", style: "width: {size}px; height: {size}px; flex-shrink: 0;" } }
}

/// A one-line text field. It tells the editor while it has the keyboard, so that keys typed
/// into it are not the map's, and hands on what is typed and Enter. Blitz draws no
/// placeholder, so the field's own is drawn over it while it is empty.
#[component]
pub(crate) fn TextField(
    value: String,
    #[props(default)] placeholder: String,
    #[props(default = "w-full px-2 py-1 bg-surface border border-subtle rounded-sm text-primary text-xs outline-none focus:border-accent".to_owned())] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] focus: bool,
    oninput: EventHandler<String>,
    #[props(default)] onenter: Option<EventHandler<()>>,
    #[props(default)] onleave: Option<EventHandler<()>>,
    #[props(default)] onkey: Option<EventHandler<Key>>,
) -> Element {
    let field = rsx! {
        input {
            r#type: "text",
            class: "{class}",
            placeholder: "{placeholder}",
            disabled,
            value: "{value}",
            onmounted: move |event| {
                if focus {
                    spawn(async move {
                        let _ = event.set_focus(true).await;
                    });
                }
            },
            // The start screen has fields too, with no editor yet.
            onfocus: move |_| {
                try_with(|editor| editor.typing = true);
            },
            onblur: move |_| {
                try_with(|editor| editor.typing = false);
                if let Some(onleave) = onleave {
                    onleave.call(());
                }
            },
            oninput: move |event| oninput.call(event.value()),
            onkeydown: move |event| {
                if let Some(onkey) = onkey {
                    onkey.call(event.key());
                }
                if event.key() == Key::Named(NamedKey::Enter) {
                    if let Some(onenter) = onenter {
                        onenter.call(());
                    }
                }
            },
        }
    };
    if placeholder.is_empty() {
        return field;
    }
    rsx! {
        div { class: "field",
            {field}
            if value.is_empty() {
                span { class: "placeholder", "{placeholder}" }
            }
        }
    }
}

/// The window a dialog sits in, over the rest with the page darkened, as the web editor's
/// list of controls is. A click on the dark closes it, as does its close button.
#[component]
pub(crate) fn Modal(title: String, #[props(default = 560)] width: u32, onclose: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div { class: "fixed inset-0 flex items-center justify-center", style: "background-color: rgba(0,0,0,0.7); z-index: 9998;",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| onclose.call(()),
            div { class: "bg-elevated border border-subtle rounded-lg p-6 w-full overflow-y-auto text-primary text-[13px]",
                style: "max-width: {width}px; max-height: 80vh;",
                onclick: move |event| event.stop_propagation(),
                div { class: "flex items-center justify-between mb-4",
                    h2 { class: "text-base font-semibold text-accent m-0", "{title}" }
                    button { class: "bg-transparent border-none text-muted hover:text-primary cursor-pointer text-lg leading-none px-1",
                        onclick: move |_| onclose.call(()),
                        "\u{d7}"
                    }
                }
                {children}
            }
        }
    }
}

/// A question with two answers, in the web editor's ConfirmModal look.
#[component]
pub(crate) fn Confirm(title: String, message: String, confirm: String, #[props(default)] danger: bool, onconfirm: EventHandler<()>, oncancel: EventHandler<()>) -> Element {
    let (back, edge) = if danger { ("#6b2020", "#8b3030") } else { ("#0f3460", "#2a2a4a") };
    rsx! {
        div { class: "fixed inset-0 flex items-center justify-center", style: "background-color: rgba(0,0,0,0.7); z-index: 9999;",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| oncancel.call(()),
            div { style: "background-color: #1a1a2e; border: 1px solid #2a2a4a; border-radius: 8px; padding: 32px 40px; max-width: 480px; color: #ccc; font-size: 14px; line-height: 1.7; text-align: center;",
                onclick: move |event| event.stop_propagation(),
                h2 { style: "color: #fff; margin: 0 0 16px; font-size: 20px;", "{title}" }
                p { style: "margin: 0 0 24px;", "{message}" }
                div { style: "display: flex; gap: 12px; justify-content: center;",
                    button { style: "background-color: #2a2a4a; border: 1px solid #3a3a5a; border-radius: 4px; color: #ccc; font-size: 14px; padding: 10px 24px; cursor: pointer;",
                        onclick: move |_| oncancel.call(()),
                        "Cancel"
                    }
                    button { style: "background-color: {back}; border: 1px solid {edge}; border-radius: 4px; color: #fff; font-size: 14px; padding: 10px 24px; cursor: pointer;",
                        onclick: move |_| onconfirm.call(()),
                        "{confirm}"
                    }
                }
            }
        }
    }
}

/// Every dialog that is open.
#[component]
pub(crate) fn Dialogs() -> Element {
    let _ = watch();
    let (waiting, confirm, controls) = with(|editor| (editor.file_ask.is_some(), editor.confirm.clone(), editor.show_controls));
    rsx! {
        if waiting {
            // The window behind stays drawn and takes nothing till the dialog closes.
            div { class: "fixed inset-0 flex items-center justify-center", style: "background-color: rgba(0,0,0,0.5); z-index: 9999;",
                onmousedown: move |event| event.stop_propagation(),
                div { class: "bg-elevated border border-subtle rounded-lg p-6 text-primary text-[13px]", "Waiting for the file dialog..." }
            }
        }
        if controls {
            Modal { title: "Controls", onclose: move |_| { with(|editor| editor.show_controls = false); bump(); },
                div { class: "grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5",
                    for (keys, does) in CONTROLS.iter() {
                        kbd { class: "text-accent font-mono text-[12px] text-right whitespace-nowrap", "{keys}" }
                        span { class: "text-primary", "{does}" }
                    }
                }
                div { class: "text-center mt-4",
                    button { class: "bg-active border border-subtle rounded text-primary text-[13px] px-6 py-2 cursor-pointer hover:bg-hover",
                        onclick: move |_| { with(|editor| editor.show_controls = false); bump(); },
                        "Close"
                    }
                }
            }
        }
        crate::find::FindDialogs {}
        crate::prefabs::PrefabDialogs {}
        crate::links::LinkDialog {}
        crate::backdrops::BackdropDialog {}
        crate::air_fix::FixAirDialog {}
        if let Some(action) = confirm {
            Confirm {
                title: "Unsaved changes",
                message: "This map has unsaved changes. Discard them?",
                confirm: "Discard changes",
                danger: true,
                onconfirm: move |_| {
                    with(|editor| {
                        editor.confirm = None;
                        discard(editor, action.clone());
                    });
                    bump();
                },
                oncancel: move |_| { with(|editor| editor.confirm = None); bump(); },
            }
        }
    }
}
