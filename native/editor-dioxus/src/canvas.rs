//! The map canvas: the engine's picture in a custom widget, the mouse on it, and what is drawn on
//! top. Lines and outlines go into the widget's own scene. Text over the map is HTML laid over
//! the widget, since a widget's scene has no easy way to set type.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyrender::{PaintRef, PaintScene, RenderContext, ResourceId};
use blitz_dom::node::ComputedStyles;
use blitz_dom::Widget;
use dioxus_native::prelude::*;
use dioxus_native::winit::cursor::CursorIcon;
use dioxus_native::{CustomWidgetAttr, DeviceHandle};
use editor_core::air::{Icon, Legend, LegendRow, Sample, CIVIDIS};
use editor_core::air_draw::AirLabel;
use editor_core::renderer::{MapRenderer, Surround};
use editor_core::session::{Button, LinkKind, OutlineStyle, Pickable, PointerLook};
use editor_core::shell::colours;
use kurbo::{Affine, BezPath, Line, Point, Rect, Shape, Stroke, Vec2};
use peniko::{Color, Fill, ImageBrush, ImageSampler};

use crate::{bump, bump_map, redraw, wake, watch, watch_map, with};

/// Whether the map moves by itself: a pulsing border, moving dashes or a drifting backdrop.
/// The timer draws it again thirty times a second while it does.
static ANIMATING: AtomicBool = AtomicBool::new(false);
/// Whether the performance display is open. The timer refreshes it four times a second.
pub(crate) static PERFORMANCE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Where the mouse last moved to over the map with no button down, until the session is
    /// told. The session is told once a frame, and not for every move.
    static PENDING_MOVE: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
    /// The hover label as last worked out, and where the cursor was then. The frame draws its
    /// outline from it while the cursor and the engine's answers are as they were, since it
    /// takes some milliseconds on a station.
    static HOVER_LABEL: std::cell::Cell<Option<(Option<(f32, f32)>, Option<([(f32, f32); 4], String)>)>> = const { std::cell::Cell::new(None) };
    /// The air legend's sample tiles, each painted once (`air_draw::sample`).
    static SAMPLES: std::cell::RefCell<std::collections::HashMap<Sample, String>> = std::cell::RefCell::new(std::collections::HashMap::new());
    /// The air overlay's icons in their circles, each painted once (`air_draw::icon_picture`).
    static ICONS: std::cell::RefCell<std::collections::HashMap<(Icon, [u8; 3]), String>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The picture of an icon in its circle, twice the size it is shown at.
fn icon_uri(icon: Icon, colour: [u8; 3]) -> String {
    const SIZE: u32 = 28;
    ICONS.with(|icons| icons.borrow_mut().entry((icon, colour)).or_insert_with(|| crate::thumbs::picture_uri(&editor_core::air_draw::icon_picture(icon, colour, SIZE), SIZE, SIZE)).clone())
}

/// How wide a tile of the map is on screen, in CSS pixels, for the air overlay's marks and labels.
fn tile_pixels(zoom: f32, scale: f32) -> f32 {
    32.0 * zoom / scale.max(0.01)
}

/// The mouse moved over the map with no button down. Finding what is under it costs some
/// milliseconds on a station, and the mouse can move a thousand times a second. Done for
/// every move, the moves queued up and the window was not drawn again until the mouse
/// stopped. So the move waits for the next frame (`MapWidget::paint`), which tells the session
/// and has the labels over the map built again, once a frame.
fn hover_moved(point: (f32, f32)) {
    PENDING_MOVE.set(Some(point));
    redraw();
}

/// Tells the session where the mouse moved to, if it has moved since it was last told. Says
/// whether it had.
fn take_move(editor: &mut crate::Editor) -> bool {
    let Some(point) = PENDING_MOVE.take() else { return false };
    let view = editor.view;
    editor.session.pointer_move(point, view);
    true
}

/// Forgets the hover label, after a change that may change it with the cursor where it was.
pub(crate) fn forget_hover_label() {
    HOVER_LABEL.set(None);
}

/// The session's hover label, worked out again only when the cursor has moved, the engine has
/// sent something, or anything else has changed (`bump`) since it last was (`HOVER_LABEL`).
fn hover_label(session: &mut editor_core::session::Session) -> Option<([(f32, f32); 4], String)> {
    let label = match HOVER_LABEL.take() {
        Some((at, label)) if at == session.hover => label,
        _ => session.hover_label(),
    };
    HOVER_LABEL.set(Some((session.hover, label.clone())));
    label
}

/// How long a dash of the region's outline is, and the gap after it, in CSS pixels.
const DASH: f64 = 6.0;

/// The lines Windows scrolls for a notch of the wheel, unless the user changed it.
const LINES_PER_NOTCH: f32 = 3.0;

