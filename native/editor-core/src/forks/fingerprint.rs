//! Fingerprints of what a fork checkout is made of, from git's own hashes: the prototypes the
//! pack is made from, the textures the bake reads, and the code the engine side is built from.
//!
//! A fingerprint is one line per thing, as a name and a value. In a git checkout each folder or
//! file is git's hash of it at HEAD (`git ls-tree HEAD -- <paths>`, as `git rev-parse
//! HEAD:<path>` gives it, with a missing path as `-`), and the working tree's changes there are
//! one more line: a hash of `git status --porcelain` with the contents of every file it lists,
//! so that editing a changed file again changes it too.
//!
//! A folder that is not a git checkout is fingerprinted by its files: how many, their bytes and
//! the newest time one was written. Its pack and textures are made again whenever a file there
//! changes. Its code cannot be told apart that way, and it is refused (`code_verdict`).
//!
//! The engine's own working tree (RobustToolbox) is not counted in the code: the editor's
//! engine patches are applied there in the checkout the engine side is built from. Its commit
//! is: the one checked out, or the one the fork names when its submodule is not set up
//! (`engine_commit`). A checkout without the engine's files takes them from the engine side the
//! editor built for it, so its prototypes and textures count that commit too.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::{git, is_git, Fnv};
use crate::files::forward_slashes;

/// What has to match the checkout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Prototypes,
    Textures,
    Code,
}

/// What `ResourcePack.Build` reads in each of the two Resources folders.
const PROTOTYPE_PATHS: [&str; 5] = ["Resources/EnginePrototypes", "Resources/Prototypes", "Resources/migration.yml", "Resources/PartialPrototypes", "Resources/IgnoredPrototypes"];
/// Where the textures are. The bake reads every PNG of the two Resources folders, and in the
/// forks all of them are under these.
const TEXTURE_PATHS: [&str; 1] = ["Resources/Textures"];
/// The projects the engine side is built against. The projects they refer to are followed.
const CONTENT_PROJECTS: [&str; 3] = ["Content.Shared/Content.Shared.csproj", "Content.Server/Content.Server.csproj", "Content.Client/Content.Client.csproj"];
/// Files of the checkout's top folder that change how all of it builds.
const BUILD_FILES: [&str; 3] = ["Directory.Packages.props", "Directory.Build.props", "global.json"];
const ENGINE: &str = "RobustToolbox";
/// The name of the first line of a fingerprint of a folder that is not a git checkout.
const NOT_GIT: &str = "not-git";
/// Bumped when the pack or the bake changes form, so that what the cache holds is made again.
pub const FORMAT: u32 = 1;

/// The name of the file beside the engine side that holds the code fingerprint it was built from.
pub const STAMP_FILE: &str = "code-fingerprint.txt";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fingerprint {
    pub lines: Vec<(String, String)>,
}

impl Fingerprint {
    /// The fingerprint of one part of a checkout as it is now.
    pub fn of(fork: &Path, part: Part) -> Fingerprint {
        let mut lines = Vec::new();
        if !is_git(fork) {
            lines.push((NOT_GIT.to_owned(), forward_slashes(fork)));
            if part != Part::Code {
                for (label, root) in [("", fork.to_path_buf()), ("RobustToolbox/", fork.join(ENGINE))] {
                    for path in paths(part) {
                        lines.push((format!("{label}{path}"), files(&root.join(&path))));
                    }
                }
            }
            return Fingerprint { lines };
        }
        match part {
            Part::Prototypes | Part::Textures => {
                spot(&mut lines, fork, "", &paths(part));
                spot(&mut lines, &fork.join(ENGINE), "RobustToolbox/", &paths(part));
                if !is_git(&fork.join(ENGINE)) {
                    lines.push((ENGINE.to_owned(), engine_commit(fork).unwrap_or_else(|| "-".to_owned())));
                }
            }
            Part::Code => {
                spot(&mut lines, fork, "", &code_paths(fork));
                lines.push((ENGINE.to_owned(), engine_commit(fork).unwrap_or_else(|| "-".to_owned())));
            }
        }
        Fingerprint { lines }
    }

    /// Whether it was made from a git checkout.
    pub fn is_git(&self) -> bool {
        self.lines.first().is_none_or(|(name, _)| name != NOT_GIT)
    }

