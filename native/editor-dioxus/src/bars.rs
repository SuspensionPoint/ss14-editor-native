//! The menu bar, the tool bar down the left side, and the status bar, using the web editor's
//! markup and class names (its MenuBar.tsx, Toolbar.tsx and StatusBar.tsx).

use dioxus_native::prelude::*;
use editor_core::session::{InHand, Tool, ToolEntry};

use crate::dialogs::{self, Discarding};
use crate::{bump, watch, with, Editor};

/// One row of a menu: its label, its key, whether it can be chosen, a tick for a switch, and
/// what it does. None is a separator.
#[derive(Clone, PartialEq)]
#[allow(unpredictable_function_pointer_comparisons)]
struct Item {
    label: String,
    shortcut: &'static str,
    enabled: bool,
    checked: Option<bool>,
    action: fn(&mut Editor),
}

fn item(label: impl Into<String>, shortcut: &'static str, enabled: bool, action: fn(&mut Editor)) -> Option<Item> {
    Some(Item { label: label.into(), shortcut, enabled, checked: None, action })
}

fn switch(label: &str, on: bool, action: fn(&mut Editor)) -> Option<Item> {
    Some(Item { label: label.to_owned(), shortcut: "", enabled: true, checked: Some(on), action })
}

/// The menus, with what is known now.
fn menus(editor: &Editor) -> Vec<(&'static str, Vec<Option<Item>>)> {
    let session = &editor.session;
    let history = session.doc.as_ref().map(|doc| doc.history.clone()).unwrap_or_default();
    let undo = format!("Undo {}", history.undo_label.as_deref().unwrap_or("")).trim_end().to_owned();
    let redo = format!("Redo {}", history.redo_label.as_deref().unwrap_or("")).trim_end().to_owned();
    // Each acts on the area tool's region when that tool holds one, and else on the selected
    // entities, and is named for it.
    let target = session.edit_target();
    let (copyable, actionable, open) = (session.can_copy(), session.can_delete(), session.doc.is_some());
    let file = vec![
        item("New map", "Ctrl+N", true, |editor| dialogs::discard_for(editor, Discarding::New)),
        item("Open map...", "Ctrl+O", true, dialogs::show_open_dialog),
        None,
        item("Save", "Ctrl+S", true, dialogs::save),
        item("Save as...", "Ctrl+Shift+S", true, dialogs::show_save_dialog),
        None,
        item("Quit", "", true, |editor| dialogs::discard_for(editor, Discarding::Quit)),
    ];
    let edit = vec![
        item(undo, "Ctrl+Z", history.can_undo, |editor| editor.session.undo()),
        item(redo, "Ctrl+Y", history.can_redo, |editor| editor.session.redo()),
        None,
        item(format!("Copy {target}"), "Ctrl+C", copyable, |editor| editor.session.copy()),
        item(format!("Cut {target}"), "Ctrl+X", copyable, |editor| editor.session.cut()),
        item("Paste", "Ctrl+V", session.clipboard().is_some(), |editor| editor.session.begin_paste()),
        None,
        item(format!("Turn {target}"), "R", actionable, |editor| editor.session.rotate(true)),
        item(format!("Delete {target}"), "Del", actionable, |editor| editor.session.delete_key()),
        None,
        item("Find entity...", "Ctrl+F", open, |editor| editor.find.show(&editor.session)),
    ];
    let view = vec![
        item("Fit map to window", "F", true, |editor| {
            let view = editor.view;
            editor.session.fit(view);
        }),
        item("Actual size", "", true, |editor| editor.session.camera.zoom = 1.0),
        None,
        switch("Show entities", session.layers.entities, |editor| editor.session.layers.entities ^= true),
        switch("Grid lines", session.show_grid, |editor| editor.session.show_grid ^= true),
        switch("Space backdrop", session.show_backdrop, |editor| editor.session.show_backdrop ^= true),
        None,
        switch("Performance", editor.show_performance, |editor| editor.show_performance ^= true),
    ];
    let map = vec![
        item("Validate map...", "", open, |editor| {
            editor.validation = Some(editor.session.validate());
            editor.validated = editor.session.validation_key();
        }),
        None,
        item("Backdrop...", "", session.backdrop_unavailable().is_none(), |editor| {
            editor.session.open_backdrops();
        }),
        item("Fix grid atmos...", "", session.fix_air_unavailable().is_none(), |editor| {
            editor.session.open_fix_air();
        }),
        None,
        item("Switch fork...", "", true, |editor| dialogs::discard_for(editor, Discarding::SwitchFork)),
    ];
    let help = vec![item("Controls", "?", true, |editor| editor.show_controls = true)];
    vec![("File", file), ("Edit", edit), ("View", view), ("Map", map), ("Help", help)]
}

