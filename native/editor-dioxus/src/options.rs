//! The options bar: a strip across the top of the map that shows the options of the tool in
//! hand, as an art program has one. What Paint and Erase work on and in what shape, what
//! Select selects and how, the Room tool's floor and walls, the cables and pipes, and a line
//! of help for the tools with no options.

use dioxus_native::prelude::*;
use editor_core::session::{decal_colour_bytes, CableKind, PipeNetwork, PIPE_COLOURS};
use editor_core::toolbox::{BarTool, SelectMode, Shape, Target};

use crate::icons::Icon;
use crate::palettes::{ColourField, NumberField, PaletteTab};
use crate::theme::{ICON, ICON_ON};
use crate::{bump, watch, with, Editor};

/// A disposal pipe's swatch, the colour the cable and pipe panel always gave it.
const DISPOSAL: (u8, u8, u8) = (0x88, 0x66, 0x44);

/// One button of a row of buttons that go together.
#[derive(Clone, PartialEq)]
struct Choice {
    icon: Option<&'static str>,
    text: &'static str,
    on: bool,
    enabled: bool,
}

impl Choice {
    fn new(icon: &'static str, text: &'static str, on: bool) -> Choice {
        Choice { icon: Some(icon), text, on, enabled: true }
    }
}

/// A label and a row of buttons joined into one control. A press calls `onpick` with the
/// button's place in the row.
#[component]
fn Choices(label: &'static str, items: Vec<Choice>, onpick: EventHandler<usize>) -> Element {
    rsx! {
        if !label.is_empty() {
            span { class: "options-label", "{label}" }
        }
        div { class: "segmented",
            for (at, item) in items.into_iter().enumerate() {
                button { key: "{at}",
                    class: match (item.on, item.enabled) { (true, _) => "segment on", (false, true) => "segment", (false, false) => "segment off" },
                    disabled: !item.enabled,
                    onclick: move |_| {
                        if item.enabled {
                            onpick.call(at);
                        }
                    },
                    if let Some(icon) = item.icon {
                        Icon { name: icon, colour: if item.on { ICON_ON } else { ICON }, size: 16 }
                    }
                    span { "{item.text}" }
                }
            }
        }
    }
}

/// The icon of a target.
fn target_icon(target: Target) -> &'static str {
    match target {
        Target::Tiles => "tiles",
        Target::Entities => "entities",
        Target::Decals => "decals",
    }
}

/// The icon of a shape.
fn shape_icon(shape: Shape) -> &'static str {
    match shape {
        Shape::Point => "point",
        Shape::Line => "line",
        Shape::Rectangle => "rectangle",
        Shape::Circle => "circle",
    }
}

/// The palette tab of a target.
fn target_tab(target: Target) -> PaletteTab {
    match target {
        Target::Tiles => PaletteTab::Tiles,
        Target::Entities => PaletteTab::Entities,
        Target::Decals => PaletteTab::Decals,
    }
}

/// Sets the target and shows its palette.
fn pick_target(editor: &mut Editor, target: Target) {
    editor.session.set_target(target);
    if editor.session.target() == target {
        editor.tab = target_tab(target);
        editor.seen_target = Some(target);
    }
}

/// The palette follows the target: when the target changed since the last frame, by a choice
/// or a key, a palette tab of a target turns to the new one. The favourites, the prefabs and
/// the grids stay, since a choice there should not take the list away.
pub(crate) fn follow_target(editor: &mut Editor) {
    let target = editor.session.target();
    if editor.seen_target == Some(target) {
        return;
    }
    editor.seen_target = Some(target);
    if matches!(editor.tab, PaletteTab::Tiles | PaletteTab::Entities | PaletteTab::Decals) {
        editor.tab = target_tab(target);
    }
}

