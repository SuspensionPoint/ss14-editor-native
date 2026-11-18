//! The Prefabs and Grids tabs and their dialogs: naming a prefab for Save as Prefab, deleting a
//! prefab, renaming a grid and removing one. The session reads and writes the files and the grids.
//! This file only draws, using the web editor's PrefabPanel.tsx markup where there is one.

use std::collections::HashSet;

use dioxus_native::prelude::*;
use editor_core::prefab::Preview;
use editor_core::session::MAP_GROUND;

use crate::dialogs::{ask_file, Asking, Chevron, Confirm, Modal, TextField};
use crate::{bump, watch, with};

/// The Prefabs tab's own state: the search, the groups folded shut, and the dialogs.
#[derive(Default)]
pub(crate) struct PrefabsPanel {
    search: String,
    folded: HashSet<String>,
    /// The name being typed for Save as Prefab.
    naming: Option<String>,
    /// The prefab whose delete waits for the user to agree.
    deleting: Option<String>,
}

impl PrefabsPanel {
    /// Whether one of its dialogs is open.
    pub(crate) fn dialog_open(&self) -> bool {
        self.naming.is_some() || self.deleting.is_some()
    }

    /// Closes its dialogs, as Escape does.
    pub(crate) fn close_dialogs(&mut self) {
        self.naming = None;
        self.deleting = None;
    }
}

/// The Grids tab's own state: the name typed for a new grid, and the dialogs.
#[derive(Default)]
pub(crate) struct GridsPanel {
    new_name: String,
    /// The grid being renamed, and the name typed.
    renaming: Option<(i32, String)>,
    /// The grid whose removal waits for the user to agree, and the question asked.
    removing: Option<(i32, String)>,
}

impl GridsPanel {
    /// Whether one of its dialogs is open.
    pub(crate) fn dialog_open(&self) -> bool {
        self.renaming.is_some() || self.removing.is_some()
    }

    /// Closes its dialogs, as Escape does.
    pub(crate) fn close_dialogs(&mut self) {
        self.renaming = None;
        self.removing = None;
    }
}

/// One prefab row as the list shows it.
#[derive(Clone, PartialEq)]
struct PrefabRow {
    name: String,
    title: String,
    group: String,
    detail: String,
    readable: bool,
    preview: Option<Preview>,
    picked: bool,
}

/// A prefab's preview: its cells' shape in a square, as small divs.
#[component]
fn PrefabPreview(preview: Preview) -> Element {
    let side = 32.0 / preview.width.max(preview.height).max(1) as f64;
    let (left, top) = ((32.0 - side * preview.width as f64) / 2.0, (32.0 - side * preview.height as f64) / 2.0);
    let cells: Vec<(f64, f64)> = preview
        .filled
        .iter()
        .enumerate()
        .filter(|(_, filled)| **filled)
        .map(|(index, _)| (left + (index % preview.width) as f64 * side, top + (index / preview.width) as f64 * side))
        .collect();
    rsx! {
        span { class: "bg-surface rounded-sm", style: "position: relative; display: inline-block; width: 32px; height: 32px; flex-shrink: 0;",
            for (x, y) in cells {
                span { style: "position: absolute; left: {x}px; top: {y}px; width: {side}px; height: {side}px; background: #4488ff;" }
            }
        }
    }
}