    /// One line per thing, its name and value apart by a tab.
    pub fn text(&self) -> String {
        self.lines.iter().map(|(name, value)| format!("{name}\t{value}\n")).collect()
    }

    pub fn parse(text: &str) -> Fingerprint {
        let lines = text.lines().filter_map(|line| line.split_once('\t')).map(|(name, value)| (name.to_owned(), value.to_owned())).collect();
        Fingerprint { lines }
    }

    /// The name of its folder in the cache: a hash of the whole of it and of the cache's form.
    pub fn key(&self) -> String {
        let mut hash = Fnv::default();
        hash.add(&FORMAT.to_le_bytes());
        hash.add(self.text().as_bytes());
        hash.hex()
    }

    /// The names whose values differ between two fingerprints, or that only one has, in order.
    pub fn differences(&self, other: &Fingerprint) -> Vec<String> {
        let theirs: HashMap<&str, &str> = other.lines.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let ours: HashMap<&str, &str> = self.lines.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let mut names: Vec<String> = self.lines.iter().filter(|(name, value)| theirs.get(name.as_str()) != Some(&value.as_str())).map(|(name, _)| name.clone()).collect();
        names.extend(other.lines.iter().filter(|(name, _)| !ours.contains_key(name.as_str())).map(|(name, _)| name.clone()));
        names
    }
}

fn paths(part: Part) -> Vec<String> {
    match part {
        Part::Prototypes => PROTOTYPE_PATHS.map(String::from).to_vec(),
        Part::Textures => TEXTURE_PATHS.map(String::from).to_vec(),
        Part::Code => Vec::new(),
    }
}

/// The lines of some paths of one git checkout: git's hash of each at HEAD, and the working
/// tree's changes there. With git failing, the paths' files are counted instead.
fn spot(lines: &mut Vec<(String, String)>, repo: &Path, label: &str, paths: &[String]) {
    let listed = if is_git(repo) {
        let mut args = vec!["ls-tree", "-z", "HEAD", "--"];
        args.extend(paths.iter().map(String::as_str));
        git(repo, &args)
    } else {
        None
    };
    let Some(listed) = listed else {
        for path in paths {
            lines.push((format!("{label}{path}"), files(&repo.join(path))));
        }
        return;
    };
    // Each entry is "<mode> <type> <hash>\t<path>".
    let hashes: HashMap<String, String> = listed
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (meta, path) = entry.split_once('\t')?;
            Some((path.to_owned(), meta.split(' ').nth(2)?.to_owned()))
        })
        .collect();
    for path in paths {
        lines.push((format!("{label}{path}"), hashes.get(path).cloned().unwrap_or_else(|| "-".to_owned())));
    }
    let mut args = vec!["status", "--porcelain=v1", "-z", "--untracked-files=all", "--"];
    args.extend(paths.iter().map(String::as_str));
    lines.push((format!("{label}changes"), changes(repo, git(repo, &args))));
}

/// The working tree's changes as one value: `none`, or a hash of git's list of them and of the
/// contents of every file in it. If git could not tell, a value that matches nothing.
fn changes(repo: &Path, status: Option<Vec<u8>>) -> String {
    let Some(status) = status else {
        let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
        return format!("unknown {now}");
    };
    if status.is_empty() {
        return "none".to_owned();
    }
    let mut hash = Fnv::default();
    hash.add(&status);
    for path in changed_paths(&status) {
        if let Ok(bytes) = std::fs::read(repo.join(&path)) {
            hash.add(path.as_bytes());
            hash.add(&bytes);
        }
    }
    hash.hex()
}

/// The paths `git status --porcelain=v1 -z` lists. A rename or copy is followed by the path it
/// came from, which is not a file now.
pub(crate) fn changed_paths(status: &[u8]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut entries = status.split(|byte| *byte == 0).filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        paths.push(String::from_utf8_lossy(&entry[3..]).into_owned());
        if matches!(entry[0], b'R' | b'C') {
            entries.next();
        }
    }
    paths
}

/// The files under a path, as how many, their bytes and when the newest was written, in
/// seconds. A missing path is `-`.
fn files(path: &Path) -> String {
    let (mut count, mut bytes, mut newest) = (0u64, 0u64, 0u64);
    let mut pending = vec![path.to_path_buf()];
    let mut seen = false;
    while let Some(path) = pending.pop() {
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        seen = true;
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
            continue;
        }
        count += 1;
        bytes += meta.len();
        let written = meta.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs());
        newest = newest.max(written);
    }
    if !seen {
        return "-".to_owned();
    }
    format!("{count} {bytes} {newest}")
}

