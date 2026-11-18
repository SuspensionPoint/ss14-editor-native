//! The forks a mapper works on: the settings that remember them, the fingerprints of a
//! checkout's prototypes, textures and code, and the cache of packs and baked textures made
//! from them (docs/architecture.md).
//!
//! Opening a fork is [`check`]: the folder must look like a fork, an engine side must be built
//! from its code (the editor's own, one built for it before, or one to build: `engines`), and
//! its pack and textures are found in the cache or planned to be made. The session makes what
//! is missing before the engine starts (`cache::make`).
//!
//! Forks can also be downloaded by the editor into a folder of its own, and brought up to date
//! there (`download`).

pub mod avatars;
pub mod cache;
pub mod download;
pub mod engines;
pub mod fingerprint;
pub mod popular;
pub mod settings;

use std::path::{Path, PathBuf};
use std::process::Command;

use cache::{Cache, Prepared};
use engines::{Engine, Sources};
use fingerprint::{Fingerprint, Part};

/// The folders a fork checkout has, which "Add fork..." looks for.
pub const FORK_FOLDERS: [&str; 3] = ["Resources/Prototypes", "RobustToolbox", "Content.Shared"];

/// Why a folder is not a fork checkout, or None if it looks like one.
pub fn not_a_fork(folder: &Path) -> Option<String> {
    if !folder.is_dir() {
        return Some(format!("{} doesn't exist.", folder.display()));
    }
    let missing: Vec<&str> = FORK_FOLDERS.into_iter().filter(|wanted| !folder.join(wanted).is_dir()).collect();
    if missing.is_empty() {
        return None;
    }
    Some(format!("{} doesn't look like a fork checkout. It's missing {}.", folder.display(), missing.join(", ")))
}

/// The name a fork is shown by at first: its folder's name.
pub fn default_name(folder: &Path) -> String {
    folder.file_name().map_or_else(|| folder.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned())
}

/// The branch a checkout is on, or its commit's short hash when it is on none. None when it is
/// not a git checkout.
pub fn branch(folder: &Path) -> Option<String> {
    if !is_git(folder) {
        return None;
    }
    let name = text(git(folder, &["rev-parse", "--abbrev-ref", "HEAD"])?);
    if name != "HEAD" {
        return Some(name);
    }
    git(folder, &["rev-parse", "--short", "HEAD"]).map(|commit| format!("no branch, at {}", text(commit)))
}

/// The URL a checkout's `origin` remote fetches from, read only, for its card's avatar and
/// colour. None when it has none or is not a git checkout.
pub fn origin_url(folder: &Path) -> Option<String> {
    if !is_git(folder) {
        return None;
    }
    git(folder, &["config", "--get", "remote.origin.url"]).map(text).filter(|url| !url.is_empty())
}

/// Whether a folder is the top of a git checkout of its own. A folder inside another checkout
/// without a `.git` of its own is not, since its files would be hashed as the outer one's.
pub fn is_git(folder: &Path) -> bool {
    folder.join(".git").exists()
}

/// What opening a fork needs: where its pack and textures are, which must be made first, the
/// engine side it opens with, and a warning to show while it is open.
#[derive(Clone, Debug)]
pub struct Opening {
    pub prepared: Prepared,
    pub warning: Option<String>,
    pub engine: Engine,
}

/// Checks a fork before it is opened. Refused, with the message the start screen shows, when
/// the folder is not a fork, or its code is not what the editor's engine side was built from
/// and no engine side can be built for it. `home` is the editor's own engine side. `last` is
/// the keys of the pack and textures it was last opened with, from the settings.
pub fn check(fork: &Path, name: &str, home: &Path, last: (Option<&str>, Option<&str>), cache: &Cache) -> Result<Opening, String> {
    check_with(fork, name, home, last, cache, Sources::locate(home))
}

/// The same, with the editor's sources given, or none to build from.
pub fn check_with(fork: &Path, name: &str, home: &Path, last: (Option<&str>, Option<&str>), cache: &Cache, sources: Option<Sources>) -> Result<Opening, String> {
    if let Some(why) = not_a_fork(fork) {
        return Err(why);
    }
    let (engine, warning) = engines::choose(fork, name, home, cache, sources)?;
    let prototypes = Fingerprint::of(fork, Part::Prototypes);
    let textures = Fingerprint::of(fork, Part::Textures);
    let prepared = cache::plan(name, cache, &prototypes, &textures, last, |path| path.is_file());
    Ok(Opening { prepared, warning, engine })
}

/// The engine's Resources folder for a fork: its RobustToolbox's, or, when the checkout has not
/// set up the engine's submodule, the copy beside the engine side the editor built for it. As
/// `ResourcePack.EngineResources` on the engine side.
pub fn engine_resources(fork: &Path, engine_folder: &Path) -> PathBuf {
    let own = fork.join("RobustToolbox").join("Resources");
    if own.join("EnginePrototypes").is_dir() {
        return own;
    }
    let beside = engine_folder.join("RobustToolbox").join("Resources");
    if beside.is_dir() {
        beside
    } else {
        own
    }
}

/// Git's setting for paths longer than Windows' old 260 characters. Forks nest textures deep
/// (Goob Station has paths of 196 characters), and under the editor's own folder they pass 260,
/// so without it a clone cannot check out and git cannot read such files.
pub(crate) const LONG_PATHS: &str = "core.longpaths=true";

/// Git, to be run in a folder, with long paths allowed on Windows.
pub(crate) fn git_in(folder: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(folder);
    if cfg!(windows) {
        command.arg("-c").arg(LONG_PATHS);
    }
    command
}

/// A git command run in a folder. Its output, or None if git failed or is not installed.
pub(crate) fn git(folder: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = quiet(git_in(folder).args(args)).output().ok()?;
    output.status.success().then_some(output.stdout)
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_owned()
}

/// A command that opens no console window of its own on Windows.
pub(crate) fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// FNV-1a of 64 bits: a hash that stays the same from one build of the editor to the next, for
/// fingerprints and the names of cache folders. Not for anything that must resist tampering.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub(crate) fn add(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= *byte as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub(crate) fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

#[cfg(test)]
#[path = "../tests/forks.rs"]
pub(crate) mod tests;