#[component]
pub(crate) fn PrefabPalette() -> Element {
    let _ = watch();
    let (folder, search, rows, folded, total) = with(|editor| {
        let search = editor.prefabs.search.clone();
        let session = &mut editor.session;
        let chosen = session.is_pasting().then(|| session.chosen_prefab().map(str::to_owned)).flatten();
        let found = session.prefabs_matching(&search);
        let prefabs = session.prefabs();
        let total = prefabs.len();
        let rows: Vec<PrefabRow> = found
            .iter()
            .map(|&index| {
                let prefab = &prefabs[index];
                let (detail, readable, preview) = match &prefab.shape {
                    Ok(shape) => {
                        let entities = if shape.entities == 1 { "1 entity".to_owned() } else { format!("{} entities", shape.entities) };
                        (format!("{}x{}, {} tiles, {entities}", shape.size.0, shape.size.1, shape.tiles), true, Some(shape.preview.clone()))
                    }
                    Err(why) => (format!("Can't read this file: {why}"), false, None),
                };
                PrefabRow { name: prefab.name.clone(), title: prefab.title.clone(), group: prefab.group.clone(), detail, readable, preview, picked: chosen.as_deref() == Some(prefab.name.as_str()) }
            })
            .collect();
        (editor.session.prefabs_folder().display().to_string(), search, rows, editor.prefabs.folded.clone(), total)
    });
    let searching = !search.trim().is_empty();
    let shown = rows.len();
    // The rows with a header before each group's first, and a search showing folded groups too.
    let mut lines: Vec<(Option<(String, usize, bool)>, Option<PrefabRow>)> = Vec::new();
    let mut group: Option<String> = None;
    for row in &rows {
        if group.as_deref() != Some(row.group.as_str()) {
            group = Some(row.group.clone());
            if !row.group.is_empty() {
                let count = rows.iter().filter(|other| other.group == row.group).count();
                lines.push((Some((row.group.clone(), count, folded.contains(&row.group))), None));
            }
        }
        if searching || !folded.contains(&row.group) {
            lines.push((None, Some(row.clone())));
        }
    }

    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            div { class: "px-2 pt-2 pb-1 text-[10px] text-muted", "In {folder}" }
            div { class: "flex gap-1 p-2 border-b border-subtle",
                TextField { value: search, placeholder: "Search prefabs...", oninput: move |text: String| { with(|editor| editor.prefabs.search = text); bump(); } }
                button { class: "px-2 py-1 bg-elevated border border-subtle rounded-sm text-primary text-xs cursor-pointer hover:bg-hover",
                    title: "Reload the folder",
                    onclick: move |_| { with(|editor| editor.session.refresh_prefabs()); bump(); },
                    "Refresh"
                }
                button { class: "px-2 py-1 bg-elevated border border-subtle rounded-sm text-primary text-xs cursor-pointer hover:bg-hover",
                    title: "Copy a .ss14prefab file into the folder",
                    onclick: move |_| { with(|editor| ask_file(editor, Asking::ImportPrefab)); bump(); },
                    "Import..."
                }
            }
            div { class: "flex-1 overflow-y-auto",
                if rows.is_empty() {
                    div { class: "text-muted text-xs p-3 italic text-center leading-relaxed",
                        if searching { "No matching prefabs." } else { "No prefabs yet. Save a region with Area Select's right-click menu, or import a file." }
                    }
                }
                for (index, line) in lines.into_iter().enumerate() {
                    match line {
                        (Some((group, count, folded)), _) => rsx! {
                            div { key: "g{index}", class: "flex items-center gap-1 px-2 py-1 cursor-pointer bg-surface border-b border-subtle text-[11px] text-muted select-none",
                                onclick: move |_| {
                                    with(|editor| {
                                        if !editor.prefabs.folded.remove(&group) {
                                            editor.prefabs.folded.insert(group.clone());
                                        }
                                    });
                                    bump();
                                },
                                span { class: "flex items-center", Chevron { open: !folded } }
                                span { "{group}" }
                                span { class: "text-muted ml-auto", "{count}" }
                            }
                        },
                        (None, Some(row)) => rsx! { PrefabLine { key: "p{row.name}", row } },
                        _ => rsx! {},
                    }
                }
            }
            div { class: "px-2 py-1 text-[10px] text-muted border-t border-subtle", "{shown} of {total} prefabs" }
        }
    }
}

