//! The selection panel. With one entity selected it shows what is known about it, with several it
//! shows a summary, and there are buttons to turn, delete and deselect them. With only decals
//! selected it shows their fields. Also the layers. Uses the markup of the web editor's
//! EntityInfoPanel.tsx, DecalInfoPanel.tsx and LayerPanel.tsx. The cable and pipe choices are in
//! the options bar (options.rs).

use dioxus_native::prelude::*;
use editor_core::air::AirView;
use editor_core::session::{decal_colour_bytes, DecalField, Session};
use editor_core::shell::{layer, LAYER_NAMES, T_RAY_LAYER};

use crate::palettes::{ColourField, NumberField};
use crate::{bump, watch, with};

/// A row of a label and its value, as the web editor's entity panel has them.
#[component]
fn Field(label: String, value: String) -> Element {
    rsx! {
        div { class: "flex justify-between py-px",
            span { class: "text-muted", "{label}:" }
            span { class: "text-primary text-right", style: "max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;", title: "{value}", "{value}" }
        }
    }
}

/// One of the panel's buttons, which do what their keys do. Delete is in the web editor's red.
type SelectionButton = (&'static str, &'static str, fn(&mut Session));

#[component]
fn Buttons(buttons: Vec<(&'static str, &'static str, bool)>) -> Element {
    rsx! {
        div { class: "flex gap-1 mt-2", style: "flex-wrap: wrap;",
            for (index, (label, key, danger)) in buttons.into_iter().enumerate() {
                button {
                    class: "flex-1 px-1.5 py-1 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover",
                    style: if danger { "color: #cc4444;" } else { "" },
                    title: "{key}",
                    onclick: move |_| {
                        with(|editor| {
                            let actions = buttons_of(&editor.session);
                            if let Some((_, _, action)) = actions.get(index) {
                                action(&mut editor.session);
                            }
                        });
                        bump();
                    },
                    "{label}"
                }
            }
        }
    }
}

/// The buttons for what is selected now.
fn buttons_of(session: &Session) -> Vec<SelectionButton> {
    let several = session.selection.len() > 1;
    let (left, right, delete, deselect) = if several { ("Turn all left", "Turn all right", "Delete all", "Deselect all") } else if session.selection.is_empty() { ("Turn left", "Turn right", "Delete", "Deselect") } else { ("Turn left", "Turn right", "Delete", "Deselect") };
    vec![
        (left, "Shift+R", |session: &mut Session| session.rotate_selection(false)),
        (right, "R", |session: &mut Session| session.rotate_selection(true)),
        (delete, "Delete", |session: &mut Session| session.delete_selection()),
        (deselect, "Escape", |session: &mut Session| session.deselect()),
    ]
}

#[component]
pub(crate) fn SelectionPanel() -> Element {
    let _ = watch();
    let (count, decals, buttons) = with(|editor| {
        let session = &editor.session;
        let buttons: Vec<(&'static str, &'static str, bool)> = buttons_of(session).into_iter().map(|(label, key, _)| (label, key, label.starts_with("Delete"))).collect();
        (session.selection.len(), session.selected_decals.len(), buttons)
    });
    rsx! {
        div { class: "p-3 text-xs",
            match count {
                0 if decals > 0 => rsx! { DecalPanel {} },
                0 => rsx! {},
                1 => rsx! { OneSelected { buttons: buttons.clone() } },
                _ => rsx! { SeveralSelected { buttons: buttons.clone() } },
            }
            if decals > 0 && count > 0 {
                div { class: "text-muted text-[11px] mt-1", if decals == 1 { "and 1 decal" } else { "and {decals} decals" } }
            }
        }
    }
}

#[component]
fn SeveralSelected(buttons: Vec<(&'static str, &'static str, bool)>) -> Element {
    let _ = watch();
    let Some((heading, breakdown)) = with(|editor| editor.session.selection_summary()) else { return rsx! {} };
    rsx! {
        div { class: "flex justify-between items-center mb-1.5",
            span { class: "font-bold text-xs", "{heading}" }
        }
        div { class: "text-muted text-[11px] mb-1.5", "{breakdown}" }
        Buttons { buttons }
    }
}