/// Starts the thread that draws again what moves by itself.
pub(crate) fn start_timer() {
    let _ = std::thread::Builder::new().name("frame timer".into()).spawn(|| {
        let mut count = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            count += 1;
            if ANIMATING.load(Ordering::Relaxed) || crate::loading::MOVING.load(Ordering::Relaxed) {
                redraw();
            }
            if count % 8 == 0 && PERFORMANCE.load(Ordering::Relaxed) {
                wake();
            }
        }
    });
}

fn rgba([red, green, blue, alpha]: [u8; 4]) -> Color {
    Color::from_rgba8(red, green, blue, alpha)
}

fn rgb((red, green, blue): (u8, u8, u8)) -> Color {
    Color::from_rgb8(red, green, blue)
}

/// The map's point under the mouse, in device pixels, from where the event was in the window.
fn map_point(event: &MouseEvent) -> (f32, f32) {
    let client = event.client_coordinates();
    with(|editor| {
        let (x, y) = (client.x as f32 - editor.origin.0, client.y as f32 - editor.origin.1);
        (x * editor.scale, y * editor.scale)
    })
}

/// Notes where the map is in the window, from an event whose target is the map.
fn learn_origin(event: &MouseEvent) {
    let (client, element) = (event.client_coordinates(), event.element_coordinates());
    with(|editor| editor.origin = ((client.x - element.x) as f32, (client.y - element.y) as f32));
}

fn button_of(event: &MouseEvent) -> Option<Button> {
    match event.trigger_button()? {
        dioxus_html::input_data::MouseButton::Primary => Some(Button::Primary),
        dioxus_html::input_data::MouseButton::Auxiliary => Some(Button::Middle),
        dioxus_html::input_data::MouseButton::Secondary => Some(Button::Secondary),
        _ => None,
    }
}

fn held(event: &MouseEvent, button: Button) -> bool {
    use dioxus_html::input_data::MouseButton;
    let buttons = event.held_buttons();
    match button {
        Button::Primary => buttons.contains(MouseButton::Primary),
        Button::Middle => buttons.contains(MouseButton::Auxiliary),
        Button::Secondary => buttons.contains(MouseButton::Secondary),
    }
}

/// The mouse moving anywhere in the window while a press that began on the map lasts. The
/// press goes on off the map.
pub(crate) fn drag_move(event: &MouseEvent) {
    let Some(button) = with(|editor| editor.pressing) else { return };
    let point = map_point(event);
    let released = with(|editor| {
        let view = editor.view;
        if editor.last_pointer != Some(point) {
            editor.session.pointer_move(point, view);
            editor.last_pointer = Some(point);
        }
        // A release that was not seen, as off the window.
        let released = !held(event, button);
        if released {
            editor.session.pointer_up(point, view);
            editor.pressing = None;
        }
        released
    });
    // A move changes what the map shows. What it changes beside, the engine's answer brings.
    if released {
        bump();
    } else {
        bump_map();
    }
}

/// A button coming up anywhere in the window.
pub(crate) fn drag_up(event: &MouseEvent) {
    let Some(button) = with(|editor| editor.pressing) else { return };
    if button_of(event) != Some(button) {
        return;
    }
    let point = map_point(event);
    with(|editor| {
        let view = editor.view;
        editor.session.pointer_up(point, view);
        editor.pressing = None;
    });
    bump();
}