/// The engine's commit: the one its checkout has, or, when the fork's submodule is not set up,
/// the one the fork names at its HEAD. None if neither can be told.
pub fn engine_commit(fork: &Path) -> Option<String> {
    let engine = fork.join(ENGINE);
    if is_git(&engine) {
        return git(&engine, &["rev-parse", "HEAD"]).map(|commit| String::from_utf8_lossy(&commit).trim().to_owned());
    }
    // "160000 commit <hash>\tRobustToolbox"
    let listed = git(fork, &["ls-tree", "HEAD", "--", ENGINE])?;
    let listed = String::from_utf8_lossy(&listed);
    let (meta, _) = listed.trim().split_once('\t')?;
    let mut fields = meta.split(' ');
    (fields.next() == Some("160000") && fields.next() == Some("commit")).then(|| fields.next().map(str::to_owned)).flatten()
}

/// What the code fingerprint counts of a checkout, from its top: the projects' folders and the
/// files that change how all of it builds.
pub fn code_paths(fork: &Path) -> Vec<String> {
    let mut code = content_projects(fork);
    code.extend(BUILD_FILES.map(String::from));
    code
}

/// The folders of the projects the engine side is built against, from the checkout's top: the
/// three Content projects, the fork's modules beside them, and every project they refer to, but
/// the engine's.
pub fn content_projects(fork: &Path) -> Vec<String> {
    let top = lexical(fork);
    let mut queue: Vec<PathBuf> = CONTENT_PROJECTS.iter().map(|project| top.join(project)).collect();
    queue.extend(module_projects(&top));
    let mut folders = BTreeSet::new();
    while let Some(project) = queue.pop() {
        let Some(folder) = project.parent() else { continue };
        let Ok(inside) = folder.strip_prefix(&top) else { continue };
        let inside = forward_slashes(inside);
        if inside.is_empty() || inside == ENGINE || inside.starts_with("RobustToolbox/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&project) else { continue };
        if !folders.insert(inside) {
            continue;
        }
        queue.extend(references(&text).into_iter().map(|reference| lexical(&folder.join(reference.replace('\\', "/")))));
    }
    folders.into_iter().collect()
}

/// A fork's server and client modules, as Goob Station's Content.Goobstation.Server and
/// Content.Goobstation.Client, which nothing refers to: `Content.<name>.Server` and
/// `Content.<name>.Client`, as the editor's C# projects refer to them.
fn module_projects(top: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(top)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let middle = name.strip_prefix("Content.")?.strip_suffix(".Server").or_else(|| name.strip_prefix("Content.")?.strip_suffix(".Client"))?;
            let project = entry.path().join(format!("{name}.csproj"));
            (!middle.is_empty() && project.is_file()).then_some(project)
        })
        .collect();
    found.sort();
    found
}

/// The projects a project file refers to, as written in it.
pub(crate) fn references(project: &str) -> Vec<String> {
    const START: &str = "ProjectReference Include=\"";
    let mut found = Vec::new();
    let mut rest = project;
    while let Some(at) = rest.find(START) {
        rest = &rest[at + START.len()..];
        let Some(end) = rest.find('"') else { break };
        found.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    found
}

/// A path with its `.` and `..` worked out, without asking the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// What the editor knows of an engine side's build: the code fingerprint written beside it when
/// it was published, the build it was written for, and the checkout it was made from.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineStamp {
    /// The size and time of Editor.Native.dll when the stamp was written (`engine_identity`).
    pub engine: String,
    pub from: String,
    pub code: Fingerprint,
}

impl EngineStamp {
    pub fn read(engine_folder: &Path) -> Option<EngineStamp> {
        EngineStamp::parse(&std::fs::read_to_string(engine_folder.join(STAMP_FILE)).ok()?)
    }

    pub fn parse(text: &str) -> Option<EngineStamp> {
        let mut lines = text.splitn(3, '\n');
        let engine = lines.next()?.strip_prefix("engine\t")?.trim_end().to_owned();
        let from = lines.next()?.strip_prefix("from\t")?.trim_end().to_owned();
        let code = Fingerprint::parse(lines.next().unwrap_or(""));
        Some(EngineStamp { engine, from, code })
    }