/// The options bar. Every part of it that reads the editor calls `watch`, as this does: a
/// component with no props is never drawn again because its parent was, so a part that read
/// the editor without it kept showing the state it first drew with.
#[component]
pub(crate) fn OptionsBar() -> Element {
    let _ = watch();
    let (bar, target, shape) = with(|editor| (editor.session.bar_tool(), editor.session.target(), editor.session.shape()));
    let point = shape == Some(Shape::Point);
    rsx! {
        div { class: "options-bar",
            onmousedown: move |event| event.stop_propagation(),
            span { class: "options-tool", "{bar.name()}" }
            div { class: "options-rule" }
            match bar {
                BarTool::Paint | BarTool::Erase | BarTool::Fill => rsx! {
                    TargetChoices { target }
                    if let Some(shape) = shape {
                        div { class: "options-rule" }
                        ShapeChoices { shape }
                    }
                    if bar == BarTool::Paint && target != Target::Tiles && point {
                        div { class: "options-rule" }
                        PlaceChoices {}
                    }
                    if bar == BarTool::Paint && target == Target::Entities {
                        div { class: "options-rule" }
                        crate::placing::WallPlacement {}
                    }
                    if bar == BarTool::Paint && target == Target::Decals {
                        div { class: "options-rule" }
                        DecalOptions {}
                    }
                    if bar == BarTool::Erase {
                        div { class: "options-rule" }
                        EraseOptions {}
                    }
                    span { class: "hint-text", "{target_hint(bar, target, point)}" }
                },
                BarTool::Select => rsx! { SelectOptions {} },
                BarTool::Room => rsx! { crate::placing::RoomOptions {} },
                BarTool::Cables => rsx! { CableOptions {} },
                BarTool::Pipes => rsx! { PipeOptions {} },
                BarTool::Pick => rsx! { span { class: "hint-text", "Click to take what's under the cursor into the palette. Over a stack, the wheel chooses" } },
                BarTool::Pan => rsx! { span { class: "hint-text", "Drag to move the map. Middle drag or Space+drag pans with any tool" } },
                BarTool::Link => rsx! { span { class: "hint-text", "Click a device, then another to link them. Shift+click links a device that also has a list" } },
            }
        }
    }
}

/// The line of help after the options of Paint, Erase and Fill, for their target. Paint of
/// tiles, and of entities at a point, says nothing more than its options do.
fn target_hint(bar: BarTool, target: Target, point: bool) -> &'static str {
    match (bar, target) {
        (BarTool::Paint, Target::Tiles) => "",
        (BarTool::Paint, Target::Entities) if point => "",
        (BarTool::Paint, Target::Entities) => "Skips cells with no tile. Walls get one if Tile under walls is on",
        (BarTool::Paint, Target::Decals) => "Skips cells with no tile",
        (BarTool::Erase, Target::Tiles) => "Entities and decals stay",
        (BarTool::Erase, Target::Entities) => "Removes what you can see. Hide a layer in Layers to keep what's on it",
        (BarTool::Erase, Target::Decals) => "Removes the decals in the shape while the decal layer shows",
        (BarTool::Fill, Target::Tiles) => "Fills the area of the same tile, up to 50,000 tiles",
        (BarTool::Fill, _) => "One per cell over the area of the same floor, up to 10,000 cells",
        _ => "",
    }
}

/// What Paint, Erase and Fill work on.
#[component]
fn TargetChoices(target: Target) -> Element {
    let items: Vec<Choice> = Target::ALL.iter().map(|&each| Choice::new(target_icon(each), each.name(), each == target)).collect();
    rsx! {
        Choices { label: "Target", items,
            onpick: move |at: usize| {
                with(|editor| pick_target(editor, Target::ALL[at]));
                bump();
            }
        }
    }
}

/// The shape Paint or Erase works in.
#[component]
fn ShapeChoices(shape: Shape) -> Element {
    let items: Vec<Choice> = Shape::ALL.iter().map(|&each| Choice::new(shape_icon(each), each.name(), each == shape)).collect();
    rsx! {
        Choices { label: "Shape", items,
            onpick: move |at: usize| {
                with(|editor| editor.session.set_shape(Shape::ALL[at]));
                bump();
            }
        }
    }
}

/// Paint at a point on entities or decals: on the cell under the cursor or where the cursor is.
/// Shift does the other for one click.
#[component]
fn PlaceChoices() -> Element {
    let _ = watch();
    let free = with(|editor| editor.session.place_free());
    let items = vec![Choice { icon: None, text: "Snap to cell", on: !free, enabled: true }, Choice { icon: None, text: "Free", on: free, enabled: true }];
    rsx! {
        Choices { label: "Place", items,
            onpick: move |at: usize| {
                with(|editor| editor.session.set_place_free(at == 1));
                bump();
            }
        }
    }
}

