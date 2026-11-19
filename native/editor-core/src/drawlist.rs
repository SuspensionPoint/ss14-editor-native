//! The engine's draw lists as they arrive: for each document, what to draw and in what order.
//!
//! A draw list is the engine's own answer to "how does this map look". It is recorded from the
//! engine's sprite code as textured rectangles in world space. Nothing here makes any decision
//! about appearance. The message layout is described in
//! src/Content.EditorView/ViewWorld.DrawList.cs.

use std::collections::HashMap;

use serde_json::Value;

/// Floats per rectangle: 4 corners (x, y), tint (r, g, b, a), texture id, source rect in pixels.
pub const FLOATS_PER_QUAD: usize = 17;

/// What an item is, as the engine says: an entity, a chunk of tiles, a grid's decals, a marker
/// entity (a mapping aid the game hides from players), an entity that lives under the floor,
/// a piece of Starlight's plumbing (a duct or a plumbing machine, under the floor too), or a
/// disposal tube under the floor.
pub const KIND_ENTITY: u8 = 0;
pub const KIND_TILES: u8 = 1;
pub const KIND_DECALS: u8 = 2;
pub const KIND_MARKER: u8 = 3;
pub const KIND_SUBFLOOR: u8 = 4;
pub const KIND_PLUMBING: u8 = 5;
pub const KIND_DISPOSALS: u8 = 6;

/// The document id under which the engine reports how entity prototypes look (previews).
pub const PREVIEWS: i32 = -1;

/// A texture that is one plain white square, for the editor's own flat-coloured rectangles.
/// Use the source rectangle [`WHITE_SOURCE`] with it.
pub const WHITE_TEXTURE: u32 = 3_999_999;
pub const WHITE_SOURCE: [f32; 4] = [1.0, 1.0, 3.0, 3.0];

/// Texture ids the editor gives out itself start here; the engine's count up from zero. Both
/// have to survive being carried in a 32-bit float, which is exact up to 16 million.
const LOCAL_TEXTURES: u32 = 4_000_000;

/// Where a texture comes from, in terms of the fork's resource files.
#[derive(Clone, Debug)]
pub struct TextureSource {
    /// Resource path of the RSI folder, or of the image itself when there is no state.
    pub path: String,
    pub state: Option<String>,
    /// Which frame of the image, in reading order.
    pub index: u32,
    pub width: u32,
    pub height: u32,
    /// Of a frame of an animated state: the frame that follows it, the last followed by the
    /// first, and how many seconds it shows for. None and 0 for anything else.
    pub next: Option<u32>,
    pub delay: f32,
}

impl TextureSource {
    /// The image file this frame is cut from.
    pub fn file(&self) -> String {
        image_file(&self.path, self.state.as_deref())
    }
}

/// The image file of a picture given as the engine names one: a state of an RSI folder, or
/// with no state the image itself.
pub fn image_file(path: &str, state: Option<&str>) -> String {
    match state {
        Some(state) => format!("{path}/{state}.png"),
        None => path.to_owned(),
    }
}

/// If a map position is inside a rectangle of a draw list, the rectangle's texture and the
/// pixel of it that is there.
pub fn quad_pixel_at(quad: &[f32], world: (f32, f32)) -> Option<(u32, f32, f32)> {
    // Corners: bottom left, bottom right, top right, top left. Along the bottom edge and up
    // the left one, how far across the point is.
    let (ox, oy) = (quad[0], quad[1]);
    let (ax, ay) = (quad[2] - ox, quad[3] - oy);
    let (bx, by) = (quad[6] - ox, quad[7] - oy);
    let (px, py) = (world.0 - ox, world.1 - oy);
    let (aa, bb) = (ax * ax + ay * ay, bx * bx + by * by);
    if aa == 0.0 || bb == 0.0 {
        return None;
    }
    let (u, v) = ((px * ax + py * ay) / aa, (px * bx + py * by) / bb);
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return None;
    }
    // The source rectangle is left, top, right, bottom; the bottom-left corner shows (left, bottom).
    Some((quad[12] as u32, quad[13] + u * (quad[15] - quad[13]), quad[16] + v * (quad[14] - quad[16])))
}

