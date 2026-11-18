//! What the game draws behind a map: its backdrop ("parallax"). It is a stack of pictures, each
//! following the camera a little more slowly than the map.
//!
//! The game decides which backdrop a map has, what its layers are and where each one goes. The
//! engine names the layers as image files (src/Editor.Native, the `backdrop` command) and paints
//! the ones the game generates with the game's own generator. [`Layer::place`] is the arithmetic
//! of the game's ParallaxOverlay. The only part that is the editor's own is how large a layer is
//! drawn when the view is zoomed out further than the game ever shows a map (see
//! `MapRenderer::set_surround`). Reading the pictures takes a while, since a layer can be
//! thousands of pixels each way, so [`load`] belongs on its own thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::json;

use crate::engine::Engine;
use crate::textures::decode_premultiplied;

/// A picture and its smaller copies, each half the size of the one before, down to one pixel:
/// 8-bit RGBA with the alpha multiplied into the colour.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
}

impl Picture {
    fn new(width: u32, height: u32, pixels: Vec<u8>) -> Picture {
        let mut levels = vec![pixels];
        let (mut w, mut h) = (width as usize, height as usize);
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let from = levels.last().unwrap();
            let mut smaller = Vec::with_capacity(nw * nh * 4);
            for y in 0..nh {
                for x in 0..nw {
                    let (x0, y0) = ((x * 2).min(w - 1), (y * 2).min(h - 1));
                    let (x1, y1) = ((x * 2 + 1).min(w - 1), (y * 2 + 1).min(h - 1));
                    for channel in 0..4 {
                        let sum = from[(y0 * w + x0) * 4 + channel] as u32
                            + from[(y0 * w + x1) * 4 + channel] as u32
                            + from[(y1 * w + x0) * 4 + channel] as u32
                            + from[(y1 * w + x1) * 4 + channel] as u32;
                        smaller.push(((sum + 2) / 4) as u8);
                    }
                }
            }
            levels.push(smaller);
            (w, h) = (nw, nh);
        }
        Picture { width, height, levels }
    }

    /// The size of one of the smaller copies.
    pub fn level_size(&self, level: usize) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }
}

/// How the game places a layer; the names are the game's (ParallaxLayerConfig).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placing {
    pub scale_x: f32,
    pub scale_y: f32,
    /// Repeated over the whole view, or shown once around its home.
    pub tiled: bool,
    pub home_x: f32,
    pub home_y: f32,
    pub adjust_x: f32,
    pub adjust_y: f32,
    /// 0 fixes the layer to the world, 1 to the camera.
    pub slowness: f32,
    /// Tiles a second the layer drifts by itself.
    pub scroll_x: f32,
    pub scroll_y: f32,
}

impl Placing {
    /// The layer's bottom left corner and its size, in world tiles, for a camera looking at
    /// `eye`, `seconds` after the editor started, and a picture `pixels` large. For a tiled
    /// layer the corner is the one nearest the eye of all the corners it repeats at.
    pub fn place(&self, pixels: (u32, u32), eye: (f32, f32), seconds: f64) -> ((f32, f32), (f32, f32)) {
        let along = |size_pixels: u32, scale: f32, eye: f32, home: f32, adjust: f32, scroll: f32| {
            let size = (size_pixels as f64 / 32.0) * scale as f64;
            let (eye, home) = (eye as f64, home as f64);
            let mut corner = (eye - home) * self.slowness as f64 + scroll as f64 * seconds + home + adjust as f64 - size / 2.0;
            if self.tiled && size > 0.0 {
                corner += ((eye - corner) / size).floor() * size;
            }
            (corner as f32, size as f32)
        };
        let (x, width) = along(pixels.0, self.scale_x, eye.0, self.home_x, self.adjust_x, self.scroll_x);
        let (y, height) = along(pixels.1, self.scale_y, eye.1, self.home_y, self.adjust_y, self.scroll_y);
        ((x, y), (width, height))
    }
}

pub struct Layer {
    pub picture: Picture,
    pub placing: Placing,
}

/// A map's backdrop, bottom layer first. No layers means the game draws none there.
#[derive(Default)]
pub struct Backdrop {
    /// Different for every backdrop that was ever loaded, so that whoever shows one can tell
    /// whether it is the one it already has. 0 is the empty backdrop.
    pub id: u64,
    pub name: String,
    pub layers: Vec<Layer>,
    /// Layers the game has that are not shown.
    pub skipped: u32,
    /// How long the game's generator took to paint layers for this, if it had to.
    pub paint_ms: u64,
    /// Of a map's backdrop: the map names it, and is not left to the game's default.
    pub chosen: bool,
    /// Of a map's backdrop: the map is a planet's, which the game draws no backdrop behind.
    pub planet: bool,
}

