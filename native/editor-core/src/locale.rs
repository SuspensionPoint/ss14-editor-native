//! The fork's English text, looked up by localisation key. It is read from the `.ftl` files in
//! `Resources/Locale/en-US` on its own thread as the session starts. The editor uses it for device
//! port names, which the engine only reports as keys.
//!
//! Only plain messages are read: a key at the start of a line, `=`, and its text, which can carry
//! on over indented lines. Terms, attributes and comments are skipped. So is any message whose
//! text has a placeable (`{ ... }`), since only the game can fill one in. When a key isn't found,
//! the caller shows its id instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

/// Reads the plain messages of one `.ftl` file into a map of key to text. A key already in the
/// map keeps its text.
pub fn parse_ftl(text: &str, into: &mut HashMap<String, String>) {
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, first)) = message_start(line) else { continue };
        let mut value = first.trim().to_owned();
        // The text goes on over the indented lines after it, up to an attribute.
        while let Some(next) = lines.peek() {
            let trimmed = next.trim();
            if !next.starts_with([' ', '\t']) || trimmed.starts_with('.') {
                break;
            }
            if !trimmed.is_empty() {
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(trimmed);
            }
            lines.next();
        }
        if !value.is_empty() && !value.contains('{') {
            into.entry(key.to_owned()).or_insert(value);
        }
    }
}

/// The key and the rest of a line that begins a message: a name of letters, digits, `-` and
/// `_` that begins with a letter, at the start of the line, then `=`.
fn message_start(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim_end();
    let mut chars = key.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some((key, rest))
}

/// Every `.ftl` file under a folder, in the order of their paths.
fn ftl_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "ftl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The fork's English messages. A key in two files has the text of the first by path.
pub fn read_locale(fork_repo: &Path) -> HashMap<String, String> {
    let mut text = HashMap::new();
    for file in ftl_files(&fork_repo.join("Resources").join("Locale").join("en-US")) {
        if let Ok(contents) = std::fs::read_to_string(&file) {
            parse_ftl(&contents, &mut text);
        }
    }
    text
}

/// The fork's English text, once it has been read.
#[derive(Default)]
pub struct Locale {
    reading: Option<Receiver<HashMap<String, String>>>,
    text: HashMap<String, String>,
}

impl Locale {
    /// Starts reading the fork's English files. `wake` is called when they have been read.
    pub fn read(fork_repo: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Locale {
        let (send, reading) = mpsc::channel();
        let fork = PathBuf::from(fork_repo);
        let spawned = std::thread::Builder::new().name("locale".into()).spawn(move || {
            let _ = send.send(read_locale(&fork));
            wake();
        });
        Locale { reading: spawned.ok().map(|_| reading), text: HashMap::new() }
    }

    /// A locale of this text, read already.
    pub fn of(text: HashMap<String, String>) -> Locale {
        Locale { reading: None, text }
    }

    /// Takes the text if it has been read since the last call, and says so.
    pub fn poll(&mut self) -> bool {
        let Some(reading) = self.reading.as_ref() else { return false };
        match reading.try_recv() {
            Ok(text) => {
                self.reading = None;
                self.text = text;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.reading = None;
                false
            }
        }
    }

    /// The text of a key, if it has been read and is a plain message.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.text.get(key).map(String::as_str)
    }
}

#[cfg(test)]
#[path = "tests/locale.rs"]
mod tests;
