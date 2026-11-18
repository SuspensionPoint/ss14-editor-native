//! The loading screen shown over the editor while the engine starts and a map opens. Each stage
//! that finishes drops a floor tile into its slot, and the clown hops onto it. editor-core's
//! loading model decides where everything is (`editor_core::loading`). This file draws it every
//! frame, with the row of tiles and the clown in a widget of their own and the text as HTML
//! around it.

use std::cell::{Cell, RefCell};
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyrender::{Paint, PaintScene, RenderContext};
use blitz_dom::node::ComputedStyles;
use blitz_dom::Widget;
use dioxus_native::prelude::*;
use dioxus_native::CustomWidgetAttr;
use editor_core::loading::{Frame, AREA_HEIGHT, CLOWN_WIDTH, TILE};
use editor_core::textures::decode_premultiplied_from;
use kurbo::{Affine, Rect, Stroke};
use peniko::{Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality, ImageSampler};

use crate::{bump, redraw, watch, with};

/// Whether the screen moves, so that the frame timer draws it again.
pub(crate) static MOVING: AtomicBool = AtomicBool::new(false);

/// The web editor's clown, as its loading screen had it.
const CLOWN: &[u8] = include_bytes!("../assets/clown.png");
/// Room beside the row for the clown, which waits left of it and slips off its end.
const MARGIN: f32 = 64.0;
/// The height of the menu bar, which a failed load leaves free so that its File menu works.
const MENU_BAR: f32 = 36.0;

thread_local! {
    /// The signal the screen's words read, which the widget bumps when they change.
    static TICK: Cell<Option<Signal<u64>>> = const { Cell::new(None) };
    /// The words and the opacity the screen was last built with.
    static SHOWN: RefCell<Option<(String, f32)>> = const { RefCell::new(None) };
}

/// What the screen's words are at one moment: the number of slots, the screen's opacity, the
/// stage line and its count, the engine's error, and the map a failed open can go back to.
struct Words {
    slots: usize,
    opacity: f32,
    line: String,
    count: Option<String>,
    error: Option<String>,
    back: Option<String>,
}

fn words(editor: &crate::Editor) -> Option<Words> {
    let loading = editor.session.loading.as_ref()?;
    let (line, count) = loading.stage_line();
    let error = loading.failure().map(|(_, error)| error.lines().next().unwrap_or("").to_owned());
    let back = error.as_ref().and(editor.session.doc.as_ref()).map(|doc| doc.source.clone());
    let opacity = loading.frame(Instant::now()).opacity;
    Some(Words { slots: loading.stages().len(), opacity, line, count, error, back })
}

/// A failed load that left a map open goes back to it.
pub(crate) fn dismiss(editor: &mut crate::Editor) -> bool {
    let failed = editor.session.loading.as_ref().is_some_and(|loading| loading.failure().is_some());
    if failed && editor.session.doc.is_some() {
        editor.session.loading_done();
        return true;
    }
    false
}

/// The widget of one showing of the screen. Blitz takes the widget out of its attribute when
/// the `object` holding it is made, and an attribute given to the next `object` would be
/// empty: each showing has a new one, kept while it shows, and the next load makes another.
#[derive(Default)]
struct ScreenWidget(Cell<Option<CustomWidgetAttr>>);

impl ScreenWidget {
    /// The widget of the showing under way, or of a new one.
    fn showing(&self) -> CustomWidgetAttr {
        let widget = self.0.take().unwrap_or_else(|| CustomWidgetAttr::new(LoadingWidget::default()));
        self.0.set(Some(widget.clone()));
        widget
    }

    /// The screen is gone, and its `object` with the widget.
    fn hidden(&self) {
        self.0.set(None);
    }
}