/// One thing to draw: an entity, a chunk of tiles, or a grid's decals.
pub struct Item {
    pub kind: u8,
    pub depth: i32,
    pub order: u32,
    pub sort: f32,
    /// Breaks ties after the sort key: the depth a thing had before the game showed it through
    /// the floor, and its depth otherwise.
    pub tie: i32,
    pub quads: Vec<f32>,
}

#[derive(Default)]
pub struct Document {
    pub items: HashMap<i32, Item>,
    /// Different whenever the items are.
    pub version: u64,
    ordered: Option<Vec<i32>>,
    /// Items that changed since a renderer last asked, and whether the drawing order did.
    changed: Vec<i32>,
    order_changed: bool,
}

impl Document {
    /// Item ids in the order the engine's renderer draws them. Sorted again only after a
    /// change that can move something: an item coming, going, or getting a different key.
    pub fn in_draw_order(&mut self) -> &[i32] {
        if self.ordered.is_none() {
            let mut keys: Vec<(i32, u32, f32, i32, i32)> =
                self.items.iter().filter(|(_, item)| !item.quads.is_empty()).map(|(id, item)| (item.depth, item.order, item.sort, item.tie, *id)).collect();
            keys.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)).then(a.3.cmp(&b.3)).then(a.4.cmp(&b.4))
            });
            self.ordered = Some(keys.into_iter().map(|key| key.4).collect());
        }
        self.ordered.as_deref().unwrap()
    }

    /// What changed since the last call: the items, and whether the drawing order may have.
    pub fn take_changes(&mut self) -> (Vec<i32>, bool) {
        (std::mem::take(&mut self.changed), std::mem::take(&mut self.order_changed))
    }

    fn reorder(&mut self) {
        self.ordered = None;
        self.order_changed = true;
    }

    pub fn quad_count(&self) -> usize {
        self.items.values().map(|item| item.quads.len()).sum::<usize>() / FLOATS_PER_QUAD
    }

    /// A box around the main body of what is drawn, in world tiles: (min x, min y, max x, max y).
    /// A map file can hold several grids far apart (a station and its shuttles). Starting from
    /// the middle rectangle, this takes everything outwards until the first gap of 16 tiles, so
    /// a distant grid does not shrink the main one to a dot.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut centres: Vec<(f32, f32)> = Vec::new();
        for item in self.items.values() {
            for quad in item.quads.chunks_exact(FLOATS_PER_QUAD) {
                centres.push(((quad[0] + quad[4]) / 2.0, (quad[1] + quad[5]) / 2.0));
            }
        }
        if centres.is_empty() {
            return None;
        }
        let middle = |pick: fn(&(f32, f32)) -> f32, centres: &[(f32, f32)]| {
            let mut values: Vec<f32> = centres.iter().map(pick).collect();
            values.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            values[values.len() / 2]
        };
        let (mx, my) = (middle(|c| c.0, &centres), middle(|c| c.1, &centres));
        centres.sort_unstable_by(|a, b| {
            let (da, db) = ((a.0 - mx).hypot(a.1 - my), (b.0 - mx).hypot(b.1 - my));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut kept = centres.len();
        for i in centres.len() / 2..centres.len() - 1 {
            let (near, far) = (centres[i], centres[i + 1]);
            if (far.0 - mx).hypot(far.1 - my) - (near.0 - mx).hypot(near.1 - my) > 16.0 {
                kept = i + 1;
                break;
            }
        }
        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in &centres[..kept] {
            bounds = (bounds.0.min(*x), bounds.1.min(*y), bounds.2.max(*x), bounds.3.max(*y));
        }
        Some((bounds.0 - 2.0, bounds.1 - 2.0, bounds.2 + 2.0, bounds.3 + 2.0))
    }
}

/// What one message changed.
pub struct Applied {
    pub document: i32,
    pub full: bool,
    pub items: usize,
    pub removed: usize,
    pub quads: usize,
}

#[derive(Default)]
pub struct DrawLists {
    /// Texture ids stay valid for as long as the engine runs, so this only ever grows.
    pub textures: HashMap<u32, TextureSource>,
    pub documents: HashMap<i32, Document>,
    next_version: u64,
    local: HashMap<(String, u32), u32>,
}

