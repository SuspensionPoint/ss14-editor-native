//! The avatars of the forks' owners on GitHub, shown on the start screen's cards. Each is
//! fetched once, by curl as a child process (Windows 10 and later and macOS have it), and kept
//! as a PNG under the user data folder. The screen never waits for one: it shows a monogram
//! until the picture is there, and keeps it when the network is not.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::quiet;
use crate::files::user_data;

/// The size GitHub is asked for, twice the badge's so that it stays sharp.
pub const SIZE: u32 = 96;

/// The folder the avatars are kept in.
pub fn avatars_folder() -> PathBuf {
    user_data().join("avatars")
}

/// Where an owner's avatar is on GitHub.
pub fn avatar_url(owner: &str) -> String {
    format!("https://github.com/{owner}.png?size={SIZE}")
}

/// The file an owner's avatar is kept in. Owners differ only in case on GitHub, so the name is
/// in small letters.
pub fn avatar_file(folder: &Path, owner: &str) -> PathBuf {
    folder.join(format!("{}.png", owner.to_lowercase()))
}

/// Whether bytes are a PNG file, by its signature.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

/// An owner's avatar: the kept one, else fetched and kept. None when it cannot be had, as
/// when offline.
pub fn avatar(folder: &Path, owner: &str) -> Option<Vec<u8>> {
    let file = avatar_file(folder, owner);
    if let Some(bytes) = std::fs::read(&file).ok().filter(|bytes| is_png(bytes)) {
        return Some(bytes);
    }
    fetch(&avatar_url(owner), &file).ok()
}

/// Fetches a PNG into a file: written beside it first, and moved in only when it is one.
pub fn fetch(url: &str, file: &Path) -> Result<Vec<u8>, String> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let partial = file.with_extension("png.partial");
    let status = quiet(Command::new("curl").args(["--fail", "--silent", "--location", "--max-time", "20", "--output"]).arg(&partial).arg(url))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("couldn't start curl: {error}"))?;
    let bytes = std::fs::read(&partial).unwrap_or_default();
    if !status.success() || !is_png(&bytes) {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{url} gave no picture"));
    }
    std::fs::rename(&partial, file).map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "../tests/forks/avatars.rs"]
mod tests;