/// What the canvas shows over the map as HTML, worked out from the session.
struct Labels {
    /// Each a label of white on black, centred over a point, in CSS pixels.
    boxes: Vec<(f32, f32, String)>,
    /// The ghost's text: lines centred under a point, in its colour.
    ghost: Option<(f32, f32, Vec<String>, String)>,
    /// The list the wheel steps through: where, its rows (mark and id) and the current row.
    list: Option<(f32, f32, Vec<(Option<&'static str>, String)>, usize)>,
    /// The region's menu: where, and its entries.
    menu: Option<(f32, f32, Vec<editor_core::session::MenuEntry>)>,
    /// The air overlay's box by the mouse: where its top left corner goes, its icon's picture
    /// and its lines.
    air_box: Option<(f32, f32, Option<String>, Vec<String>)>,
    /// The air overlay's legend, while the overlay is on.
    legend: Option<Legend>,
    /// The air overlay's labels on rooms, each centred on its point, in CSS pixels.
    air_labels: Vec<(f32, f32, AirLabel)>,
    performance: Option<String>,
    cursor: &'static str,
}

fn labels() -> Labels {
    with(|editor| {
        let (view, scale) = (editor.view, editor.scale);
        let session = &mut editor.session;
        let camera = session.camera;
        let css = |world: (f32, f32)| {
            let at = camera.view_at(world, view);
            (at.0 / scale, at.1 / scale)
        };
        let mut boxes = Vec::new();
        if let Some((cell, text)) = hover_label(session) {
            let points: Vec<(f32, f32)> = cell.iter().map(|&corner| css(corner)).collect();
            let top = points.iter().map(|point| point.1).fold(f32::MAX, f32::min);
            let middle = points.iter().map(|point| point.0).sum::<f32>() / 4.0;
            boxes.push((middle, top - 8.0, text));
        }
        for (at, text) in [session.drag_label(), session.region_label()].into_iter().flatten() {
            let at = css(at);
            boxes.push((at.0, at.1 - 8.0, text));
        }
        let ghost = session.place_preview(scale).map(|preview| {
            let middle = css(preview.at);
            let below = css(preview.below);
            let [red, green, blue, alpha] = preview.colour;
            let lines = [preview.label, preview.name].into_iter().flatten().collect();
            (middle.0, below.1 + 2.0, lines, format!("rgba({red}, {green}, {blue}, {})", alpha as f32 / 255.0))
        });
        let list = match (session.stack(), session.pick_list()) {
            (Some(stack), _) => Some((stack.cell, stack.entries.into_iter().map(|id| (None, id)).collect::<Vec<_>>(), stack.current)),
            (None, Some(pick)) => {
                let rows = pick
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        Pickable::Entity(id) => (Some("E"), id.clone()),
                        Pickable::Decal(id, _) => (Some("D"), id.clone()),
                        Pickable::Tile(id) => (Some("T"), id.clone()),
                    })
                    .collect();
                Some((pick.cell, rows, pick.current))
            }
            (None, None) => None,
        };
        // Four pixels to the right of its cell, from the cell's top down.
        let list = list.map(|(cell, rows, current)| {
            let corners = cell.map(css);
            let right = corners.iter().map(|at| at.0).fold(f32::MIN, f32::max);
            let top = corners.iter().map(|at| at.1).fold(f32::MAX, f32::min);
            (right + 4.0, top, rows, current)
        });
        let menu = session.region_menu().map(|(at, entries)| {
            let at = css(at);
            (at.0, at.1, entries)
        });
        // Below and to the right of the cursor, or to its left near the map's right edge. Over a
        // Problems badge, what is wrong there, before the air.
        let problem = session.problem_hover();
        let icon = match &problem {
            Some(_) => Some(icon_uri(Icon::Bang, editor_core::air::LEAK)),
            None => session.air_hover_icon().map(|(icon, colour)| icon_uri(icon, colour)),
        };
        let air_box = problem.or_else(|| session.air_hover()).zip(session.hover).map(|(lines, world)| {
            let at = css(world);
            let x = if at.0 + 16.0 + AIR_BOX_WIDTH > view.0 as f32 / scale { at.0 - 16.0 - AIR_BOX_WIDTH } else { at.0 + 16.0 };
            (x, at.1 + 16.0, icon, lines)
        });
        let legend = session.air_legend();
        // Only those on the map in view, and a label's width beyond, and none covering another.
        let (width, height) = (view.0 as f32 / scale, view.1 as f32 / scale);
        let placed = session.air_labels(tile_pixels(camera.zoom, scale)).into_iter().map(|label| (css(label.at), label)).filter(|((x, y), _)| (-200.0..width + 200.0).contains(x) && (-20.0..height + 20.0).contains(y)).collect();
        let air_labels = editor_core::air_draw::uncrowded(placed).into_iter().map(|((x, y), label)| (x, y, label)).collect();
        let performance = editor.show_performance.then(|| performance_text(editor));
        let look = editor.session.pointer_look();
        let (cursor, icon) = match look {
            PointerLook::Default => ("default", CursorIcon::Default),
            PointerLook::Crosshair => ("crosshair", CursorIcon::Crosshair),
            PointerLook::Grab => ("grab", CursorIcon::Grab),
            PointerLook::Grabbing => ("grabbing", CursorIcon::Grabbing),
        };
        // Blitz sets the pointer from the style only when the mouse goes over another element,
        // so a change of tool over the map is given to the window directly.
        let over = editor.session.hover.is_some() || editor.pressing.is_some();
        if over && editor.cursor != Some(look) {
            if let Some(window) = crate::WINDOW.get() {
                window.set_cursor(icon.into());
            }
        }
        editor.cursor = over.then_some(look);
        Labels { boxes, ghost, list, menu, air_box, legend, air_labels, performance, cursor }
    })
}

/// The performance display's text: the session's numbers with the frame time of this
/// interface, measured around Blitz's redraw (`run.rs`).
fn performance_text(editor: &mut crate::Editor) -> String {
    let (cost, rate) = crate::run::FRAME_TIMES.with(|times| times.borrow().summary());
    let mut lines = editor.session.performance().lines(cost).to_vec();
    if let Some(rate) = rate {
        lines.push(format!("rate       {rate:.0} frames a second while drawing"));
    }
    lines.push(format!("held       {}", editor.session.held.words()));
    for line in [&editor.session.last_put, &editor.session.last_put_answer].into_iter().flatten() {
        lines.push(format!("put        {line}"));
    }
    if let Some(breakdown) = &editor.session.last_breakdown {
        lines.push(breakdown.clone());
    }
    lines.join("\n")
}