#[component]
fn OneSelected(buttons: Vec<(&'static str, &'static str, bool)>) -> Element {
    let _ = watch();
    let found = with(|editor| {
        let session = &editor.session;
        let (Some(doc), &[uid]) = (session.doc.as_ref(), &session.selection[..]) else { return None };
        let entity = doc.entities.get(&uid)?;
        let prototype = session.prototype(&entity.prototype);
        // Its own name if the map gives it one, or else its prototype's.
        let name = entity.name.clone().or_else(|| prototype.map(|prototype| prototype.name.clone())).unwrap_or_default();
        let category = session.palette_category(&entity.prototype).unwrap_or("...").to_owned();
        let description = prototype.map(|prototype| prototype.description.clone()).unwrap_or_default();
        let parent = match doc.grid(entity.parent) {
            Some(grid) => format!("grid {} ({})", grid.uid, grid.name),
            None if entity.parent == doc.map_uid => "the map".to_owned(),
            None => format!("entity {}", entity.parent),
        };
        let degrees = entity.rotation.to_degrees().rem_euclid(360.0).round() as i64 % 360;
        let rows = vec![
            ("Name", name),
            ("Prototype", entity.prototype.clone()),
            ("Uid", if entity.yaml_uid != 0 { format!("{} (in file: {})", entity.uid, entity.yaml_uid) } else { format!("{} (new)", entity.uid) }),
            ("Position", format!("{:.1}, {:.1}", entity.x, entity.y)),
            ("Rotation", format!("{degrees}\u{b0}")),
            ("Category", category),
            ("Anchored", if entity.anchored { "yes" } else { "no" }.to_owned()),
            ("On", parent),
        ];
        Some((uid, rows, description))
    });
    let Some((uid, rows, description)) = found else { return rsx! {} };
    rsx! {
        div { class: "flex justify-between items-center mb-1.5",
            span { class: "font-bold text-xs", "Entity info" }
            button { class: "bg-transparent border-none text-muted text-[16px] cursor-pointer px-1 leading-none", title: "Deselect",
                onclick: move |_| { with(|editor| editor.session.deselect()); bump(); },
                "\u{d7}"
            }
        }
        // The buttons come first, where they stay in view however long the rest is.
        Buttons { buttons }
        div { class: "mt-2",
            for (label, value) in rows {
                Field { label, value }
            }
        }
        if !description.is_empty() {
            div { class: "text-muted text-[11px] mt-1 italic", "{description}" }
        }
        // Its links and list entries, then its name, its containers, and what the map file
        // stores for it beyond its prototype, each editable.
        crate::links::LinksSection { uid }
        crate::components::EntityEditor { uid }
    }
}