/// Erase's Only: take away just the kind in hand, named on the switch.
#[component]
fn EraseOptions() -> Element {
    let _ = watch();
    let (only, named, target) = with(|editor| (editor.session.erase_only(), editor.session.erase_only_id(), editor.session.target()));
    let text = match (named, target) {
        (Some(id), _) => format!("Only {id}"),
        (None, Target::Tiles) => "Only the tile in hand".to_owned(),
        (None, Target::Entities) => "Only the entity in hand".to_owned(),
        (None, Target::Decals) => "Only the decal in hand".to_owned(),
    };
    rsx! {
        label { class: "option-check",
            input { r#type: "checkbox", class: "check", checked: only,
                oninput: move |event| {
                    let on = event.checked();
                    with(|editor| editor.session.set_erase_only(on));
                    bump();
                }
            }
            "{text}"
        }
    }
}

/// Paint on decals: what a decal is placed with. A colour is typed as #RRGGBBAA, since Blitz
/// has no colour picker.
#[component]
fn DecalOptions() -> Element {
    let _ = watch();
    let settings = with(|editor| editor.session.decal_settings().clone());
    let set = |change: Box<dyn FnOnce(&mut editor_core::session::DecalSettings)>| {
        with(|editor| {
            let mut settings = editor.session.decal_settings().clone();
            change(&mut settings);
            editor.session.set_decal_settings(settings);
        });
        bump();
    };
    let degrees = format!("{:.1}", settings.angle.to_degrees());
    let swatch = settings.color.as_deref().and_then(decal_colour_bytes).map(|[red, green, blue, alpha]| format!("rgba({red}, {green}, {blue}, {})", alpha as f32 / 255.0));
    rsx! {
        label { class: "option-check",
            input { r#type: "checkbox", class: "check", checked: settings.color.is_some(),
                oninput: move |event| {
                    let on = event.checked();
                    set(Box::new(move |settings| settings.color = on.then(|| "#FFFFFFFF".to_owned())));
                }
            }
            "Colour"
        }
        if let Some(text) = settings.color.clone() {
            span { class: "swatch", style: "background: {swatch.clone().unwrap_or_default()};" }
            ColourField { value: text, onchange: move |text: String| set(Box::new(move |settings| settings.color = Some(text))) }
        }
        span { class: "options-label", "Angle" }
        div { class: "option-number", NumberField { value: degrees, onchange: move |value: f64| set(Box::new(move |settings| settings.angle = value.clamp(0.0, 360.0).to_radians())) } }
        span { class: "options-label", "Z" }
        div { class: "option-number", NumberField { value: settings.z_index.to_string(), onchange: move |value: f64| set(Box::new(move |settings| settings.z_index = value.round() as i32)) } }
        label { class: "option-check",
            input { r#type: "checkbox", class: "check", checked: settings.cleanable, oninput: move |event| { let on = event.checked(); set(Box::new(move |settings| settings.cleanable = on)); } }
            "Cleanable"
        }
    }
}

/// What Select selects, its shape, and whether a press replaces the selection, adds to it or
/// takes from it. With Tiles on it selects a region of cells, which takes everything on them,
/// so Entities and Decals show off, and a press of either leaves the region for that one alone
/// (`SelectTargets::toggle`).
#[component]
fn SelectOptions() -> Element {
    let _ = watch();
    let (targets, mode, region) = with(|editor| (editor.session.select_targets(), editor.session.select_mode(), editor.session.region_size()));
    let order = [Target::Entities, Target::Decals, Target::Tiles];
    let items: Vec<Choice> = order.iter().map(|&each| Choice::new(target_icon(each), each.name(), targets.lit(each))).collect();
    let modes: Vec<Choice> = SelectMode::ALL.iter().map(|&each| Choice::new(mode_icon(each), each.name(), each == mode)).collect();
    let hint = match (targets.tiles, region) {
        (true, 0) => "A region takes everything on its cells. Pick Entities or Decals to select only those".to_owned(),
        (true, cells) => format!("{cells} cells. Right-click the region to copy, cut, delete or save it as a prefab"),
        (false, _) => "Shift adds and Ctrl subtracts for one click".to_owned(),
    };
    rsx! {
        Choices { label: "Select", items,
            onpick: move |at: usize| {
                with(|editor| editor.session.toggle_select_target(order[at]));
                bump();
            }
        }
        div { class: "options-rule" }
        Choices { label: "Shape", items: vec![Choice::new("box", "Rectangle", true)], onpick: move |_| {} }
        div { class: "options-rule" }
        Choices { label: "Mode", items: modes,
            onpick: move |at: usize| {
                with(|editor| editor.session.set_select_mode(SelectMode::ALL[at]));
                bump();
            }
        }
        span { class: "hint-text", "{hint}" }
    }
}

/// The icon of a select mode.
fn mode_icon(mode: SelectMode) -> &'static str {
    match mode {
        SelectMode::Replace => "replace",
        SelectMode::Add => "add",
        SelectMode::Subtract => "subtract",
    }
}