impl Backdrop {
    /// Whether any layer drifts by itself, so that the picture changes while nothing is done.
    pub fn moves(&self) -> bool {
        self.layers.iter().any(|layer| layer.placing.scroll_x != 0.0 || layer.placing.scroll_y != 0.0)
    }
}

/// A small picture of a backdrop: 8-bit RGBA, the alpha multiplied into the colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The size of a backdrop's picture in the gallery, the shape of the game's window.
pub const THUMB_SIZE: (u32, u32) = (160, 90);

/// How many tiles across a backdrop's picture shows, about what the game shows of a map.
const THUMB_TILES: f32 = 40.0;

/// A backdrop's layers as they are drawn around a camera at the world's origin, on black,
/// scaled down to `size`. Each pixel takes the nearest pixel of the smaller copy of a layer
/// that is closest to its size.
pub fn thumbnail(backdrop: &Backdrop, size: (u32, u32)) -> Thumb {
    let (width, height) = (size.0.max(1), size.1.max(1));
    let mut pixels = [0, 0, 0, 255].repeat((width * height) as usize);
    // Tiles to a pixel of the picture made.
    let tiles = THUMB_TILES / width as f32;
    for layer in &backdrop.layers {
        let picture = &layer.picture;
        let ((left, bottom), (across, up)) = layer.placing.place((picture.width, picture.height), (0.0, 0.0), 0.0);
        if across <= 0.0 || up <= 0.0 {
            continue;
        }
        // The layer's pixels under one pixel made, and the smaller copy nearest that.
        let under = picture.width as f32 / across * tiles;
        let level = (under.max(1.0).log2().floor() as usize).min(picture.levels.len() - 1);
        let (level_width, level_height) = picture.level_size(level);
        let source = &picture.levels[level];
        for row in 0..height {
            for column in 0..width {
                let x = (column as f32 + 0.5 - width as f32 / 2.0) * tiles;
                let y = (height as f32 / 2.0 - row as f32 - 0.5) * tiles;
                let (mut u, mut v) = ((x - left) / across, (y - bottom) / up);
                if layer.placing.tiled {
                    (u, v) = (u.rem_euclid(1.0), v.rem_euclid(1.0));
                } else if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                // The picture's first row is its top.
                let sx = ((u * level_width as f32) as usize).min(level_width as usize - 1);
                let sy = (((1.0 - v) * level_height as f32) as usize).min(level_height as usize - 1);
                let from = &source[(sy * level_width as usize + sx) * 4..][..4];
                let to = &mut pixels[((row * width + column) * 4) as usize..][..4];
                let keep = 255 - from[3] as u32;
                for channel in 0..4 {
                    to[channel] = (from[channel] as u32 + (to[channel] as u32 * keep + 127) / 255).min(255) as u8;
                }
            }
        }
    }
    Thumb { width, height, pixels }
}

/// Where painted layers are kept between runs.
pub fn cache_folder() -> PathBuf {
    crate::files::user_data().join("backdrops")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Described {
    name: String,
    layers: Vec<DescribedLayer>,
    skipped: u32,
    paint_ms: u64,
    /// An engine from before the gallery does not say.
    #[serde(default)]
    chosen: bool,
    #[serde(default)]
    planet: bool,
}

#[derive(Deserialize)]
struct DescribedLayer {
    file: String,
    #[serde(flatten)]
    placing: Placing,
}

/// Asks the engine for a document's backdrop and reads its pictures. The first time a fork's
/// painted layers are needed the engine paints them, which takes seconds; neither that nor the
/// reading holds up the engine's other work.
pub fn load(engine: &Engine, document: i32, cache: &Path) -> Result<Backdrop, String> {
    read(engine.call("backdrop", &json!({ "doc": document, "cache": cache.to_string_lossy() }))?)
}

/// The same for one of the fork's backdrops by its id.
pub fn load_named(engine: &Engine, name: &str, cache: &Path) -> Result<Backdrop, String> {
    read(engine.call("backdrop.named", &json!({ "name": name, "cache": cache.to_string_lossy() }))?)
}

/// The ids of the fork's backdrops, in order. A quick question of the engine.
pub fn names(engine: &Engine) -> Result<Vec<String>, String> {
    engine.call("backdrops", &json!({}))
}

/// Reads the pictures the engine named.
fn read(described: Described) -> Result<Backdrop, String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut skipped = described.skipped;
    let mut layers = Vec::with_capacity(described.layers.len());
    for layer in described.layers {
        match decode_premultiplied(Path::new(&layer.file)) {
            Some((width, height, pixels)) if width > 0 && height > 0 => layers.push(Layer { picture: Picture::new(width, height, pixels), placing: layer.placing }),
            _ => skipped += 1,
        }
    }
    Ok(Backdrop {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        name: described.name,
        layers,
        skipped,
        paint_ms: described.paint_ms,
        chosen: described.chosen,
        planet: described.planet,
    })
}

#[cfg(test)]
#[path = "tests/backdrop.rs"]
mod tests;