#[component]
pub(crate) fn LoadingScreen() -> Element {
    let _ = watch();
    let tick = use_signal(|| 0u64);
    use_hook(|| TICK.with(|cell| cell.set(Some(tick))));
    let _ = *tick.read();
    let screen = use_hook(|| std::rc::Rc::new(ScreenWidget::default()));
    let Some(words) = with(|editor| words(editor)) else {
        MOVING.store(false, Ordering::Relaxed);
        screen.hidden();
        return rsx! {};
    };
    let widget = screen.showing();
    SHOWN.with(|shown| *shown.borrow_mut() = Some((format!("{}{:?}", words.line, words.count), words.opacity)));

    let Words { slots, opacity, line, count, error, back } = words;
    let failed = error.is_some();
    let width = slots as f32 * TILE + 2.0 * MARGIN;
    // A failed load leaves the menu bar free, and keeps its words where they were.
    let (top, bottom) = if failed { (MENU_BAR, MENU_BAR) } else { (0.0, 0.0) };
    rsx! {
        div {
            style: "position: fixed; left: 0; right: 0; top: {top}px; bottom: 0; padding-bottom: {bottom}px; z-index: 40; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 22px; background-color: rgba(26, 26, 46, {opacity}); font-family: 'Segoe UI', sans-serif; user-select: none;",
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| {
                if with(dismiss) {
                    bump();
                }
            },
            div { style: "font-size: 26px; font-weight: 600; letter-spacing: 0.3px; color: rgba(68, 136, 255, {opacity});", "SS14 Map Editor" }
            object { "data": widget, style: "width: {width}px; height: {AREA_HEIGHT}px;" }
            div { style: "font-size: 14px; min-height: 20px; color: rgba(224, 224, 224, {opacity});",
                "{line}"
                if let Some(count) = count {
                    span { style: "color: rgba(138, 138, 168, {opacity});", " {count}" }
                }
            }
            div { style: "font-size: 13px; min-height: 18px; max-width: 80%; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 6px;",
                if let Some(error) = error {
                    div { style: "color: #ff6b6b;", "{error}" }
                }
                if let Some(back) = back {
                    div { style: "color: #8a8aa8;", "Click or press Esc to go back to {back}." }
                }
            }
        }
    }
}

/// The pictures the screen draws: the clown, its shadow, and the fork's steel floor tile.
struct Pictures {
    clown: Option<ImageData>,
    shadow: Option<ImageData>,
    steel: Option<ImageData>,
}

fn image((width, height, pixels): (u32, u32, Vec<u8>)) -> ImageData {
    ImageData { data: Blob::from(pixels), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::AlphaPremultiplied, width, height }
}

impl Pictures {
    fn load(fork_repo: &str) -> Pictures {
        let clown = decode_premultiplied_from(Cursor::new(CLOWN));
        // The mockup's drop shadow: the clown's shape in black at a third.
        let shadow = clown.as_ref().map(|(width, height, pixels)| {
            let pixels = pixels.chunks_exact(4).flat_map(|pixel| [0, 0, 0, (pixel[3] as f32 * 0.35) as u8]).collect();
            image((*width, *height, pixels))
        });
        let steel = editor_core::loading::steel_tile_picture(Path::new(fork_repo));
        let steel = steel.and_then(|steel| std::fs::File::open(steel).ok()).and_then(|file| decode_premultiplied_from(BufReader::new(file)));
        Pictures { clown: clown.map(image), shadow, steel: steel.map(image) }
    }
}

/// The row of slots and the clown, as a widget Blitz lays out like an image.
#[derive(Default)]
struct LoadingWidget {
    pictures: Option<Pictures>,
}

/// Draws a picture, or the part of it in `source`, with `place` taking the picture's pixels
/// to the widget's.
fn draw(scene: &mut anyrender::Scene, picture: &ImageData, source: Rect, place: Affine, alpha: f32) {
    let sampler = ImageSampler::default().with_quality(ImageQuality::Low).with_alpha(alpha.clamp(0.0, 1.0));
    scene.fill(Fill::NonZero, place, Paint::Image(ImageBrush { image: picture, sampler }), None, &source);
}