/// The selected decals, when no entity is selected: for one its fields, and for several a
/// summary, each field editable, with the web editor's Select All and Recolor All. A field
/// they do not share says "mixed", and a change sets it on all of them.
#[component]
fn DecalPanel() -> Element {
    let _ = watch();
    let found = with(|editor| {
        let summary = editor.session.decal_summary()?;
        let one = (summary.count == 1).then(|| editor.session.selected_decal_list().into_iter().next()).flatten();
        Some((summary, one, editor.recolor.clone()))
    });
    let Some((summary, one, recolor)) = found else { return rsx! {} };
    let change = |field: DecalField| {
        with(|editor| {
            editor.session.edit_selected_decals(field);
            // Each change here is whole, typed and sent, so it ends there.
            editor.session.end_decal_edit();
        });
        bump();
    };
    let buttons = vec![("Turn left", "Shift+R", false), ("Turn right", "R", false), ("Delete", "Delete", true), ("Deselect", "Escape", false)];
    let own = summary.color.clone().flatten();
    let swatch = own.as_deref().and_then(decal_colour_bytes).map(|[red, green, blue, alpha]| format!("rgba({red}, {green}, {blue}, {})", alpha as f32 / 255.0));
    let degrees = format!("{:.1}", summary.angle.unwrap_or(0.0).to_degrees());
    let z_index = summary.z_index.unwrap_or(0).to_string();
    let cleanable = summary.cleanable.unwrap_or(false);
    let mixed = |shared: bool| rsx! {
        if !shared {
            span { class: "text-muted italic text-[11px]", "mixed" }
        }
    };
    rsx! {
        div { class: "flex justify-between items-center mb-1.5",
            span { class: "font-bold text-xs", if summary.count == 1 { "Decal info" } else { "{summary.count} decals selected" } }
        }
        Buttons { buttons }
        div { class: "mt-2",
            if let Some((grid, decal)) = one {
                Field { label: "Prototype", value: decal.prototype.clone() }
                Field { label: "Id", value: format!("{} on grid {grid}", decal.id) }
                Field { label: "Position", value: format!("{:.2}, {:.2}", decal.x, decal.y) }
            } else {
                div { class: "text-muted text-[11px] mb-1.5", "{summary.breakdown}" }
            }
        }
        div { class: "mt-2 space-y-1.5",
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Colour" }
                if let Some(swatch) = swatch {
                    span { class: "border border-subtle rounded-sm", style: "background: {swatch}; width: 16px; height: 16px; flex-shrink: 0;" }
                }
                match (&summary.color, own.clone()) {
                    (Some(_), Some(text)) => rsx! {
                        ColourField { value: text, onchange: move |text: String| change(DecalField::Color(Some(text))) }
                        button { class: "px-1.5 py-0.5 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover", title: "Leave the decal's own colours",
                            onclick: move |_| change(DecalField::Color(None)),
                            "No colour"
                        }
                    },
                    (Some(_), None) => rsx! {
                        ColourField { value: "", onchange: move |text: String| change(DecalField::Color(Some(text))) }
                        span { class: "text-muted text-[11px]", "none" }
                    },
                    (None, _) => rsx! {
                        ColourField { value: "", onchange: move |text: String| change(DecalField::Color(Some(text))) }
                        {mixed(false)}
                    },
                }
            }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Angle" }
                NumberField { value: degrees, onchange: move |value: f64| change(DecalField::Angle(value.to_radians())) }
                span { class: "text-[11px] text-muted", "deg" }
                {mixed(summary.angle.is_some())}
            }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Z-index" }
                NumberField { value: z_index, onchange: move |value: f64| change(DecalField::ZIndex(value.round() as i32)) }
                {mixed(summary.z_index.is_some())}
            }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Cleanable" }
                input { r#type: "checkbox", class: "check", checked: cleanable, oninput: move |event| change(DecalField::Cleanable(event.checked())) }
                {mixed(summary.cleanable.is_some())}
            }
        }
        div { class: "flex gap-1 mt-2", style: "flex-wrap: wrap;",
            if let Some(prototype) = summary.prototype.clone() {
                button { class: "px-1.5 py-1 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover",
                    title: "Select every {prototype} decal on the grid",
                    onclick: move |_| { with(|editor| editor.session.select_decals_like()); bump(); },
                    "Select all of this kind"
                }
            }
            if summary.color.is_some() && summary.same_colour > 1 {
                button { class: "px-1.5 py-1 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover",
                    title: "Select every decal on the grid with this colour",
                    onclick: move |_| { with(|editor| editor.session.select_decals_coloured()); bump(); },
                    "Select all ({summary.same_colour})"
                }
            }
        }
        if summary.color.is_some() && summary.same_colour > 1 {
            div { class: "flex items-center gap-2 mt-2",
                span { class: "text-muted text-[11px]", "Recolour all to" }
                ColourField { value: recolor, onchange: move |text: String| { with(|editor| editor.recolor = text); bump(); } }
                button { class: "px-1.5 py-0.5 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover",
                    title: "Change the {summary.same_colour} decals with this colour to the colour chosen here",
                    onclick: move |_| {
                        with(|editor| {
                            let colour = editor.recolor.clone();
                            editor.session.recolor_decals(Some(colour));
                        });
                        bump();
                    },
                    "Apply"
                }
            }
        }
    }
}