#[component]
fn PrefabLine(row: PrefabRow) -> Element {
    let PrefabRow { name, title, group, detail, readable, preview, picked } = row;
    let (pick, delete) = (name.clone(), name.clone());
    let back = if picked { "bg-active border border-accent" } else { "hover:bg-hover" };
    rsx! {
        div { class: "flex items-center gap-2 px-3 py-1 text-xs cursor-pointer border-b border-subtle {back}",
            style: if group.is_empty() { "" } else { "padding-left: 14px;" },
            title: "{detail}",
            onclick: move |_| { with(|editor| editor.session.choose_prefab(&pick)); bump(); },
            if let Some(preview) = preview {
                PrefabPreview { preview }
            } else {
                span { class: "bg-surface rounded-sm", style: "display: inline-block; width: 32px; height: 32px; flex-shrink: 0;" }
            }
            div { class: "flex-1 overflow-hidden",
                div { class: if picked { "text-accent text-xs truncate" } else { "text-primary text-xs truncate" }, "{title}" }
                div { class: "text-[10px] truncate", style: if readable { "color: #888888;" } else { "color: #f87171;" }, "{detail}" }
            }
            button { class: "px-2 py-1 bg-elevated border border-subtle rounded-sm text-primary text-[10px] cursor-pointer hover:bg-hover",
                onclick: move |event| {
                    event.stop_propagation();
                    with(|editor| editor.prefabs.deleting = Some(delete.clone()));
                    bump();
                },
                "Delete"
            }
        }
    }
}