    pub fn text(&self) -> String {
        format!("engine\t{}\nfrom\t{}\n{}", self.engine, self.from, self.code.text())
    }

    /// Writes the code fingerprint of the checkout an engine side was just built from beside it.
    pub fn write(engine_folder: &Path, fork: &Path) -> Result<EngineStamp, String> {
        EngineStamp::write_for(engine_folder, fork, Fingerprint::of(fork, Part::Code))
    }

    /// Writes a code fingerprint taken before the engine side was built, beside it.
    pub fn write_for(engine_folder: &Path, fork: &Path, code: Fingerprint) -> Result<EngineStamp, String> {
        let engine = engine_identity(engine_folder).ok_or_else(|| format!("{} has no Editor.Native.dll", engine_folder.display()))?;
        let from = std::fs::canonicalize(fork).map_or_else(|_| forward_slashes(fork), |full| forward_slashes(&full).trim_start_matches("//?/").to_owned());
        let stamp = EngineStamp { engine, from, code };
        std::fs::write(engine_folder.join(STAMP_FILE), stamp.text()).map_err(|error| format!("couldn't write {STAMP_FILE}: {error}"))?;
        Ok(stamp)
    }
}

/// The build of an engine side, as the size and a hash of the contents of its Editor.Native.dll.
/// A stamp written for another build is out of date: the engine side was published again by
/// hand since. Not the time it was written, which unzipping a release changes, so a packaged
/// engine side would have counted as unstamped and opened any fork.
pub fn engine_identity(engine_folder: &Path) -> Option<String> {
    let bytes = std::fs::read(engine_folder.join("Editor.Native.dll")).ok()?;
    let mut hash = Fnv::default();
    hash.add(&bytes);
    Some(format!("{} {}", bytes.len(), hash.hex()))
}

/// Whether a checkout's code may be opened with an engine side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeVerdict {
    Same,
    /// It may, with this warning: the engine side's fingerprint is missing or out of date.
    Unstamped(String),
    /// It may not, for this reason.
    Differs(String),
}

/// The safety rule: a map is never opened with an engine side whose code differs from the
/// checkout's, since saving would drop components the engine side does not know. An engine
/// side with no fingerprint, or one written for another build of it, cannot be judged and is
/// allowed with a warning. A checkout that is not a git checkout cannot be judged either, and
/// is refused.
pub fn code_verdict(name: &str, checkout: &Fingerprint, stamp: Option<&EngineStamp>, engine: Option<&str>) -> CodeVerdict {
    if !checkout.is_git() {
        return CodeVerdict::Differs(format!(
            "{name} isn't a git checkout, so the editor can't tell whether the engine side was built from its code. A map opened with different code could lose components when you save it."
        ));
    }
    let Some(stamp) = stamp else {
        return CodeVerdict::Unstamped(
            "The engine side has no code fingerprint, so the editor can't tell whether it was built from this fork's code. Publish it with tools/run.ps1 -Publish or tools/run.sh to write one.".to_owned(),
        );
    };
    if engine != Some(stamp.engine.as_str()) {
        return CodeVerdict::Unstamped(
            "The engine side was published again after its code fingerprint was written, so the editor can't tell whether it was built from this fork's code. Publish it with tools/run.ps1 -Publish or tools/run.sh to write a new one.".to_owned(),
        );
    }
    let differs = checkout.differences(&stamp.code);
    if differs.is_empty() {
        return CodeVerdict::Same;
    }
    CodeVerdict::Differs(format!(
        "{name}'s code isn't the code the engine side was built from. It differs in {}. A map opened with it could lose components when you save it. Check out the code the engine side was built from ({}), or publish the engine side from this fork.",
        described(&differs),
        stamp.from
    ))
}

/// The names of what differs, in words, as "a, b and c".
fn described(names: &[String]) -> String {
    let words: Vec<String> = names
        .iter()
        .map(|name| match name.as_str() {
            "changes" => "the uncommitted changes".to_owned(),
            ENGINE => "the engine's commit (RobustToolbox)".to_owned(),
            other => other.to_owned(),
        })
        .collect();
    match words.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "../tests/forks/fingerprint.rs"]
mod tests;