impl DrawLists {
    /// A texture id for a frame of an image file that the engine has not drawn with yet, for
    /// the editor's own rectangles (a tile that has been asked for and not yet answered).
    pub fn local_texture(&mut self, path: &str, index: u32, width: u32, height: u32) -> u32 {
        if let Some(&id) = self.local.get(&(path.to_owned(), index)) {
            return id;
        }
        let id = LOCAL_TEXTURES + self.local.len() as u32;
        self.local.insert((path.to_owned(), index), id);
        self.textures.insert(id, TextureSource { path: path.to_owned(), state: None, index, width, height, next: None, delay: 0.0 });
        id
    }

    /// Folds a draw-list message from the engine into the store.
    pub fn apply(&mut self, message: &[u8]) -> Result<Applied, String> {
        if message.len() < 8 || &message[0..4] != b"DRW1" {
            return Err("not a draw-list message".into());
        }
        let header_length = i32::from_le_bytes(message[4..8].try_into().unwrap()) as usize;
        let header: Value = serde_json::from_slice(&message[8..8 + header_length]).map_err(|e| e.to_string())?;
        let floats = &message[8 + header_length..];

        for texture in header["textures"].as_array().into_iter().flatten() {
            self.textures.insert(
                texture["id"].as_u64().unwrap_or(0) as u32,
                TextureSource {
                    path: texture["path"].as_str().unwrap_or("").to_owned(),
                    state: texture["state"].as_str().map(str::to_owned),
                    index: texture["index"].as_u64().unwrap_or(0) as u32,
                    width: texture["width"].as_u64().unwrap_or(0) as u32,
                    height: texture["height"].as_u64().unwrap_or(0) as u32,
                    // Older engine builds do not say.
                    next: texture["next"].as_u64().map(|next| next as u32),
                    delay: texture["delay"].as_f64().unwrap_or(0.0) as f32,
                },
            );
        }

        let id = header["doc"].as_i64().unwrap_or(0) as i32;
        let full = header["full"] == true;
        if full {
            self.documents.remove(&id);
        }
        let document = self.documents.entry(id).or_default();

        let mut offset = 0;
        let mut quads = 0;
        let items = header["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for item in items {
            let count = item[4].as_u64().unwrap_or(0) as usize;
            let bytes = &floats[offset..offset + count * FLOATS_PER_QUAD * 4];
            offset += bytes.len();
            quads += count;
            let item_id = item[0].as_i64().unwrap_or(0) as i32;
            let depth = item[1].as_i64().unwrap_or(0) as i32;
            let new = Item {
                // Older engine builds do not say; then the id tells tiles and decals apart.
                kind: item[5].as_u64().map(|kind| kind as u8).unwrap_or(if item_id <= -1_000_000 {
                    KIND_DECALS
                } else if item_id < 0 {
                    KIND_TILES
                } else {
                    KIND_ENTITY
                }),
                depth,
                order: item[2].as_u64().unwrap_or(0) as u32,
                sort: item[3].as_f64().unwrap_or(0.0) as f32,
                // Older engine builds do not say. The depth then ties every item of that depth.
                tie: item[6].as_i64().map(|tie| tie as i32).unwrap_or(depth),
                quads: bytes.chunks_exact(4).map(|f| f32::from_le_bytes(f.try_into().unwrap())).collect(),
            };
            let moved = match document.items.get(&item_id) {
                Some(old) => old.kind != new.kind || old.depth != new.depth || old.order != new.order || old.sort != new.sort || old.tie != new.tie || old.quads.is_empty() != new.quads.is_empty(),
                None => !new.quads.is_empty(),
            };
            if moved {
                document.reorder();
            }
            document.items.insert(item_id, new);
            document.changed.push(item_id);
        }
        let removed = header["removed"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for gone in removed {
            if document.items.remove(&(gone.as_i64().unwrap_or(0) as i32)).is_some_and(|old| !old.quads.is_empty()) {
                document.reorder();
            }
        }

        self.next_version += 1;
        document.version = self.next_version;
        Ok(Applied { document: id, full, items: items.len(), removed: removed.len(), quads })
    }
}

#[cfg(test)]
#[path = "tests/drawlist.rs"]
mod tests;