#[component]
pub(crate) fn MenuBar() -> Element {
    let _ = watch();
    let (menus, open, dirty, backdrop, unavailable) = with(|editor| {
        // Which of the game's backdrops this map has, under the View menu's switch.
        let backdrop = match (&editor.session.backdrop, editor.session.doc.is_some()) {
            (Some(backdrop), _) if backdrop.layers == 0 => "This map has no backdrop".to_owned(),
            (Some(backdrop), _) => format!("Backdrop: {}", backdrop.name),
            (None, true) => "Backdrop: loading...".to_owned(),
            (None, false) => String::new(),
        };
        // Why the Map menu's items cannot be chosen, under them.
        let reasons = [editor.session.backdrop_unavailable().map(|why| format!("Backdrop: {why}")), editor.session.fix_air_unavailable().map(|why| format!("Fix grid atmos: {why}"))];
        let unavailable: Vec<String> = reasons.into_iter().flatten().collect();
        (menus(editor), editor.menu, editor.dirty(), backdrop, unavailable)
    });
    let (fork, notice) = with(|editor| (editor.fork_label.clone(), editor.notice.clone()));

    rsx! {
        div { class: "flex items-center h-9 bg-surface border-b border-subtle px-2 gap-1",
            span { class: "text-[13px] font-bold text-accent mr-3", "SS14 Map Editor" }
            for (name, items) in menus {
                div { class: "relative", key: "{name}",
                    onmousedown: move |event| event.stop_propagation(),
                    button {
                        class: if open == Some(name) { "px-3 py-1 text-xs text-primary cursor-pointer hover:bg-hover rounded-sm border-none h-full bg-active" } else { "px-3 py-1 text-xs text-primary cursor-pointer hover:bg-hover rounded-sm border-none h-full bg-transparent" },
                        onclick: move |_| {
                            with(|editor| editor.menu = if editor.menu == Some(name) { None } else { Some(name) });
                            bump();
                        },
                        // Once a menu is open, the mouse over another opens that one.
                        onmouseenter: move |_| {
                            let moved = with(|editor| {
                                let switch = editor.menu.is_some() && editor.menu != Some(name);
                                if switch {
                                    editor.menu = Some(name);
                                }
                                switch
                            });
                            if moved {
                                bump();
                            }
                        },
                        "{name}"
                    }
                    if open == Some(name) {
                        div { class: "absolute top-full left-0 min-w-[160px] bg-elevated border border-subtle rounded-sm shadow-lg z-50 py-1",
                            style: "min-width: 220px;",
                            for (index, entry) in items.into_iter().enumerate() {
                                match entry {
                                    None => rsx! { div { key: "{index}", class: "h-px bg-subtle mx-2 my-1" } },
                                    Some(entry) => rsx! { MenuRow { key: "{index}", entry } },
                                }
                            }
                            if name == "View" && !backdrop.is_empty() {
                                div { class: "px-3 py-1 text-[10px] text-muted", "{backdrop}" }
                            }
                            if name == "Map" {
                                for (index, why) in unavailable.iter().enumerate() {
                                    div { key: "{index}", class: "px-3 py-1 text-[10px] text-muted", style: "max-width: 220px;", "{why}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "flex-1" }
            // Unsaved changes, as the web editor shows them at the right of its bar.
            if dirty {
                span { class: "text-warning text-[10px]", "Unsaved changes" }
            }
            // The fork the editor runs on, and its branch.
            span { class: "text-muted text-[10px] ml-3", "{fork}" }
        }
        // A warning the start left, such as an engine side with no code fingerprint.
        if let Some(notice) = notice {
            div { class: "flex items-center bg-surface border-b border-subtle px-3 py-1 gap-2 text-[11px] text-warning",
                onmousedown: move |event| event.stop_propagation(),
                span { class: "flex-1", "{notice}" }
                button { class: "bg-transparent border-none text-muted hover:text-primary cursor-pointer text-sm leading-none px-1",
                    onclick: move |_| { with(|editor| editor.notice = None); bump(); },
                    "\u{d7}"
                }
            }
        }
    }
}

#[component]
fn MenuRow(entry: Item) -> Element {
    let Item { label, shortcut, enabled, checked, action } = entry;
    rsx! {
        button {
            disabled: !enabled,
            class: if enabled { "flex w-full px-3 py-1.5 text-xs text-left items-center gap-2 border-none bg-transparent text-primary cursor-pointer hover:bg-hover" } else { "flex w-full px-3 py-1.5 text-xs text-left items-center gap-2 border-none bg-transparent text-muted cursor-default hover:bg-transparent" },
            onclick: move |_| {
                if enabled {
                    with(|editor| {
                        editor.menu = None;
                        action(editor);
                    });
                    bump();
                }
            },
            span { class: "w-[18px] flex items-center justify-center", if checked == Some(true) { dialogs::Tick {} } }
            span { class: "flex-1", "{label}" }
            if !shortcut.is_empty() {
                span { class: "text-muted ml-4 text-[10px]", "{shortcut}" }
            }
        }
    }
}

#[component]
pub(crate) fn ToolBar() -> Element {
    let _ = watch();
    let active = with(|editor| editor.session.tool());
    // The web editor's three groups, the last of the tools that lay cables and pipes.
    let group = |tool: Tool| if tool.is_infrastructure_tool() { 2 } else { usize::from(tool.is_entity_tool()) };
    rsx! {
        div { class: "flex flex-col w-[76px] bg-panel border-r border-subtle py-1 overflow-y-auto",
            for (index, section) in ["Tiles", "Entities", "Infra"].into_iter().enumerate() {
                if index > 0 {
                    div { class: "h-px bg-subtle mx-2 my-1" }
                }
                div { class: "text-[9px] text-muted uppercase text-center tracking-wider py-0.5", "{section}" }
                for entry in Tool::ALL.into_iter().filter(|entry| group(entry.tool) == index) {
                    ToolButton { key: "{entry.name}", tool: entry.tool, active: active == entry.tool }
                }
            }
            div { class: "text-[8px] text-muted text-center mt-2 leading-tight opacity-60", "Space+Drag" br {} "to pan" }
        }
    }
}

#[component]
fn ToolButton(tool: Tool, active: bool) -> Element {
    let Some(entry) = Tool::ALL.into_iter().find(|entry| entry.tool == tool) else { return rsx! {} };
    let ToolEntry { name, key, .. } = entry;
    // What it does, from the list of controls, as the button's hover text.
    let title = match entry.control() {
        Some(does) => format!("{does} ({key})"),
        None => format!("{name} ({key})"),
    };
    rsx! {
        button {
            class: if active { "flex flex-col items-center justify-center w-full py-2 text-[10px] cursor-pointer border-none gap-0.5 bg-active text-accent" } else { "flex flex-col items-center justify-center w-full py-2 text-[10px] cursor-pointer border-none gap-0.5 bg-transparent text-muted hover:bg-hover hover:text-primary" },
            title: "{title}",
            onclick: move |_| {
                with(|editor| editor.session.set_tool(tool));
                bump();
            },
            span { class: "leading-tight", "{name}" }
            span { class: "text-[8px] text-muted opacity-60", "{key}" }
        }
    }
}

#[component]
pub(crate) fn StatusBar() -> Element {
    let _ = (watch(), crate::watch_map());
    let (held, status, zoom, hover, document, selected) = with(|editor| {
        let held = editor.session.in_hand().map(|held| {
            let picture = in_hand_picture(editor, &held);
            let hover = match &held {
                InHand::Entity { id, .. } => format!("{}: {id}", held.kind()),
                _ => held.kind().to_owned(),
            };
            let starred = editor.session.held_favourite().is_some_and(|favourite| editor.session.is_favourite(&favourite));
            (held.name().to_owned(), hover, picture, starred)
        });
        let session = &editor.session;
        // The document: how many entities, and the size of the grid being edited.
        let document = session.doc.is_some().then(|| {
            let entities = session.performance().entities;
            let grid = session.grid_size().map_or(String::new(), |(width, height)| format!("grid {width} x {height}   "));
            format!("{grid}{entities} entities")
        });
        let selected = (!session.selection.is_empty()).then(|| format!("{} selected", session.selection.len()));
        (held, session.status_line().to_owned(), format!("{:.0}%", session.camera.zoom * 100.0), session.hover_text(), document, selected)
    });
    rsx! {
        div { class: "flex items-center h-6 bg-surface border-t border-subtle px-3 text-[10px] text-muted gap-4",
            // What is in hand, with its picture. The web editor has no such place.
            if let Some((name, hover, picture, starred)) = held {
                span { class: "flex items-center gap-1", title: "{hover}",
                    span { class: "in-hand",
                        if let Some(picture) = picture {
                            img { src: "{picture}", class: "pixelated", style: "width: 16px; height: 16px;" }
                        }
                    }
                    span { class: "text-accent font-bold", "{name}" }
                    // Adds what is in hand to the favourites, or takes it off.
                    span { class: "flex items-center gap-1 cursor-pointer hover:text-primary",
                        onclick: move |_| {
                            with(|editor| {
                                if let Some(favourite) = editor.session.held_favourite() {
                                    editor.session.toggle_favourite(favourite);
                                }
                            });
                            bump();
                        },
                        crate::dialogs::Star { filled: starred, size: 12 }
                        if starred { "Remove from favourites" } else { "Add to favourites" }
                    }
                }
            }
            span { "{status}" }
            span { class: "ml-auto" }
            if let Some(selected) = selected {
                span { "{selected}" }
            }
            if let Some(document) = document {
                span { "{document}" }
            }
            if let Some(hover) = hover {
                span { "{hover}" }
            }
            span { "{zoom}" }
        }
    }
}

/// The picture of what is in hand, small.
fn in_hand_picture(editor: &mut Editor, held: &InHand) -> Option<String> {
    let Editor { session, textures, thumbs, .. } = editor;
    match held {
        InHand::Tile(id) => {
            let sprite = session.tiles.iter().find(|tile| &tile.id == id).and_then(|tile| tile.sprite.clone());
            thumbs.tile(textures, id, sprite.as_deref(), 32)
        }
        InHand::Entity { index, .. } => {
            session.want_previews(&[*index]);
            thumbs.entity(session, textures, *index, 32)
        }
        InHand::Decal(id) => {
            let index = session.decal_prototypes.iter().position(|decal| &decal.id == id)?;
            thumbs.decal(session, textures, index)
        }
    }
}
