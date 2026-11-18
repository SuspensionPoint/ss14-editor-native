//! The cache of packs and baked textures, a folder for each fingerprint under the user data
//! folder: `cache/prototypes/<key>/kernel-resources.zip` and `cache/textures/<key>/textures.sstx`.
//! A fork opened again with the same prototypes or textures finds them there, and forks that
//! share them share the folder.
//!
//! What is missing is made by the session before the engine starts: the pack by the engine
//! side itself as a child process (`dotnet Editor.Native.dll pack`), and the textures here with
//! editor-core's own bake. Each is written beside its place and moved there once whole, so a
//! stopped run leaves nothing half made.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::fingerprint::Fingerprint;
use super::quiet;
use crate::files::{forward_slashes, user_data};

pub const PACK_FILE: &str = "kernel-resources.zip";
pub const TEXTURES_FILE: &str = "textures.sstx";
/// How many folders of each part are kept beyond those a known fork last used.
const SPARE: usize = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root }
    }

    /// The cache in the user data folder.
    pub fn standard() -> Cache {
        Cache::new(user_data().join("cache"))
    }

    pub fn pack(&self, key: &str) -> PathBuf {
        self.root.join("prototypes").join(key).join(PACK_FILE)
    }

    pub fn textures(&self, key: &str) -> PathBuf {
        self.root.join("textures").join(key).join(TEXTURES_FILE)
    }

    /// The folder of an engine side built for a fork's code (`engines`): the engine side in
    /// `engine`, its build's log, and while it builds the copy it is built from.
    pub fn engine_build(&self, key: &str) -> PathBuf {
        self.root.join("engines").join(key)
    }

    /// Takes away the folders of each part that no key in `keep` names, but for the [`SPARE`]
    /// written last, which a fork left out of the settings may still want. An engine side is
    /// moved aside before it is deleted, which fails while an editor runs on it, so that one
    /// in use is never left half deleted.
    pub fn prune(&self, keep: &[String]) {
        for part in ["prototypes", "textures", "engines"] {
            let Ok(entries) = std::fs::read_dir(self.root.join(part)) else { continue };
            let mut others: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
            for entry in entries.flatten() {
                if keep.iter().any(|key| entry.file_name().to_string_lossy() == key.as_str()) {
                    continue;
                }
                // What a prune that stopped part way moved aside.
                if entry.path().extension().is_some_and(|extension| extension == "old") {
                    let _ = super::download::remove_tree(&entry.path());
                    continue;
                }
                others.push((entry.metadata().and_then(|meta| meta.modified()).unwrap_or(std::time::UNIX_EPOCH), entry.path()));
            }
            others.sort_by_key(|other| std::cmp::Reverse(other.0));
            for (_, folder) in others.into_iter().skip(SPARE) {
                if part != "engines" {
                    let _ = std::fs::remove_dir_all(folder);
                    continue;
                }
                let mut aside = folder.clone().into_os_string();
                aside.push(".old");
                let aside = PathBuf::from(aside);
                if std::fs::rename(&folder, &aside).is_ok() {
                    let _ = super::download::remove_tree(&aside);
                }
            }
        }
    }
}

/// Where a fork's pack and textures are, and which must be made first, with what the loading
/// screen says while it is made.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub pack: PathBuf,
    pub textures: PathBuf,
    pub pack_stage: Option<String>,
    pub textures_stage: Option<String>,
    /// The keys, which the settings keep for the fork, to tell "changed since last time".
    pub prototypes_key: String,
    pub textures_key: String,
}

/// Where a fork's pack and textures are in the cache, and what must be made. `last` is the
/// keys it was last opened with, and `exists` tells whether a file is there.
pub fn plan(name: &str, cache: &Cache, prototypes: &Fingerprint, textures: &Fingerprint, last: (Option<&str>, Option<&str>), exists: impl Fn(&Path) -> bool) -> Prepared {
    let (prototypes_key, textures_key) = (prototypes.key(), textures.key());
    let pack = cache.pack(&prototypes_key);
    let baked = cache.textures(&textures_key);
    let pack_stage = (!exists(&pack)).then(|| match last.0 {
        None => format!("Packing {name}'s prototypes for the first time"),
        Some(key) if key != prototypes_key => format!("{name}'s prototypes changed since last time, packing them again"),
        Some(_) => format!("Packing {name}'s prototypes"),
    });
    let textures_stage = (!exists(&baked)).then(|| match last.1 {
        None => format!("Baking {name}'s textures for the first time (up to a minute)"),
        Some(key) if key != textures_key => format!("{name}'s textures changed since last time, baking them again (up to a minute)"),
        Some(_) => format!("Baking {name}'s textures (up to a minute)"),
    });
    Prepared { pack, textures: baked, pack_stage, textures_stage, prototypes_key, textures_key }
}

/// Makes the pack: the engine side packs the fork's prototypes in a process of its own, with
/// the `dotnet` the system has.
pub fn make_pack(engine_folder: &Path, fork: &Path, output: &Path) -> Result<(), String> {
    let partial = partial(output)?;
    let done = quiet(Command::new("dotnet").arg(engine_folder.join("Editor.Native.dll")).arg("pack").arg(fork).arg(&partial))
        .output()
        .map_err(|error| format!("couldn't run dotnet to pack the prototypes: {error}"))?;
    if !done.status.success() {
        let _ = std::fs::remove_file(&partial);
        let said = [&done.stderr, &done.stdout].map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned()).into_iter().find(|said| !said.is_empty()).unwrap_or_default();
        return Err(format!("packing the prototypes failed: {}", said.lines().last().unwrap_or("no message")));
    }
    finish(&partial, output)
}

/// Bakes the fork's textures, here in this process, with the engine's from its checkout or
/// from beside the engine side (`forks::engine_resources`).
pub fn make_textures(fork: &Path, engine_folder: &Path, output: &Path) -> Result<(), String> {
    let partial = partial(output)?;
    if let Err(error) = crate::textures::bake_with(fork, &super::engine_resources(fork, engine_folder), &partial) {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("baking the textures failed: {error}"));
    }
    finish(&partial, output)
}

/// The file a part is written to before it is whole, in the folder it goes to.
fn partial(output: &Path) -> Result<PathBuf, String> {
    let folder = output.parent().ok_or("the cache has no folder")?;
    std::fs::create_dir_all(folder).map_err(|error| format!("couldn't create {}: {error}", forward_slashes(folder)))?;
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    Ok(output.with_file_name(name))
}

fn finish(partial: &Path, output: &Path) -> Result<(), String> {
    std::fs::rename(partial, output).map_err(|error| format!("couldn't move {} into place: {error}", forward_slashes(output)))
}

#[cfg(test)]
#[path = "../tests/forks/cache.rs"]
mod tests;