impl Widget for LoadingWidget {
    fn paint(&mut self, _render_ctx: &mut dyn RenderContext, _styles: &ComputedStyles, width: u32, height: u32, scale: f64) -> anyrender::Scene {
        let mut scene = anyrender::Scene::new();
        let now = Instant::now();
        let Some((frame, words)) = with(|editor| {
            let loading = editor.session.loading.as_ref()?;
            let (line, count) = loading.stage_line();
            Some((loading.frame(now), format!("{line}{count:?}")))
        }) else {
            MOVING.store(false, Ordering::Relaxed);
            return scene;
        };
        MOVING.store(frame.moving, Ordering::Relaxed);
        if frame.gone {
            // Faded into the editor: the screen goes, and the editor is built without it.
            with(|editor| editor.session.loading_done());
            bump();
            return scene;
        }
        // The words follow the stages, and the fade every frame.
        let stale = SHOWN.with(|shown| shown.borrow().as_ref().is_none_or(|(shown, opacity)| *shown != words || *opacity != frame.opacity));
        if stale {
            if let Some(mut tick) = TICK.with(Cell::get) {
                *tick.write() += 1;
            }
            redraw();
        }

        let pictures = self.pictures.get_or_insert_with(|| with(|editor| Pictures::load(&editor.session.fork_repo)));
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &Rect::new(0.0, 0.0, width as f64, height as f64));
        paint_frame(&mut scene, pictures, &frame, scale);
        scene.pop_layer();
        scene
    }
}

/// The slots, the tiles in them and the clown, in device pixels.
fn paint_frame(scene: &mut anyrender::Scene, pictures: &Pictures, frame: &Frame, scale: f64) {
    let screen = frame.opacity;
    let css = Affine::scale(scale) * Affine::translate((MARGIN as f64, 0.0));
    let slot_top = (AREA_HEIGHT - TILE) as f64;
    let tile = TILE as f64;

    for (index, laid) in frame.tiles.iter().enumerate() {
        let left = index as f64 * tile;
        // The slot: a faint dashed outline just inside it.
        let outline = Rect::new(left + 0.5, slot_top + 0.5, left + tile - 0.5, slot_top + tile - 0.5);
        let dashed = Stroke::new(1.0).with_dashes(0.0, [3.0, 3.0]);
        scene.stroke(&dashed, css, Color::from_rgba8(68, 136, 255, (0.25 * screen * 255.0) as u8), None, &outline);

        let Some(pose) = laid else { continue };
        let top = slot_top - pose.lift as f64;
        let alpha = pose.opacity * screen;
        match &pictures.steel {
            // Each of the four variants in turn, drawn pixelated.
            Some(steel) => {
                let size = steel.height as f64;
                let variant = (index % 4) as f64 * size;
                let place = css * Affine::translate((left, top)) * Affine::scale(tile / size) * Affine::translate((-variant, 0.0));
                draw(scene, steel, Rect::new(variant, 0.0, variant + size, size), place, alpha);
            }
            None => {
                let grey = Color::from_rgba8(128, 128, 136, (alpha.clamp(0.0, 1.0) * 255.0) as u8);
                scene.fill(Fill::NonZero, css, grey, None, &Rect::new(left, top, left + tile, top + tile));
            }
        }
    }

    let (Some(clown), Some(shadow)) = (&pictures.clown, &pictures.shadow) else { return };
    let pose = frame.clown;
    let (pixels_wide, pixels_high) = (clown.width as f64, clown.height as f64);
    let width = CLOWN_WIDTH as f64;
    let height = width * pixels_high / pixels_wide;
    let squash = pose.squash as f64 * 0.18;
    // Around the middle of its feet: turned, squashed, then the picture stood on that point.
    let feet = css * Affine::translate((pose.x as f64, pose.y as f64)) * Affine::rotate((pose.turn as f64).to_radians()) * Affine::scale_non_uniform(1.0 + squash, 1.0 - squash);
    let picture = Affine::scale(width / pixels_wide);
    let source = Rect::new(0.0, 0.0, pixels_wide, pixels_high);
    let alpha = pose.opacity * screen;
    draw(scene, shadow, source, feet * Affine::translate((-width / 2.0, 3.0 - height)) * picture, alpha);
    draw(scene, clown, source, feet * Affine::translate((-width / 2.0, -height)) * picture, alpha);
}

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;