/// A swatch button: a colour and a name, chosen or not, greyed where the fork lacks it.
#[component]
fn SwatchChoice(swatch: String, name: String, chosen: bool, enabled: bool, onpick: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: match (chosen, enabled) { (true, _) => "segment on", (false, true) => "segment", (false, false) => "segment off" },
            disabled: !enabled,
            onclick: move |_| {
                if enabled {
                    onpick.call(());
                }
            },
            span { class: "swatch small", style: "background: {swatch};" }
            span { "{name}" }
        }
    }
}

fn rgb((red, green, blue): (u8, u8, u8)) -> String {
    format!("rgb({red}, {green}, {blue})")
}

/// The kind of cable the cable tool lays. A kind the fork does not have is greyed.
#[component]
fn CableOptions() -> Element {
    let _ = watch();
    let cables: Vec<(CableKind, bool, bool)> = with(|editor| {
        let session = &editor.session;
        CableKind::ALL.into_iter().map(|kind| (kind, session.cable_kind() == kind, session.cable_available(kind))).collect()
    });
    rsx! {
        span { class: "options-label", "Cable" }
        div { class: "segmented",
            for (kind, chosen, enabled) in cables {
                SwatchChoice { key: "{kind.label()}", swatch: rgb(kind.colour()), name: kind.label().to_owned(), chosen, enabled,
                    onpick: move |_| { with(|editor| editor.session.choose_cable(kind)); bump(); }
                }
            }
        }
        span { class: "hint-text", "Right-drag removes" }
    }
}

/// The pipe the pipe tool lays: a gas pipe of a colour, or a disposal pipe, and any other
/// colour for a gas pipe, or none for the prototype's own.
#[component]
fn PipeOptions() -> Element {
    let _ = watch();
    let (gas, disposal, network, current) = with(|editor| {
        let session = &editor.session;
        (session.pipe_network_available(PipeNetwork::Gas), session.pipe_network_available(PipeNetwork::Disposal), session.pipe_network(), session.pipe_colour().map(str::to_owned))
    });
    let choose = |network: PipeNetwork, colour: Option<String>| {
        with(|editor: &mut Editor| editor.session.choose_pipe(network, colour));
        bump();
    };
    rsx! {
        span { class: "options-label", "Pipe" }
        div { class: "segmented",
            for (name, text) in PIPE_COLOURS {
                {
                    let bytes = decal_colour_bytes(text).unwrap_or([255; 4]);
                    let chosen = network == PipeNetwork::Gas && current.as_deref() == Some(text);
                    rsx! {
                        SwatchChoice { key: "{name}", swatch: rgb((bytes[0], bytes[1], bytes[2])), name: name.to_owned(), chosen, enabled: gas,
                            onpick: move |_| choose(PipeNetwork::Gas, Some(text.to_owned()))
                        }
                    }
                }
            }
            SwatchChoice { swatch: rgb(DISPOSAL), name: "Disposal".to_owned(), chosen: network == PipeNetwork::Disposal, enabled: disposal,
                onpick: move |_| choose(PipeNetwork::Disposal, None)
            }
        }
        if network == PipeNetwork::Gas {
            div { class: "options-rule" }
            label { class: "option-check",
                input { r#type: "checkbox", class: "check", checked: current.is_some(),
                    oninput: move |event| {
                        let on = event.checked();
                        with(|editor| editor.session.choose_pipe(PipeNetwork::Gas, on.then(|| PIPE_COLOURS[0].1.to_owned())));
                        bump();
                    }
                }
                "Colour"
            }
            if let Some(text) = current.clone() {
                ColourField { value: text, onchange: move |text: String| { with(|editor| editor.session.choose_pipe(PipeNetwork::Gas, Some(text))); bump(); } }
            } else {
                span { class: "hint-text", "the pipe's own" }
            }
        }
        span { class: "hint-text", "Right-drag removes" }
    }
}