#[component]
pub(crate) fn Canvas() -> Element {
    let _ = (watch(), watch_map());
    let map = use_hook(|| CustomWidgetAttr::new(MapWidget::default()));
    let labels = labels();
    PERFORMANCE.store(labels.performance.is_some(), Ordering::Relaxed);

    rsx! {
        div {
            class: "map-surface",
            style: "cursor: {labels.cursor};",
            onmousedown: move |event| {
                event.stop_propagation();
                learn_origin(&event);
                let Some(button) = button_of(&event) else { return };
                let point = map_point(&event);
                let modifiers = event.modifiers();
                with(|editor| {
                    editor.menu = None;
                    if editor.pressing.is_some() {
                        // The secondary button during a drag drops a room being dragged out.
                        // The press goes on until its own button comes up.
                        if button == Button::Secondary {
                            editor.session.cancel_room();
                        }
                        return;
                    }
                    take_move(editor);
                    editor.shift = modifiers.shift();
                    editor.ctrl = modifiers.ctrl();
                    editor.hand_on_held();
                    let view = editor.view;
                    editor.session.pointer_down(point, view, button);
                    editor.pressing = Some(button);
                    editor.last_pointer = Some(point);
                });
                bump();
            },
            onmousemove: move |event| {
                learn_origin(&event);
                if with(|editor| editor.pressing.is_some()) {
                    // The window's handler has it.
                    return;
                }
                let point = map_point(&event);
                let moved = with(|editor| {
                    if editor.last_pointer == Some(point) {
                        return false;
                    }
                    editor.last_pointer = Some(point);
                    true
                });
                if moved {
                    hover_moved(point);
                }
            },
            onmouseleave: move |_| {
                PENDING_MOVE.set(None);
                let left = with(|editor| {
                    if editor.pressing.is_some() || editor.session.hover.is_none() {
                        return false;
                    }
                    editor.session.pointer_left();
                    editor.last_pointer = None;
                    true
                });
                if left {
                    bump_map();
                }
            },
            onwheel: move |event| {
                // Winit's lines are up for away from the user, which zooms in. On Windows it
                // gives the system's lines for a notch, three unless the user changed it. The
                // session takes the notch whole, and gives a
                // zoom over the frames that follow (`MapWidget::paint`).
                let notches = match event.delta() {
                    dioxus_html::geometry::WheelDelta::Lines(lines) => lines.y as f32 / LINES_PER_NOTCH,
                    dioxus_html::geometry::WheelDelta::Pixels(pixels) => pixels.y as f32 / 40.0,
                    dioxus_html::geometry::WheelDelta::Pages(pages) => pages.y as f32 * 4.0,
                };
                let client = event.client_coordinates();
                with(|editor| {
                    if editor.pressing.is_some() || notches == 0.0 {
                        return;
                    }
                    take_move(editor);
                    let point = ((client.x as f32 - editor.origin.0) * editor.scale, (client.y as f32 - editor.origin.1) * editor.scale);
                    let view = editor.view;
                    editor.session.wheel(notches, point, view);
                });
                bump();
            },
            object { "data": map, class: "map-widget" }
            // The air overlay's labels on rooms, under all other text.
            for (index, (x, y, label)) in labels.air_labels.into_iter().enumerate() {
                div { key: "air{index}", class: "air-label", style: "left: {x}px; top: {y}px; border-color: {css_colour(label.colour)};",
                    if let Some(icon) = label.icon {
                        img { class: "air-label-icon", src: "{icon_uri(icon, label.colour)}" }
                    }
                    span { class: "air-label-text", "{label.text}" }
                }
            }
            // Text over the map, which the mouse goes through.
            for (index, (x, y, text)) in labels.boxes.into_iter().enumerate() {
                div { key: "{index}", class: "map-label", style: "left: {x}px; top: {y}px;", "{text}" }
            }
            if let Some((x, y, lines, colour)) = labels.ghost {
                div { class: "map-ghost", style: "left: {x}px; top: {y}px; color: {colour};",
                    for line in lines {
                        div { "{line}" }
                    }
                }
            }
            if let Some((x, y, rows, current)) = labels.list {
                WheelList { x, y, rows, current }
            }
            if let Some(legend) = labels.legend {
                AirLegend { legend }
            }
            if let Some((x, y, icon, lines)) = labels.air_box {
                div { class: "air-hover", style: "left: {x}px; top: {y}px; width: {AIR_BOX_WIDTH}px;",
                    for (index, line) in lines.into_iter().enumerate() {
                        if index == 0 {
                            div { key: "{index}", class: "air-hover-title",
                                if let Some(icon) = icon.clone() {
                                    img { class: "air-hover-icon", src: "{icon}" }
                                }
                                span { "{line}" }
                            }
                        } else {
                            div { key: "{index}", "{line}" }
                        }
                    }
                }
            }
        }
        if let Some((x, y, entries)) = labels.menu {
            RegionMenu { x, y, entries }
        }
        if let Some(text) = labels.performance {
            div { class: "performance", "{text}" }
        }
    }
}