/// The layers panel: a checkbox for each layer, the connections view, the air overlay with its
/// four views, and the Problems overlay.
///
/// Every checkbox here and elsewhere listens for `input`. Blitz ticks a clicked box itself and
/// sends only an `input` event. It has no `change` event, so `onchange` is never called.
#[component]
pub(crate) fn LayersPanel() -> Element {
    let _ = watch();
    let (mut layers, connections, air, air_view, problems) =
        with(|editor| (editor.session.layers, editor.session.show_connections, editor.session.show_air, editor.session.air_view, editor.session.problems_shown()));
    let shown: Vec<(usize, &str, bool)> = LAYER_NAMES.iter().enumerate().map(|(index, name)| (index, *name, *layer(&mut layers, index))).collect();
    rsx! {
        div { class: "p-3",
            for (index, name, on) in shown {
                if index == T_RAY_LAYER {
                    div { class: "h-px bg-subtle my-2" }
                }
                label { class: "flex items-center gap-2 py-0.5 text-primary text-[11px] cursor-pointer select-none",
                    input { r#type: "checkbox", class: "check accent-accent w-3 h-3", checked: on,
                        oninput: move |event| {
                            let on = event.checked();
                            with(|editor| {
                                *layer(&mut editor.session.layers, index) = on;
                                editor.session.sync_layers();
                            });
                            bump();
                        }
                    }
                    "{name}"
                }
            }
            label { class: "flex items-center gap-2 py-0.5 text-primary text-[11px] cursor-pointer select-none", title: "Show device links and connections",
                input { r#type: "checkbox", class: "check accent-accent w-3 h-3", checked: connections,
                    oninput: move |event| {
                        let on = event.checked();
                        with(|editor| editor.session.show_connections = on);
                        bump();
                    }
                }
                "Connections"
            }
            label { class: "flex items-center gap-2 py-0.5 text-primary text-[11px] cursor-pointer select-none", title: "Show the air each tile stores",
                input { r#type: "checkbox", class: "check accent-accent w-3 h-3", checked: air,
                    oninput: move |event| {
                        let on = event.checked();
                        with(|editor| editor.session.show_air = on);
                        bump();
                    }
                }
                "Air"
            }
            // The views beneath it. Choosing one shows the overlay.
            div { class: "flex gap-1 mt-1", style: if air { "margin-left: 20px;" } else { "margin-left: 20px; opacity: 0.55;" },
                for view in AirView::ALL {
                    button {
                        class: if air && view == air_view { "flex-1 px-1.5 py-1 rounded-sm border border-accent text-[11px] cursor-pointer bg-accent text-white" } else { "flex-1 px-1.5 py-1 rounded-sm border border-subtle text-primary text-[11px] cursor-pointer bg-elevated hover:bg-hover" },
                        onclick: move |_| {
                            with(|editor| {
                                editor.session.air_view = view;
                                editor.session.show_air = true;
                            });
                            bump();
                        },
                        "{view.name()}"
                    }
                }
            }
            label { class: "flex items-center gap-2 py-0.5 mt-1 text-primary text-[11px] cursor-pointer select-none", title: "Mark stacked walls, duplicates and walls with no floor on the map",
                input { r#type: "checkbox", class: "check accent-accent w-3 h-3", checked: problems,
                    oninput: move |event| {
                        let on = event.checked();
                        with(|editor| editor.session.show_problems(on));
                        bump();
                    }
                }
                "Problems"
            }
        }
    }
}
