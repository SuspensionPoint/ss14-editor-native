//! The part of the selection panel for a single entity: its name, its containers and what each
//! holds, and its components. Each component is a form of its data fields, with the full text
//! behind a switch. The session builds, sends and checks every change. This file only draws and
//! passes on what is typed and clicked, in the markup of the web editor's EntityInfoPanel.tsx and
//! ContainerContentsEditor.tsx.

use std::collections::{HashMap, HashSet};

use dioxus_native::prelude::*;
use editor_core::fields::{flag_set, FieldKind};
use editor_core::session::{decal_colour_bytes, ComponentEntry, ComponentRow, FieldRow, PIPE_COLOURS};
use serde_json::{json, Value};

use crate::dialogs::{Chevron, TextField};
use crate::palettes::ColourField;
use crate::{bump, watch, with};

/// The colour of the engine's refusal.
const REFUSED: &str = "#e07070";

/// How many matches a search under the panel offers.
const OFFERED: usize = 8;

/// An enum of at most this many names is a row of its choices. One of more is a list that opens below.
const SHORT_LIST: usize = 4;

/// The small buttons of the panel.
const SMALL: &str = "px-1.5 py-0.5 rounded-sm border border-subtle text-primary text-[10px] cursor-pointer bg-elevated hover:bg-hover";

/// A small button that is the choice made, or a switch that is on.
const CHOSEN: &str = "px-1.5 py-0.5 rounded-sm border border-accent text-accent text-[10px] cursor-pointer bg-active";

/// What the panel keeps while one entity is edited. Selecting another starts it afresh.
#[derive(Default)]
pub(crate) struct EntityPanel {
    uid: Option<i32>,
    /// The name being typed, until the field lets go of the keyboard.
    name: Option<String>,
    /// What is being typed into the forms, by component and field.
    fields: HashMap<(String, String), String>,
    /// The search for a component to add.
    component_search: String,
    /// The container a thing is being added to, and the search for its prototype.
    adding_to: Option<String>,
    contain_search: String,
    /// The components that are open.
    open: HashSet<String>,
    /// The components whose whole text is shown beside their form.
    json: HashSet<String>,
    /// The enum field whose long list is open, by component and field.
    listing: Option<(String, String)>,
}

/// The name, the containers and the components of the one selected entity.
#[component]
pub(crate) fn EntityEditor(uid: i32) -> Element {
    let _ = watch();
    let error = with(|editor| {
        if editor.entity_panel.uid != Some(uid) {
            editor.entity_panel = EntityPanel { uid: Some(uid), ..Default::default() };
        }
        editor.session.entity_error().map(str::to_owned)
    });
    rsx! {
        if let Some(error) = error {
            div { class: "text-[10px] mt-1", style: "color: {REFUSED};", "{error}" }
        }
        NameField { uid }
        Containers {}
        Components {}
    }
}

/// The entity's own name, sent when the field lets go of the keyboard or with Enter.
#[component]
fn NameField(uid: i32) -> Element {
    let _ = watch();
    let Some((own, hint, typed)) = with(|editor| {
        let entity = editor.session.doc.as_ref()?.entities.get(&uid)?;
        let hint = editor.session.prototype(&entity.prototype).map(|prototype| prototype.name.clone()).unwrap_or_default();
        Some((entity.name.clone().unwrap_or_default(), hint, editor.entity_panel.name.clone()))
    }) else {
        return rsx! {};
    };
    let send = move || {
        with(|editor| {
            if let Some(typed) = editor.entity_panel.name.take() {
                let own = editor.session.doc.as_ref().and_then(|doc| doc.entities.get(&uid)).and_then(|entity| entity.name.clone()).unwrap_or_default();
                if typed.trim() != own {
                    editor.session.rename_entity(&typed);
                }
            }
        });
        bump();
    };
    rsx! {
        div { class: "flex items-center gap-2 mt-2",
            span { class: "text-muted", "Name" }
            TextField { value: typed.unwrap_or(own), placeholder: hint,
                oninput: move |text: String| { with(|editor| editor.entity_panel.name = Some(text)); bump(); },
                onenter: move |_| send(),
                onleave: move |_| send(),
            }
        }
    }
}