/// The Grids tab: the open map's grids with name and uid, a click centring the view on one,
/// and adding a grid in the middle of the view.
#[component]
pub(crate) fn GridPalette() -> Element {
    let _ = watch();
    let (refused, name, rows) = with(|editor| (editor.session.grid_edits_refused().map(str::to_owned), editor.grids.new_name.clone(), editor.session.grid_rows()));
    let add = move || {
        with(|editor| {
            if editor.session.grid_edits_refused().is_none() {
                let name = std::mem::take(&mut editor.grids.new_name);
                editor.session.add_grid(&name);
            }
        });
        bump();
    };
    let enabled = refused.is_none();
    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            div { class: "flex gap-1 p-2 border-b border-subtle",
                TextField { value: name, placeholder: "New grid name", disabled: !enabled,
                    oninput: move |text: String| { with(|editor| editor.grids.new_name = text); bump(); },
                    onenter: move |_| add(),
                }
                button { class: "px-2 py-1 bg-elevated border border-subtle rounded-sm text-primary text-xs cursor-pointer hover:bg-hover whitespace-nowrap",
                    disabled: !enabled,
                    title: "Add an empty grid in the middle of the view",
                    onclick: move |_| add(),
                    "Add grid"
                }
            }
            if let Some(why) = refused.clone() {
                div { class: "px-2 py-1 text-[10px] text-muted", "{why}" }
            }
            div { class: "flex-1 overflow-y-auto",
                if rows.is_empty() {
                    div { class: "text-muted text-xs p-3 italic text-center", "The map has no grids." }
                }
                for row in rows {
                    {
                        let (uid, label, tiles, active, ground) = (row.uid, format!("{} ({})", row.name, row.uid), if row.tiles == 1 { "1 tile".to_owned() } else { format!("{} tiles", row.tiles) }, row.active, row.ground);
                        let name = row.name.clone();
                        rsx! {
                            div { key: "{uid}", class: if active { "flex items-center gap-2 px-2 py-1 text-xs border-b border-subtle bg-active" } else { "flex items-center gap-2 px-2 py-1 text-xs border-b border-subtle hover:bg-hover" },
                                button { class: if active { "flex-1 text-left bg-transparent border-none cursor-pointer text-accent text-xs truncate" } else { "flex-1 text-left bg-transparent border-none cursor-pointer text-primary text-xs truncate" },
                                    title: "{tiles}. Click to centre the view on it and paint on it.",
                                    onclick: move |_| { with(|editor| editor.session.centre_on_grid(uid)); bump(); },
                                    "{label}"
                                }
                                span { class: "text-[10px] text-muted whitespace-nowrap", "{tiles}" }
                                button { class: "px-2 py-0.5 bg-elevated border border-subtle rounded-sm text-primary text-[10px] cursor-pointer hover:bg-hover",
                                    onclick: move |_| { with(|editor| editor.grids.renaming = Some((uid, name.clone()))); bump(); },
                                    "Rename"
                                }
                                button { class: "px-2 py-0.5 bg-elevated border border-subtle rounded-sm text-primary text-[10px] cursor-pointer hover:bg-hover",
                                    disabled: !enabled || ground,
                                    title: if ground { MAP_GROUND } else { "" },
                                    onclick: move |_| {
                                        with(|editor| editor.grids.removing = editor.session.grid_removal_question(uid).map(|question| (uid, question)));
                                        bump();
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The dialogs of both tabs.
#[component]
pub(crate) fn PrefabDialogs() -> Element {
    let _ = watch();
    let (naming, deleting, renaming, removing, folder) = with(|editor| {
        if editor.session.wants_prefab_name() {
            editor.prefabs.naming = Some(String::new());
        }
        (editor.prefabs.naming.clone(), editor.prefabs.deleting.clone(), editor.grids.renaming.clone(), editor.grids.removing.clone(), editor.session.prefabs_folder().display().to_string())
    });
    let save = move || {
        with(|editor| {
            if let Some(name) = editor.prefabs.naming.clone().filter(|name| !name.trim().is_empty()) {
                editor.prefabs.naming = None;
                editor.session.save_region_as_prefab(&name);
            }
        });
        bump();
    };
    let rename = move || {
        with(|editor| {
            if let Some((uid, name)) = editor.grids.renaming.take() {
                editor.session.rename_grid(uid, &name);
            }
        });
        bump();
    };
    let button = "bg-active border border-subtle rounded text-primary text-[13px] px-6 py-2 cursor-pointer hover:bg-hover";
    rsx! {
        if let Some(name) = naming {
            Modal { title: "Save as prefab", width: 420, onclose: move |_| { with(|editor| editor.prefabs.naming = None); bump(); },
                div { class: "text-[11px] text-muted mb-2", "Use / for subfolders. Saved in {folder}" }
                TextField { value: name, placeholder: "rooms/kitchen", focus: true,
                    oninput: move |text: String| { with(|editor| editor.prefabs.naming = Some(text)); bump(); },
                    onenter: move |_| save(),
                }
                div { class: "flex gap-2 mt-4 justify-center",
                    button { class: button, onclick: move |_| save(), "Save" }
                    button { class: button, onclick: move |_| { with(|editor| editor.prefabs.naming = None); bump(); }, "Cancel" }
                }
            }
        }
        if let Some(name) = deleting {
            Confirm {
                title: "Delete prefab",
                message: format!("Delete the prefab {name}? This removes its file from the folder."),
                confirm: "Delete",
                danger: true,
                onconfirm: move |_| {
                    with(|editor| {
                        editor.prefabs.deleting = None;
                        editor.session.delete_prefab(&name);
                    });
                    bump();
                },
                oncancel: move |_| { with(|editor| editor.prefabs.deleting = None); bump(); },
            }
        }
        if let Some((uid, name)) = renaming {
            Modal { title: "Rename grid", width: 320, onclose: move |_| { with(|editor| editor.grids.renaming = None); bump(); },
                TextField { value: name, focus: true,
                    oninput: move |text: String| { with(|editor| editor.grids.renaming = Some((uid, text))); bump(); },
                    onenter: move |_| rename(),
                }
                div { class: "flex gap-2 mt-4 justify-center",
                    button { class: button, onclick: move |_| rename(), "Rename" }
                    button { class: button, onclick: move |_| { with(|editor| editor.grids.renaming = None); bump(); }, "Cancel" }
                }
            }
        }
        if let Some((uid, question)) = removing {
            Confirm {
                title: "Remove grid",
                message: question,
                confirm: "Remove",
                danger: true,
                onconfirm: move |_| {
                    with(|editor| {
                        editor.grids.removing = None;
                        editor.session.remove_grid(uid);
                    });
                    bump();
                },
                oncancel: move |_| { with(|editor| editor.grids.removing = None); bump(); },
            }
        }
    }
}
