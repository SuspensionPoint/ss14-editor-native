//! Baked textures: every image of a fork in one file, decoded once.
//!
//! Opening a station needs thousands of sprite images. Decoding PNGs every time a map opens gives
//! the same result each time, so it happens once per fork build instead. Each image is decoded to
//! 8-bit RGBA with straight alpha, compressed with LZ4 and appended to one file, with an index at
//! the end. At run time the file is memory-mapped, and loading an image costs one decompression
//! at memory speed.
//!
//! The colour stays sRGB and the alpha stays out of it. The renderer samples the images as sRGB
//! textures the way the game does, so the GPU decodes them to linear light before they are
//! blended, and alpha multiplied into sRGB bytes would come out wrong once decoded. An RSI whose
//! meta.json has `"load": { "srgb": false }` is one the game uploads without that decoding
//! (RsiLoading.cs, TextureLoadParameters). Its colours are stored here already encoded, so that
//! the decoding gives back the bytes of the file, as the game reads them.
//!
//! Layout: "SSTX2\0\0\0", the index's offset (u64), the compressed images, then the index. The
//! index is a count (u32) and, per image, the path's length (u16), the path, the offset (u64), the
//! compressed length (u32), width (u32) and height (u32). Little-endian throughout.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use rayon::prelude::*;

const MAGIC: &[u8; 8] = b"SSTX2\0\0\0";
/// The start of a file from before the images kept straight alpha.
const OLD_MAGIC: &[u8; 5] = b"SSTX1";

pub struct BakeReport {
    pub images: usize,
    pub failed: usize,
    pub pixels: u64,
    pub file_bytes: u64,
}

/// Bakes every PNG under the fork's two Resources folders. The fork's own files win over the
/// engine's, matching the order the engine mounts them in.
pub fn bake(fork_repo: &Path, output: &Path) -> std::io::Result<BakeReport> {
    bake_with(fork_repo, &fork_repo.join("RobustToolbox").join("Resources"), output)
}

/// The same, with the engine's Resources folder from elsewhere: a checkout whose engine
/// submodule is not set up has them beside the engine side built for it.
pub fn bake_with(fork_repo: &Path, engine_resources: &Path, output: &Path) -> std::io::Result<BakeReport> {
    let roots = [engine_resources.to_path_buf(), fork_repo.join("Resources")];
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    for root in &roots {
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
                    let relative = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    files.insert(format!("/{relative}"), path);
                }
            }
        }
    }

    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    let mut linear_folders: HashMap<PathBuf, bool> = HashMap::new();
    for path in files.values() {
        if let Some(folder) = path.parent() {
            linear_folders.entry(folder.to_path_buf()).or_insert_with(|| loads_linear(folder));
        }
    }
    let baked: Vec<(&String, Option<(u32, u32, Vec<u8>)>)> = names
        .par_iter()
        .map(|name| {
            let path = &files[*name];
            let linear = path.parent().is_some_and(|folder| linear_folders[folder]);
            let image = decode_straight(path).map(|(width, height, mut pixels)| {
                if linear {
                    encode_linear(&mut pixels);
                }
                (width, height, lz4_flex::compress(&pixels))
            });
            (*name, image)
        })
        .collect();

    let mut writer = BufWriter::new(File::create(output)?);
    writer.write_all(MAGIC)?;
    writer.write_all(&0u64.to_le_bytes())?;
    let mut offset = 16u64;
    let mut index = Vec::new();
    let mut report = BakeReport { images: 0, failed: 0, pixels: 0, file_bytes: 0 };
    for (name, image) in &baked {
        let Some((width, height, compressed)) = image else {
            report.failed += 1;
            continue;
        };
        writer.write_all(compressed)?;
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(name.as_bytes());
        index.extend_from_slice(&offset.to_le_bytes());
        index.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        index.extend_from_slice(&width.to_le_bytes());
        index.extend_from_slice(&height.to_le_bytes());
        offset += compressed.len() as u64;
        report.images += 1;
        report.pixels += *width as u64 * *height as u64;
    }
    writer.write_all(&(report.images as u32).to_le_bytes())?;
    writer.write_all(&index)?;
    let mut file = writer.into_inner()?;
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(8))?;
    file.write_all(&offset.to_le_bytes())?;
    report.file_bytes = offset + 4 + index.len() as u64;
    Ok(report)
}

/// Whether a folder is an RSI the game loads without decoding its sRGB: its meta.json has
/// `"load": { "srgb": false }`, which RsiLoading.cs reads into the RSI's TextureLoadParameters.
pub(crate) fn loads_linear(folder: &Path) -> bool {
    if !folder.extension().is_some_and(|e| e.eq_ignore_ascii_case("rsi")) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(folder.join("meta.json")) else { return false };
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else { return false };
    meta.get("load").and_then(|load| load.get("srgb")).and_then(|srgb| srgb.as_bool()) == Some(false)
}

/// Encodes the colour of straight RGBA pixels as sRGB, in place, so that an sRGB texture made
/// of them decodes back to the bytes they had.
pub(crate) fn encode_linear(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            *channel = crate::colour::linear_to_byte(*channel as f32 / 255.0);
        }
    }
}