/// The entity's containers, what each holds with a button to remove it, and a search to add a
/// new thing to one that can take more.
#[component]
fn Containers() -> Element {
    let _ = watch();
    let found = with(|editor| {
        let containers = match editor.session.containers() {
            None => return None,
            Some(Err(why)) => return Some(Err(why.lines().next().unwrap_or("").to_owned())),
            Some(Ok(containers)) if containers.is_empty() => return None,
            Some(Ok(containers)) => containers.clone(),
        };
        let doc = editor.session.doc.as_ref();
        let rows: Vec<(String, String, usize, bool, Vec<(i32, String)>)> = containers
            .iter()
            .map(|container| {
                let held = container.entities.iter().map(|&held| (held, doc.and_then(|doc| doc.entities.get(&held)).map_or_else(|| format!("entity {held}"), |entity| entity.prototype.clone()))).collect();
                let kind = if container.kind == "ContainerSlot" { "slot" } else { "container" };
                (container.id.clone(), kind.to_owned(), container.entities.len(), container.can_insert, held)
            })
            .collect();
        let adding = editor.entity_panel.adding_to.clone();
        let search = editor.entity_panel.contain_search.clone();
        let words: Vec<&str> = search.split_whitespace().collect();
        let offers: Vec<(String, String)> = if adding.is_some() && !words.is_empty() {
            editor.session.placeable_matching(&words).into_iter().take(OFFERED).map(|index| {
                let prototype = &editor.session.prototypes()[index];
                (prototype.id.clone(), prototype.name.clone())
            }).collect()
        } else {
            Vec::new()
        };
        Some(Ok((rows, adding, search, offers)))
    });
    let Some(found) = found else { return rsx! {} };
    let (rows, adding, search, offers) = match found {
        Err(why) => return rsx! { div { class: "text-muted text-[10px] mt-2", "Couldn't read the containers: {why}" } },
        Ok(found) => found,
    };
    rsx! {
        div { class: "mt-2",
            div { class: "text-muted text-[10px] mb-1", "Containers" }
            for (id, kind, count, can_insert, held) in rows {
                {
                    let open = adding.as_deref() == Some(id.as_str());
                    let toggle = id.clone();
                    rsx! {
                        div { key: "{id}", class: "mb-1",
                            div { class: "flex items-center gap-1.5",
                                span { class: "text-primary text-[11px]", "{id}" }
                                span { class: "text-muted text-[10px] flex-1", "{kind}, {count} held" }
                                button { class: SMALL, disabled: !can_insert, title: if can_insert { "" } else { "The slot is full" },
                                    onclick: move |_| {
                                        with(|editor| {
                                            editor.entity_panel.adding_to = if open { None } else { Some(toggle.clone()) };
                                            editor.entity_panel.contain_search.clear();
                                        });
                                        bump();
                                    },
                                    if open { "Close" } else { "Add..." }
                                }
                            }
                            for (held, prototype) in held {
                                div { key: "{held}", class: "flex items-center gap-1.5 px-1 py-0.5 rounded-sm hover:bg-hover", style: "margin-left: 12px;",
                                    span { class: "text-primary text-[11px] truncate flex-1", "{prototype}  ({held})" }
                                    button { class: SMALL, title: "Delete this item and everything inside it",
                                        onclick: move |_| { with(|editor| editor.session.uncontain(held)); bump(); },
                                        "Remove"
                                    }
                                }
                            }
                            if open {
                                div { class: "relative mt-1",
                                    TextField { value: search.clone(), placeholder: "Search to add an item...", focus: true,
                                        oninput: move |text: String| { with(|editor| editor.entity_panel.contain_search = text); bump(); },
                                    }
                                    for (prototype, name) in offers.clone() {
                                        {
                                            let container = id.clone();
                                            rsx! {
                                                button { key: "{prototype}", class: "flex items-center gap-1.5 w-full px-2 py-1 text-left border-none bg-transparent text-primary text-[10px] cursor-pointer hover:bg-hover",
                                                    onclick: move |_| {
                                                        with(|editor| {
                                                            editor.session.contain(&container, &prototype);
                                                            editor.entity_panel.adding_to = None;
                                                            editor.entity_panel.contain_search.clear();
                                                        });
                                                        bump();
                                                    },
                                                    span { class: "truncate", "{name}" }
                                                    span { class: "text-muted ml-auto text-[9px]", "{prototype}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Each component as a form of its fields, with its whole text behind a switch, Apply, Revert
/// and Remove, and adding a component by name.
#[component]
fn Components() -> Element {
    let _ = watch();
    let (entries, busy, open, json, search, offers, error) = with(|editor| {
        let entries = editor.session.component_entries();
        let search = editor.entity_panel.component_search.trim().to_owned();
        let offers: Vec<String> = if search.is_empty() { Vec::new() } else { editor.session.component_names_matching(&search, OFFERED).into_iter().filter(|name| **name != *search).map(|name| name.to_string()).collect() };
        let error = editor.session.component_fields_error().map(|why| why.lines().next().unwrap_or("").to_owned());
        (entries, editor.session.entity_busy(), editor.entity_panel.open.clone(), editor.entity_panel.json.clone(), editor.entity_panel.component_search.clone(), offers, error)
    });
    if entries.is_empty() {
        return rsx! { div { class: "text-muted text-[10px] mt-2", "Reading components..." } };
    }
    let own = entries.iter().filter(|entry| entry.own).count();
    let count = if entries.len() == 1 { "1 component".to_owned() } else { format!("{} components", entries.len()) };
    let add = move || {
        with(|editor| {
            if !editor.entity_panel.component_search.trim().is_empty() {
                let typed = std::mem::take(&mut editor.entity_panel.component_search);
                editor.session.add_component(&typed);
            }
        });
        bump();
    };
    rsx! {
        div { class: "mt-2",
            if let Some(why) = error {
                div { class: "text-muted text-[10px]", "Forms unavailable: {why}" }
            }
            div { class: "text-muted text-[10px]", "{count}, {own} with data of their own" }
            for entry in entries {
                {
                    let name = entry.row.name.clone();
                    let (shown, whole) = (open.contains(&name), json.contains(&name));
                    rsx! { ComponentBox { key: "{name}", entry, busy, shown, json: whole } }
                }
            }
        }
        // Adding a component, from the names the prototypes have.
        div { class: "mt-1 flex gap-0.5 items-center",
            TextField { value: search, placeholder: "Add a component...",
                class: "bg-elevated border border-subtle rounded-sm text-primary text-[10px] px-1 py-0.5 flex-1 w-full outline-none focus:border-accent",
                oninput: move |text: String| { with(|editor| editor.entity_panel.component_search = text); bump(); },
                onenter: move |_| add(),
            }
            button { class: "bg-active border border-subtle rounded-sm text-primary text-[10px] px-1.5 py-px cursor-pointer", disabled: busy, onclick: move |_| add(), "Add" }
        }
        if !offers.is_empty() {
            div { class: "flex gap-1 mt-1", style: "flex-wrap: wrap;",
                for name in offers {
                    button { key: "{name}", class: SMALL,
                        onclick: move |_| { with(|editor| editor.entity_panel.component_search = name.clone()); bump(); },
                        "{name}"
                    }
                }
            }
        }
    }
}

/// One component: its name, which opens it, and open its form, or its text where it has no
/// form or the switch shows the text.
#[component]
fn ComponentBox(entry: ComponentEntry, busy: bool, shown: bool, json: bool) -> Element {
    let ComponentEntry { row, own, fields } = entry;
    let marker = if row.draft { "  (new)" } else if row.edited { "  (edited)" } else if own { "  \u{25cf}" } else { "" };
    let (toggle, switch, text_name, apply, revert) = (row.name.clone(), row.name.clone(), row.name.clone(), row.name.clone(), row.name.clone());
    let lines = row.text.lines().count().clamp(2, 14);
    let text_shown = fields.is_none() || json;
    rsx! {
        div { class: "border-b border-surface py-0.5",
            button { class: "flex items-center gap-1.5 w-full px-1 py-0.5 bg-transparent hover:bg-hover border-none rounded-sm text-[11px] cursor-pointer text-left select-none",
                style: if shown { "color: #e0e0e0;" } else { "color: #aaa;" },
                onclick: move |_| {
                    with(|editor| {
                        if !editor.entity_panel.open.remove(&toggle) {
                            editor.entity_panel.open.insert(toggle.clone());
                        }
                    });
                    bump();
                },
                span { class: "flex items-center justify-center", style: "width: 8px;", Chevron { open: shown } }
                "{row.name}{marker}"
            }
            if shown {
                if row.reserved {
                    pre { class: "my-0.5 ml-3 whitespace-pre-wrap", style: "font-size: 9px; color: #999;", "{row.text}" }
                    div { class: "text-muted text-[10px] ml-3", "Moving, turning or renaming the entity changes this." }
                } else {
                    if let Some(fields) = fields {
                        div { class: "ml-2",
                            for field in fields {
                                {
                                    let tag = field.tag.clone();
                                    rsx! { FieldLine { key: "{tag}", component: row.name.clone(), field, busy } }
                                }
                            }
                        }
                        div { class: "flex gap-1 mt-0.5",
                            button { class: if json { CHOSEN } else { SMALL },
                                onclick: move |_| {
                                    with(|editor| {
                                        if !editor.entity_panel.json.remove(&switch) {
                                            editor.entity_panel.json.insert(switch.clone());
                                        }
                                    });
                                    bump();
                                },
                                "JSON"
                            }
                            if !json {
                                RemoveButton { row: row.clone(), busy }
                            }
                        }
                    }
                    if text_shown {
                        textarea { class: "w-full bg-surface border border-subtle rounded-sm text-primary font-mono outline-none focus:border-accent",
                            style: "font-size: 10px; height: {lines * 14 + 8}px;",
                            value: "{row.text}",
                            onfocus: move |_| with(|editor| editor.typing = true),
                            onblur: move |_| with(|editor| editor.typing = false),
                            oninput: move |event| {
                                let text = event.value();
                                with(|editor| editor.session.set_component_text(&text_name, text));
                                bump();
                            },
                        }
                        div { class: "flex gap-1 mt-0.5",
                            button { class: SMALL, disabled: busy || !(row.edited || row.draft),
                                onclick: move |_| { with(|editor| editor.session.apply_component(&apply)); bump(); },
                                "Apply"
                            }
                            button { class: SMALL, disabled: !row.edited,
                                onclick: move |_| { with(|editor| editor.session.revert_component(&revert)); bump(); },
                                "Revert"
                            }
                            RemoveButton { row: row.clone(), busy }
                        }
                    }
                }
            }
        }
    }
}

/// Reset, Remove or Drop of a component, as fits it.
#[component]
fn RemoveButton(row: ComponentRow, busy: bool) -> Element {
    let label = match (row.draft, row.from_prototype) {
        (true, _) => "Drop",
        (false, true) => "Reset",
        (false, false) => "Remove",
    };
    let name = row.name.clone();
    rsx! {
        button { class: SMALL, style: "color: #cc4444;", disabled: busy,
            onclick: move |_| { with(|editor| editor.session.remove_component(&name)); bump(); },
            "{label}"
        }
    }
}

/// What a field's widget hands on, for the session to check and send.
enum FormAction {
    Typed(String),
    Value(Value),
    Flag(String, bool),
    Reset,
}

fn act(component: &str, tag: &str, action: FormAction) {
    with(|editor| match action {
        FormAction::Typed(text) => editor.session.set_component_field(component, tag, &text),
        FormAction::Value(value) => editor.session.set_component_value(component, tag, value),
        FormAction::Flag(flag, on) => editor.session.set_component_flag(component, tag, &flag, on),
        FormAction::Reset => editor.session.reset_component_field(component, tag),
    });
    bump();
}

/// One field of a form: its name, a widget for its kind, and where the entity's own data sets
/// it a mark and a reset. A bool is a checkbox, an enum a row of its choices or a list that
/// opens below, flags a row of switches, a colour its hex text with a swatch, an `Other` JSON
/// text. Typed text is handed on with Enter or when the field lets go, and nothing typed gives
/// the field back to the prototype.
#[component]
fn FieldLine(component: String, field: FieldRow, busy: bool) -> Element {
    let _ = watch();
    let key = (component.clone(), field.tag.clone());
    let (typed, listing) = with(|editor| (editor.entity_panel.fields.get(&key).cloned(), editor.entity_panel.listing.as_ref() == Some(&key)));
    let disabled = busy || field.read_only;
    let send = {
        let (component, tag) = (component.clone(), field.tag.clone());
        move |action: FormAction| act(&component, &tag, action)
    };
    let label = if field.own { "color: #4488ff;" } else { "color: #888;" };
    let widget = match field.kind {
        FieldKind::Bool => {
            let send = send.clone();
            rsx! { input { r#type: "checkbox", class: "check", checked: field.value.as_bool().unwrap_or(false), disabled, oninput: move |event| send(FormAction::Value(json!(event.checked()))) } }
        }
        FieldKind::Enum if !field.names.is_empty() && field.names.len() <= SHORT_LIST => rsx! {
            for name in field.names.clone() {
                {
                    let send = send.clone();
                    let chosen = name == field.text;
                    rsx! { button { key: "{name}", class: if chosen { CHOSEN } else { SMALL }, disabled, onclick: move |_| send(FormAction::Value(json!(name.clone()))), "{name}" } }
                }
            }
        },
        FieldKind::Enum if !field.names.is_empty() => {
            let toggle = key.clone();
            rsx! {
                button { class: if listing { CHOSEN } else { SMALL }, disabled,
                    onclick: move |_| {
                        with(|editor| editor.entity_panel.listing = if listing { None } else { Some(toggle.clone()) });
                        bump();
                    },
                    span { class: "flex items-center gap-1.5",
                        if field.text.is_empty() { "none" } else { "{field.text}" }
                        Chevron { open: listing }
                    }
                }
            }
        }
        FieldKind::Flags if !field.names.is_empty() => {
            let set = flag_set(&field.value);
            rsx! {
                for flag in field.names.clone() {
                    {
                        let send = send.clone();
                        let on = set.contains(&flag);
                        rsx! { button { key: "{flag}", class: if on { CHOSEN } else { SMALL }, disabled, onclick: move |_| send(FormAction::Flag(flag.clone(), !on)), "{flag}" } }
                    }
                }
            }
        }
        FieldKind::Color => {
            let swatch = decal_colour_bytes(&field.text).map(|[red, green, blue, alpha]| format!("rgba({red}, {green}, {blue}, {})", alpha as f32 / 255.0));
            let typed_send = send.clone();
            let pipe = component == "AtmosPipeColor";
            rsx! {
                if let Some(swatch) = swatch {
                    span { class: "border border-subtle rounded-sm", style: "background: {swatch}; width: 14px; height: 14px; flex-shrink: 0;" }
                }
                ColourField { value: field.text.clone(), disabled, onchange: move |text: String| typed_send(FormAction::Typed(text)) }
                // The pipe colours of the pipe tool, as quick choices.
                if pipe {
                    for (label, colour) in PIPE_COLOURS {
                        {
                            let send = send.clone();
                            rsx! { button { key: "{label}", class: SMALL, disabled, onclick: move |_| send(FormAction::Typed(colour.to_owned())), "{label}" } }
                        }
                    }
                }
            }
        }
        FieldKind::Other => {
            let shown = typed.clone().unwrap_or_else(|| field.text.clone());
            let lines = shown.lines().count().clamp(1, 8);
            let (input_key, leave_key) = (key.clone(), key.clone());
            let send = send.clone();
            rsx! {
                textarea { class: "w-full bg-surface border border-subtle rounded-sm text-primary font-mono outline-none focus:border-accent",
                    style: "font-size: 10px; height: {lines * 14 + 8}px;",
                    value: "{shown}",
                    disabled,
                    onfocus: move |_| with(|editor| editor.typing = true),
                    onblur: move |_| {
                        with(|editor| editor.typing = false);
                        if let Some(text) = with(|editor| editor.entity_panel.fields.remove(&leave_key)) {
                            send(FormAction::Typed(text));
                        }
                    },
                    oninput: move |event| {
                        let text = event.value();
                        with(|editor| editor.entity_panel.fields.insert(input_key.clone(), text));
                        bump();
                    },
                }
            }
        }
        _ => {
            let shown = typed.clone().unwrap_or_else(|| field.text.clone());
            let (input_key, enter_key, leave_key) = (key.clone(), key.clone(), key.clone());
            let take = {
                let send = send.clone();
                move |key: (String, String)| {
                    if let Some(text) = with(|editor| editor.entity_panel.fields.remove(&key)) {
                        send(FormAction::Typed(text));
                    }
                }
            };
            let take_too = take.clone();
            rsx! {
                TextField { value: shown, placeholder: "none", disabled,
                    class: "flex-1 px-1 py-0.5 bg-surface border border-subtle rounded-sm text-primary text-[11px] outline-none focus:border-accent w-full",
                    oninput: move |text: String| { with(|editor| editor.entity_panel.fields.insert(input_key.clone(), text)); bump(); },
                    onenter: move |_| take(enter_key.clone()),
                    onleave: move |_| take_too(leave_key.clone()),
                }
            }
        }
    };
    let reset = send.clone();
    rsx! {
        div { class: "flex items-center gap-1.5 py-px",
            span { class: "text-[10px]", style: "width: 90px; flex-shrink: 0; overflow: hidden; {label}", "{field.tag}" }
            div { class: "flex items-center gap-1 flex-1 min-w-0", style: "flex-wrap: wrap;",
                {widget}
                if field.read_only {
                    span { class: "text-muted text-[9px]", "read-only" }
                }
            }
            if field.own {
                button { class: SMALL, disabled: busy, onclick: move |_| reset(FormAction::Reset), "Reset" }
            }
        }
        // An enum's long list, open below its field.
        if listing {
            div { class: "flex gap-1 mb-1", style: "flex-wrap: wrap; margin-left: 96px;",
                for name in field.names.clone() {
                    {
                        let send = send.clone();
                        let chosen = name == field.text;
                        rsx! {
                            button { key: "{name}", class: if chosen { CHOSEN } else { SMALL }, disabled,
                                onclick: move |_| {
                                    with(|editor| editor.entity_panel.listing = None);
                                    send(FormAction::Value(json!(name.clone())));
                                },
                                "{name}"
                            }
                        }
                    }
                }
            }
        }
    }
}