/// How wide the air overlay's box by the mouse is, in CSS pixels.
const AIR_BOX_WIDTH: f32 = 190.0;

fn css_colour([red, green, blue]: [u8; 3]) -> String {
    format!("rgb({red}, {green}, {blue})")
}

/// The picture of a legend's sample tile, drawn by the same code as the map, twice the size it
/// is shown at so that it is sharp on a screen of two pixels a point.
fn sample_uri(sample: Sample) -> String {
    const SIZE: u32 = 40;
    SAMPLES.with(|samples| samples.borrow_mut().entry(sample).or_insert_with(|| crate::thumbs::picture_uri(&editor_core::air_draw::sample(sample, SIZE), SIZE, SIZE)).clone())
}

/// The air overlay's legend over the map's bottom left corner: each kind of the view with a
/// sample tile, so that it is matched by its mark as well as its colour, or the view's scale
/// with its ticks and the safe band boxed, and the kinds of tile with no air.
#[component]
fn AirLegend(legend: Legend) -> Element {
    let rows: Vec<(String, LegendRow)> = legend.rows.iter().chain(&legend.special).map(|row| (sample_uri(row.sample), row.clone())).collect();
    let scale = legend.scale.map(|scale| {
        let last = scale.stops.len().saturating_sub(1).max(1) as f32;
        let segments: Vec<String> = CIVIDIS.windows(2).map(|pair| format!("background: linear-gradient(to right, {}, {});", css_colour(pair[0]), css_colour(pair[1]))).collect();
        let ticks: Vec<(f32, &'static str)> = scale.stops.iter().enumerate().map(|(index, stop)| (index as f32 / last * 100.0, stop.1)).collect();
        let (from, to) = (scale.place(scale.safe.0) * 100.0, scale.place(scale.safe.1) * 100.0);
        (segments, ticks, format!("left: {from}%; width: {}%;", to - from))
    });
    rsx! {
        div { class: "air-legend",
            div { class: "air-legend-title", "{legend.title}" }
            if let Some((segments, ticks, safe)) = scale {
                div { class: "air-scale-box",
                    div { class: "air-scale",
                        for (index, style) in segments.into_iter().enumerate() {
                            div { key: "{index}", class: "air-scale-part", style: "{style}" }
                        }
                    }
                    div { class: "air-safe", style: "{safe}" }
                }
                div { class: "air-ticks",
                    for (index, (at, text)) in ticks.into_iter().enumerate() {
                        div { key: "{index}", class: "air-tick", style: "left: {at}%;", "{text}" }
                    }
                }
            }
            if let Some(note) = legend.note {
                div { class: "air-legend-note", "{note}" }
            }
            for (index, (uri, row)) in rows.into_iter().enumerate() {
                div { key: "{index}", class: "air-legend-row",
                    img { class: "air-swatch", src: "{uri}" }
                    div {
                        div { class: "air-legend-label", "{row.label}" }
                        div { class: "air-legend-meaning", "{row.meaning}" }
                    }
                }
            }
        }
    }
}

/// The list the wheel steps through, as the web editor draws it: a row for each thing, the
/// current one filled. The pick tool's rows are marked E for an entity, D for a decal and T for
/// a tile.
#[component]
fn WheelList(x: f32, y: f32, rows: Vec<(Option<&'static str>, String)>, current: usize) -> Element {
    let count = rows.len();
    rsx! {
        div { class: "wheel-list", style: "left: {x}px; top: {y}px;",
            for (index, (mark, id)) in rows.into_iter().enumerate() {
                div { key: "{index}", class: if index == current { "wheel-row current" } else { "wheel-row" },
                    span { class: "wheel-count", "{index + 1}/{count}" }
                    if let Some(mark) = mark {
                        span { class: "wheel-mark", "{mark}" }
                    }
                    span { class: "wheel-id", "{id}" }
                }
            }
        }
    }
}

/// The area tool's right-click menu on a region. The session holds its state and does what
/// is chosen. A press anywhere off it closes it (`main.rs`).
#[component]
fn RegionMenu(x: f32, y: f32, entries: Vec<editor_core::session::MenuEntry>) -> Element {
    use editor_core::session::MenuAction;
    rsx! {
        div { class: "absolute min-w-[160px] bg-elevated border border-subtle rounded-sm shadow-lg z-50 py-1",
            style: "left: {x}px; top: {y}px;",
            onmousedown: move |event| event.stop_propagation(),
            for entry in entries {
                if entry.action == MenuAction::SaveAsPrefab {
                    div { class: "h-px bg-subtle mx-2 my-1" }
                }
                button {
                    disabled: !entry.enabled,
                    class: if entry.enabled { "flex w-full px-3 py-1.5 text-xs text-left items-center gap-2 border-none bg-transparent text-primary cursor-pointer hover:bg-hover" } else { "flex w-full px-3 py-1.5 text-xs text-left items-center gap-2 border-none bg-transparent text-muted cursor-default hover:bg-transparent" },
                    onclick: move |_| {
                        if entry.enabled {
                            with(|editor| editor.session.choose_from_region_menu(entry.action));
                            bump();
                        }
                    },
                    span { class: "flex-1", "{entry.label}" }
                    span { class: "text-muted ml-4 text-[10px]", "{entry.shortcut}" }
                }
            }
        }
    }
}

/// The map, as a widget Blitz lays out and paints like an image.
#[derive(Default)]
struct MapWidget {
    gpu: Option<Gpu>,
    /// When the map was last painted, for the time a frame of the wheel's zoom takes.
    painted: Option<Instant>,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: MapRenderer,
    /// Two textures taken in turn, so the one on screen is never the one being drawn.
    shown: Option<(wgpu::Texture, ResourceId)>,
    next: Option<(wgpu::Texture, ResourceId)>,
}

impl Widget for MapWidget {
    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        let Some(handle) = render_ctx.renderer_specific_context().and_then(|context| context.downcast::<DeviceHandle>().ok()) else {
            eprintln!("the interface is not drawn with wgpu, so there is no map");
            return;
        };
        let renderer = MapRenderer::new(&handle.device, wgpu::TextureFormat::Rgba8Unorm);
        self.gpu = Some(Gpu { device: handle.device.clone(), queue: handle.queue.clone(), renderer, shown: None, next: None });
    }

    fn destroy_surfaces(&mut self) {
        self.gpu = None;
    }

    fn paint(&mut self, render_ctx: &mut dyn RenderContext, _styles: &ComputedStyles, width: u32, height: u32, scale: f64) -> anyrender::Scene {
        let mut scene = anyrender::Scene::new();
        let Some(gpu) = self.gpu.as_mut() else { return scene };
        if width == 0 || height == 0 {
            return scene;
        }

        if gpu.next.as_ref().is_some_and(|(texture, _)| texture.width() != width || texture.height() != height) {
            render_ctx.unregister_resource(gpu.next.take().unwrap().1);
        }
        if gpu.next.is_none() {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("map"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let Ok(resource) = render_ctx.try_register_custom_resource(Box::new(texture.clone())) else { return scene };
            gpu.next = Some((texture, resource));
        }
        let (texture, resource) = gpu.next.clone().unwrap();
        let view = (width, height);
        let now = Instant::now();
        let seconds = self.painted.replace(now).map_or(0.0, |then| (now - then).as_secs_f32());

        let (changed, zoomed, moved) = with(|editor| {
            let resized = editor.view != view;
            (editor.view, editor.scale) = (view, scale as f32);
            let changed = editor.session.pump(view) || resized;
            if changed {
                // What the engine sent may change what is under the cursor.
                forget_hover_label();
            }
            // The last move of the mouse since the last frame, so that the outline under it is
            // drawn where it is now.
            let moved = take_move(editor);
            let session = &mut editor.session;
            // The frame gives its part of the zoom the wheel owes (`Session::advance_zoom`).
            let zoomed = session.zooming();
            session.advance_zoom(seconds);

            // The engine's picture, with the editor's own rectangles in it.
            if let Some(document) = session.document {
                let (under, above) = session.overlay_quads();
                if let Some(backdrop) = session.take_backdrop() {
                    gpu.renderer.set_backdrop(&gpu.device, &gpu.queue, &backdrop);
                }
                // The Problems overlay's badges go over everything else on the map.
                let mut above = above;
                above.extend_from_slice(session.problem_overlay(tile_pixels(session.camera.zoom, scale as f32)).1);
                gpu.renderer.set_overlays(&under, &above);
                let (air_version, air) = session.air_overlay(tile_pixels(session.camera.zoom, scale as f32));
                gpu.renderer.set_air(air_version, air);
                gpu.renderer.set_selected(&session.selection_quads());
                gpu.renderer.prepare(&gpu.device, &gpu.queue, &mut session.lists, document, &editor.textures);
                // The loading screen's last stage, once the map it waits for is ready to draw.
                session.map_drawn();
                gpu.renderer.set_view(&gpu.queue, session.camera, width, height, session.layers.hidden_mask());
                let seconds = session.started.elapsed().as_secs_f64();
                let surround = Surround { backdrop: session.show_backdrop, seconds, grid: session.show_grid.then(|| session.grid_lines()) };
                gpu.renderer.set_surround(&gpu.queue, session.camera, width, height, surround);
                // The web editor's selection border: gold, two sprite pixels wide, following
                // the sprite's own shape, and gently pulsing.
                let pulse = 0.8 + 0.2 * (seconds / 0.3).sin() as f32;
                let border = (2.0 * session.camera.zoom).clamp(1.5, 8.0);
                gpu.renderer.selection_border(&gpu.device, &gpu.queue, width, height, border, [1.0, 215.0 / 255.0, 0.0, pulse]);
            }
            // The backdrop shown may be the one under the mouse in the gallery.
            let drifting = session.show_backdrop && session.backdrop_moves();
            let moving = !session.selection.is_empty() || drifting || session.highlight_live() || session.link_line().is_some() || !session.region_edges().is_empty();
            ANIMATING.store(session.document.is_some() && moving, Ordering::Relaxed);
            (changed, zoomed, moved)
        });

        let target = texture.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let (red, green, blue) = colours::CANVAS;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: red as f64 / 255.0, g: green as f64 / 255.0, b: blue as f64 / 255.0, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if with(|editor| editor.session.document.is_some()) {
                gpu.renderer.draw(&mut pass);
            }
        }
        gpu.queue.submit([encoder.finish()]);

        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            PaintRef::Resource(ImageBrush { image: resource, sampler: ImageSampler::default() }),
            None,
            &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
        );
        with(|editor| draw_over(&mut scene, editor, scale));
        std::mem::swap(&mut gpu.next, &mut gpu.shown);
        // What the engine sent changes the panels too.
        if changed {
            wake();
        }
        // A zoom moves what is laid over the map and the status bar's zoom, and while more is
        // owed the next frame gives the next part. A move this frame took moves them as well.
        if zoomed || moved {
            bump_map();
        }
        scene
    }
}

