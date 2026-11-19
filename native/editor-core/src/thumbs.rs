//! Small palette pictures, made from the engine's own answer to "how does this look".
//!
//! The engine reports a prototype's look as the rectangles it would draw (a preview, see
//! `Session::want_previews`). Drawing those rectangles into a few dozen pixels gives a palette
//! picture with every layer of the sprite in it, exactly as on the map.

use std::collections::HashMap;

use crate::colour::{srgb_to_linear, LinearPicture};
use crate::drawlist::{quad_pixel_at, DrawLists, FLOATS_PER_QUAD};
use crate::textures::BakedTextures;

/// Draws rectangles of a draw list into a square picture `size` pixels wide, as premultiplied
/// RGBA. What is drawn is centred and, if it is larger than one tile, scaled down to fit.
/// It is blended in linear light with the tint made linear, as the map is (`crate::colour`).
/// None if there is nothing to draw.
pub fn compose(lists: &DrawLists, textures: &BakedTextures, quads: &[f32], size: u32) -> Option<Vec<u8>> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
        for corner in quad[..8].chunks_exact(2) {
            bounds = Some(match bounds {
                None => (corner[0], corner[1], corner[0], corner[1]),
                Some((l, b, r, t)) => (l.min(corner[0]), b.min(corner[1]), r.max(corner[0]), t.max(corner[1])),
            });
        }
    }
    let (left, bottom, right, top) = bounds?;
    let scale = size as f32 / (right - left).max(top - bottom).max(1.0);
    let (cx, cy) = ((left + right) / 2.0, (bottom + top) / 2.0);
    let half = size as f32 / 2.0;

    let mut frames: HashMap<u32, Option<(u32, u32, Vec<u8>)>> = HashMap::new();
    let mut picture = LinearPicture::filled(size, size, [0; 4]);
    let mut drawn = false;
    for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
        let texture = quad[12] as u32;
        let frame = frames.entry(texture).or_insert_with(|| {
            let source = lists.textures.get(&texture)?;
            let pixels = textures.sheet(&source.file())?.frame(source.index, source.width, source.height)?;
            Some((source.width, source.height, pixels))
        });
        let Some((width, height, pixels)) = frame else { continue };

        // The part of the picture this rectangle can touch.
        let xs = quad[..8].chunks_exact(2).map(|corner| half + (corner[0] - cx) * scale);
        let ys = quad[..8].chunks_exact(2).map(|corner| half - (corner[1] - cy) * scale);
        let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(low, high), x| (low.min(x), high.max(x)));
        let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(low, high), y| (low.min(y), high.max(y)));
        let clamp = |value: f32| value.clamp(0.0, size as f32) as u32;

        let tint = [srgb_to_linear(quad[8]), srgb_to_linear(quad[9]), srgb_to_linear(quad[10]), quad[11]];
        for py in clamp(y0.floor())..clamp(y1.ceil()) {
            for px in clamp(x0.floor())..clamp(x1.ceil()) {
                let world = (cx + (px as f32 + 0.5 - half) / scale, cy - (py as f32 + 0.5 - half) / scale);
                let Some((_, sx, sy)) = quad_pixel_at(quad, world) else { continue };
                let (sx, sy) = ((sx.max(0.0) as u32).min(*width - 1), (sy.max(0.0) as u32).min(*height - 1));
                let source = &pixels[((sy * *width + sx) * 4) as usize..][..4];
                if source[3] as f32 * tint[3] <= 0.0 {
                    continue;
                }
                picture.blend((py * size + px) as usize, source, tint);
                drawn = true;
            }
        }
    }
    drawn.then(|| picture.to_premultiplied_bytes())
}
