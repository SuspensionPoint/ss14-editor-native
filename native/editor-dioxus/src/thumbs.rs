//! The palettes' pictures, as PNG data URLs an `img` can show. Blitz can't be handed raw pixels for
//! an image element, so each picture is encoded once and kept.

use std::collections::HashMap;

use base64::Engine as _;
use editor_core::backdrop::Thumb;
use editor_core::session::Session;
use editor_core::textures::BakedTextures;
use editor_core::thumbs;

/// The size of the picture a palette shows beside a row under the mouse, as the web editor's.
pub(crate) const BIG: u32 = 128;

/// The pictures made so far, by what they show and their size. None for one with no picture.
#[derive(Default)]
pub(crate) struct Thumbs {
    tiles: HashMap<(String, u32), Option<String>>,
    entities: HashMap<(usize, u32), Option<String>>,
    decals: HashMap<String, Option<String>>,
    backdrops: HashMap<String, String>,
}

impl Thumbs {
    /// A tile's picture, this many pixels each way.
    pub(crate) fn tile(&mut self, textures: &BakedTextures, id: &str, sprite: Option<&str>, size: u32) -> Option<String> {
        self.tiles
            .entry((id.to_owned(), size))
            .or_insert_with(|| sprite.and_then(|sprite| textures.thumbnail(sprite, size)).map(|pixels| data_uri(&pixels, size)))
            .clone()
    }

    /// A prototype's picture, once the engine has said how it looks. None until then, and for
    /// prototypes that draw nothing.
    pub(crate) fn entity(&mut self, session: &Session, textures: &BakedTextures, index: usize, size: u32) -> Option<String> {
        if let Some(known) = self.entities.get(&(index, size)) {
            return known.clone();
        }
        let quads = session.preview(index, 0)?;
        let picture = thumbs::compose(&session.lists, textures, quads, size).map(|pixels| data_uri(&pixels, size));
        self.entities.insert((index, size), picture.clone());
        picture
    }

    /// A decal's picture, its first frame.
    pub(crate) fn decal(&mut self, session: &Session, textures: &BakedTextures, index: usize) -> Option<String> {
        let decal = session.decal_prototypes.get(index)?;
        if let Some(known) = self.decals.get(&decal.id) {
            return known.clone();
        }
        let picture = decal.file().and_then(|file| textures.thumbnail(&file, 32)).map(|pixels| data_uri(&pixels, 32));
        self.decals.insert(decal.id.clone(), picture.clone());
        picture
    }

    /// A backdrop's picture in the gallery, once the engine has painted it.
    pub(crate) fn backdrop(&mut self, thumb: &Thumb, name: &str) -> String {
        self.backdrops.entry(name.to_owned()).or_insert_with(|| picture_uri(&thumb.pixels, thumb.width, thumb.height)).clone()
    }
}

thread_local! {
    /// The star's pictures made so far, by whether it is filled and its size.
    static STARS: std::cell::RefCell<HashMap<(bool, u32), String>> = std::cell::RefCell::new(HashMap::new());
}

/// The favourite's star as a PNG data address, this many pixels each way: filled in gold, or
/// an outline in grey. The font Blitz draws with has no star, so it is drawn here.
pub(crate) fn star_uri(filled: bool, size: u32) -> String {
    STARS.with(|stars| stars.borrow_mut().entry((filled, size)).or_insert_with(|| data_uri(&star_pixels(filled, size), size)).clone())
}

/// A five-pointed star, point up, as premultiplied RGBA. Each pixel is sampled four by four
/// times for smooth edges. The outline is what of the star is not in a smaller star.
fn star_pixels(filled: bool, size: u32) -> Vec<u8> {
    let half = size as f32 / 2.0;
    let corners = |outer: f32| -> Vec<(f32, f32)> {
        (0..10)
            .map(|at| {
                let radius = if at % 2 == 0 { outer } else { outer * 0.42 };
                let angle = std::f32::consts::PI * at as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
                (half + radius * angle.cos(), half + 0.06 * size as f32 + radius * angle.sin())
            })
            .collect()
    };
    let inside = |points: &[(f32, f32)], (x, y): (f32, f32)| {
        let mut odd = false;
        for (at, &(ax, ay)) in points.iter().enumerate() {
            let (bx, by) = points[(at + 1) % points.len()];
            if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
                odd = !odd;
            }
        }
        odd
    };
    let outer = corners(half * 0.98);
    let hole = corners(half * 0.98 - size as f32 * 0.22);
    let colour = if filled { [245u32, 197, 66] } else { [150, 150, 165] };
    let samples = 4;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut covered = 0;
            for sy in 0..samples {
                for sx in 0..samples {
                    let point = (x as f32 + (sx as f32 + 0.5) / samples as f32, y as f32 + (sy as f32 + 0.5) / samples as f32);
                    if inside(&outer, point) && (filled || !inside(&hole, point)) {
                        covered += 1;
                    }
                }
            }
            let alpha = covered * 255 / (samples * samples);
            let at = ((y * size + x) * 4) as usize;
            for channel in 0..3 {
                pixels[at + channel] = (colour[channel] * alpha / 255) as u8;
            }
            pixels[at + 3] = alpha as u8;
        }
    }
    pixels
}

/// A premultiplied RGBA square as a PNG data address.
fn data_uri(pixels: &[u8], size: u32) -> String {
    picture_uri(pixels, size, size)
}

/// A premultiplied RGBA picture as a PNG data address.
pub(crate) fn picture_uri(pixels: &[u8], width: u32, height: u32) -> String {
    let mut straight = pixels.to_vec();
    for pixel in straight.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let mut file = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut writer| writer.write_image_data(&straight)).expect("thumbnail");
    }
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(file))
}