/// A closed outline through the points.
fn polygon(points: &[(f32, f32)]) -> BezPath {
    let mut path = BezPath::new();
    for (index, &(x, y)) in points.iter().enumerate() {
        let point = Point::new(x as f64, y as f64);
        if index == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}

fn line(from: (f32, f32), to: (f32, f32)) -> Line {
    Line::new((from.0 as f64, from.1 as f64), (to.0 as f64, to.1 as f64))
}

/// The outlines, lines and dashes over the map, in device pixels.
fn draw_over(scene: &mut anyrender::Scene, editor: &mut crate::Editor, scale: f64) {
    let (view, session) = (editor.view, &mut editor.session);
    let camera = session.camera;
    let at = |world: (f32, f32)| camera.view_at(world, view);
    let seconds = session.started.elapsed().as_secs_f64();
    let width = |points: f64| points * scale;

    for outline in session.outlines() {
        let points: Vec<(f32, f32)> = outline.corners.iter().map(|&corner| at(corner)).collect();
        let shape = polygon(&points);
        let (stroke, colour, fill): (f64, Color, Option<Color>) = match outline.style {
            // The cell under the cursor in the tool's own look (`Session::hover_look`).
            OutlineStyle::Hover => {
                let look = session.hover_look();
                let edge = rgba(look.edge);
                if let Some(fill) = look.fill {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, rgba(fill), None, &shape);
                }
                if look.cross {
                    let middle = points.iter().fold((0.0, 0.0), |sum, point| (sum.0 + point.0 / 4.0, sum.1 + point.1 / 4.0));
                    let inset = |point: (f32, f32)| {
                        let (dx, dy) = (middle.0 - point.0, middle.1 - point.1);
                        let length = (dx * dx + dy * dy).sqrt().max(0.001);
                        let step = 4.0 * scale as f32;
                        (point.0 + dx / length * step, point.1 + dy / length * step)
                    };
                    scene.stroke(&Stroke::new(width(2.0)), Affine::IDENTITY, edge, None, &line(inset(points[0]), inset(points[2])));
                    scene.stroke(&Stroke::new(width(2.0)), Affine::IDENTITY, edge, None, &line(inset(points[1]), inset(points[3])));
                }
                if look.dashed {
                    let dashes = [width(4.0), width(4.0)];
                    scene.stroke(&Stroke::new(width(2.0)).with_dashes(0.0, dashes), Affine::IDENTITY, edge, None, &shape);
                    continue;
                }
                (2.0, edge, None)
            }
            OutlineStyle::Drag => (1.5, rgb(colours::ACCENT), Some(Color::from_rgba8(68, 136, 255, 40))),
            // The web editor's green for a box that adds, and its red for one that takes away.
            OutlineStyle::DragAdd => (1.5, Color::from_rgba8(50, 255, 50, 230), Some(Color::from_rgba8(60, 255, 60, 64))),
            OutlineStyle::DragTakeAway => (1.5, Color::from_rgb8(255, 30, 30), Some(Color::from_rgba8(255, 40, 40, 102))),
            // The web editor's gold for the ends of a line, and its cyan for a circle's centre.
            OutlineStyle::Line => (2.0, Color::from_rgb8(255, 204, 0), None),
            OutlineStyle::Circle => (2.0, Color::from_rgb8(0, 204, 255), None),
            // The web editor outlines a selected decal in cyan.
            OutlineStyle::Decal => (2.0, Color::from_rgb8(0, 255, 255), None),
            // The web editor's pulsing red for the cell a finding was jumped to.
            OutlineStyle::Found => {
                let pulse = 0.3 + 0.6 * (0.5 + 0.5 * (seconds * std::f64::consts::PI / 0.3).sin());
                (3.0, Color::from_rgba8(255, 68, 68, (pulse * 255.0) as u8), None)
            }
            // The link tool: a faint green for what could be a source, a pulsing bright green
            // for the source, and cyan for what it is linked to.
            OutlineStyle::Linkable => (1.0, Color::from_rgba8(80, 200, 80, 102), None),
            OutlineStyle::LinkSource => {
                let pulse = 0.6 + 0.4 * (seconds / 0.2).sin();
                (3.0, Color::from_rgba8(80, 255, 80, (pulse * 255.0) as u8), None)
            }
            OutlineStyle::Linked => (2.0, Color::from_rgba8(0, 220, 220, 178), None),
        };
        if let Some(fill) = fill {
            scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &shape);
        }
        scene.stroke(&Stroke::new(width(stroke)), Affine::IDENTITY, colour, None, &shape);
    }

    // The faint outline of the cell whose name is shown above it.
    if let Some((cell, _)) = hover_label(session) {
        let points: Vec<(f32, f32)> = cell.iter().map(|&corner| at(corner)).collect();
        scene.stroke(&Stroke::new(width(2.0)), Affine::IDENTITY, Color::from_rgba8(255, 255, 100, 153), None, &polygon(&points));
    }

    // The link tool's dashed line from its source: cyan for a device list and orange for
    // links, six points on and four off.
    if let Some(link) = session.link_line() {
        let colour = match link.kind {
            LinkKind::List => Color::from_rgb8(0x00, 0xcc, 0xcc),
            LinkKind::Links => Color::from_rgb8(0xff, 0x88, 0x00),
        };
        let stroke = Stroke::new(width(2.0)).with_dashes(0.0, [width(6.0), width(4.0)]);
        scene.stroke(&stroke, Affine::IDENTITY, colour, None, &line(at(link.from), at(link.to)));
    }

    // The area tool's region: white and black dashes that move, twenty pixels a second, in
    // step along each line so that it reads as one.
    let edges = session.region_edges();
    if !edges.is_empty() {
        let dash = width(DASH);
        let moved = (seconds * 20.0 * scale).rem_euclid(dash * 2.0);
        let seen = Rect::new(-dash, -dash, view.0 as f64 + dash, view.1 as f64 + dash);
        for [from, to] in edges {
            let (from, to) = (at(from), at(to));
            let segment = line(from, to);
            let length = segment.length();
            let bounds = segment.bounding_box();
            if length <= 0.0 || bounds.x1 < seen.x0 || bounds.x0 > seen.x1 || bounds.y1 < seen.y0 || bounds.y0 > seen.y1 {
                continue;
            }
            let along = Vec2::new((to.0 - from.0) as f64, (to.1 - from.1) as f64) / length;
            let begins = Vec2::new(from.0 as f64, from.1 as f64).dot(along) - moved;
            let white = Stroke::new(width(1.5)).with_dashes(begins.rem_euclid(dash * 2.0), [dash, dash]);
            let black = Stroke::new(width(1.5)).with_dashes((begins + dash).rem_euclid(dash * 2.0), [dash, dash]);
            scene.stroke(&white, Affine::IDENTITY, Color::WHITE, None, &segment);
            scene.stroke(&black, Affine::IDENTITY, Color::BLACK, None, &segment);
        }
    }

    // What a press would put down: a dashed square while the engine has yet to send the
    // picture, and once it is turned a short arrow from its middle the way it will face.
    if let Some(preview) = session.place_preview(scale as f32) {
        let [red, green, blue, alpha] = preview.colour;
        if let Some(square) = preview.square {
            let points: Vec<(f32, f32)> = square.iter().map(|&corner| at(corner)).collect();
            // The web editor's square is a little fainter than its label.
            let stroke = Stroke::new(width(2.0)).with_dashes(0.0, [width(4.0), width(4.0)]);
            scene.stroke(&stroke, Affine::IDENTITY, Color::from_rgba8(red, green, blue, 153), None, &polygon(&points));
        }
        if let Some((x, y)) = preview.facing {
            let middle = at(preview.at);
            let tile = 32.0 * camera.zoom;
            // Up the map is up the screen, where y grows downwards.
            let length = (tile * 0.4).clamp(8.0 * scale as f32, 24.0 * scale as f32);
            let tip = (middle.0 + x * length, middle.1 - y * length);
            let ink = Color::from_rgba8(red, green, blue, alpha);
            let stroke = Stroke::new(width(2.0));
            scene.stroke(&stroke, Affine::IDENTITY, ink, None, &line(middle, tip));
            // The head: a quarter of the length, thirty degrees each side.
            let back = (-x * length * 0.25, y * length * 0.25);
            for turn in [std::f32::consts::FRAC_PI_6, -std::f32::consts::FRAC_PI_6] {
                let (sin, cos) = turn.sin_cos();
                let side = (back.0 * cos - back.1 * sin, back.0 * sin + back.1 * cos);
                scene.stroke(&stroke, Affine::IDENTITY, ink, None, &line(tip, (tip.0 + side.0, tip.1 + side.1)));
            }
        }
    }
}