/// Decodes a PNG to 8-bit RGBA with straight alpha.
pub(crate) fn decode_straight(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    decode_straight_from(std::io::BufReader::new(File::open(path).ok()?))
}

/// Decodes a PNG already read, with the alpha multiplied into the colour: the form Blitz takes
/// for a picture of the interface.
pub fn decode_premultiplied_from(png: impl std::io::BufRead + std::io::Seek) -> Option<(u32, u32, Vec<u8>)> {
    let (width, height, mut pixels) = decode_straight_from(png)?;
    crate::colour::premultiply(&mut pixels);
    Some((width, height, pixels))
}

/// Decodes a PNG already read, as [`decode_straight`] does a file.
pub fn decode_straight_from(png: impl std::io::BufRead + std::io::Seek) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(png);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let data = &buffer[..info.buffer_size()];
    let count = (info.width * info.height) as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    match info.color_type {
        png::ColorType::Rgba => pixels.extend_from_slice(data),
        png::ColorType::Rgb => data.chunks_exact(3).for_each(|p| pixels.extend_from_slice(&[p[0], p[1], p[2], 255])),
        png::ColorType::GrayscaleAlpha => data.chunks_exact(2).for_each(|p| pixels.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
        png::ColorType::Grayscale => data.iter().for_each(|&g| pixels.extend_from_slice(&[g, g, g, 255])),
        png::ColorType::Indexed => return None,
    }
    Some((info.width, info.height, pixels))
}

struct Entry {
    offset: u64,
    length: u32,
    width: u32,
    height: u32,
}

/// A whole image, as RGBA with straight alpha.
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Sheet {
    /// Copies one frame out: frames are laid out in reading order, `width` by `height` each.
    pub fn frame(&self, index: u32, width: u32, height: u32) -> Option<Vec<u8>> {
        let columns = (self.width / width.max(1)).max(1);
        let (left, top) = ((index % columns) * width, (index / columns) * height);
        if left + width > self.width || top + height > self.height {
            return None;
        }
        let mut frame = Vec::with_capacity((width * height * 4) as usize);
        for row in top..top + height {
            let start = ((row * self.width + left) * 4) as usize;
            frame.extend_from_slice(&self.pixels[start..start + (width * 4) as usize]);
        }
        Some(frame)
    }
}

/// A baked texture file, memory-mapped.
pub struct BakedTextures {
    /// None for [`BakedTextures::empty`].
    map: Option<Mmap>,
    index: HashMap<String, Entry>,
}

impl BakedTextures {
    /// No textures, for an interface to start with while the fork's are still being made.
    pub fn empty() -> BakedTextures {
        BakedTextures { map: None, index: HashMap::new() }
    }

    pub fn open(path: &Path) -> Result<BakedTextures, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let map = unsafe { Mmap::map(&file) }.map_err(|e| e.to_string())?;
        if map.len() >= 5 && &map[0..5] == OLD_MAGIC {
            return Err(format!("{} was baked by an older editor. Delete it and it will be baked again", path.display()));
        }
        if map.len() < 16 || &map[0..8] != MAGIC {
            return Err(format!("{} isn't a baked texture file", path.display()));
        }
        let mut at = u64::from_le_bytes(map[8..16].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(map[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        let mut index = HashMap::with_capacity(count);
        for _ in 0..count {
            let name_length = u16::from_le_bytes(map[at..at + 2].try_into().unwrap()) as usize;
            let name = String::from_utf8_lossy(&map[at + 2..at + 2 + name_length]).into_owned();
            at += 2 + name_length;
            let entry = Entry {
                offset: u64::from_le_bytes(map[at..at + 8].try_into().unwrap()),
                length: u32::from_le_bytes(map[at + 8..at + 12].try_into().unwrap()),
                width: u32::from_le_bytes(map[at + 12..at + 16].try_into().unwrap()),
                height: u32::from_le_bytes(map[at + 16..at + 20].try_into().unwrap()),
            };
            at += 20;
            index.insert(name, entry);
        }
        Ok(BakedTextures { map: Some(map), index })
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// A small picture of an image for a palette: its first frame, `size` pixels square, as
    /// premultiplied RGBA.
    pub fn thumbnail(&self, file: &str, size: u32) -> Option<Vec<u8>> {
        let mut pixels = self.sheet(file)?.frame(0, size, size)?;
        crate::colour::premultiply(&mut pixels);
        Some(pixels)
    }

    /// The image at a rooted resource path such as "/Textures/Tiles/steel.png".
    pub fn sheet(&self, file: &str) -> Option<Sheet> {
        let entry = self.index.get(file)?;
        let compressed = &self.map.as_ref()?[entry.offset as usize..(entry.offset + entry.length as u64) as usize];
        let pixels = lz4_flex::decompress(compressed, (entry.width * entry.height * 4) as usize).ok()?;
        Some(Sheet { width: entry.width, height: entry.height, pixels })
    }
}

#[cfg(test)]
#[path = "tests/textures.rs"]
mod tests;
