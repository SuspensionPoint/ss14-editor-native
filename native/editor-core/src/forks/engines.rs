//! The engine side built for a fork's own code. The engine side the editor comes with is built from the pinned fork. A fork whose
//! code differs gets one of its own, built once and kept in the cache by its code fingerprint
//! and the editor's own sources.
//!
//! The build never touches the mapper's checkout. It works in a copy in the cache: a shared
//! clone of the checkout at its HEAD (its objects are read from the checkout, nothing is
//! written there), checked out sparse to the code alone, with the changes not committed in the
//! code copied over. The engine (RobustToolbox) is cloned from the checkout's own when it is
//! set up there, and fetched at the commit the fork names when it is not, with its submodules.
//! The editor's engine patches go on it as units, each adding one ability: a unit the engine
//! has already is left out, else the first of its variants that fits goes on (`apply_units`).
//! Then the editor's C# projects, copied
//! beside it, are published against it with the SDK of the fork's global.json, with
//! ReadyToRun, as tools/run.ps1 -Publish does. Build output and obj folders stay in the copy,
//! which is deleted once the engine side is in place. The engine side gets the engine's
//! Resources beside it, for a checkout that has not, and its code fingerprint.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::cache::Cache;
use super::download::{remove_tree, Failure, Git, Progress, Stopper};
use super::fingerprint::{self, code_paths, content_projects, engine_commit, CodeVerdict, EngineStamp, Fingerprint, Part};
use super::{git, is_git, quiet, Fnv};
use crate::files::forward_slashes;

/// Bumped when the build changes what it makes, so that what the cache holds is built again.
pub const FORMAT: u32 = 1;

/// The manifest in each unit's folder of patches/engine.
pub const UNIT_MANIFEST: &str = "unit.txt";

/// The editor's C# projects the engine side is made of, under `src`.
const EDITOR_PROJECTS: [&str; 4] = ["Editor.Kernel", "Editor.Native", "Editor.ViewKernel", "Content.EditorView"];

/// The editor's files beside the projects that change how they build.
const EDITOR_BUILD_FILES: [&str; 4] = ["src/Directory.Build.props", "src/Directory.Build.targets", "Directory.Packages.props", "nuget.config"];

const ENGINE: &str = "RobustToolbox";

/// The editor's own sources, which an engine side is built from: the C# projects and the
/// engine patches. Found beside the editor or its engine side.
#[derive(Clone, Debug, PartialEq)]
pub struct Sources {
    pub root: PathBuf,
}

impl Sources {
    /// The sources in a folder, if it has them.
    pub fn at(root: &Path) -> Option<Sources> {
        (root.join("src/Editor.Native/Editor.Native.csproj").is_file() && root.join("patches/engine").is_dir()).then(|| Sources { root: root.to_path_buf() })
    }

    /// The first folder above one of these that has the sources.
    pub fn find(starts: &[PathBuf]) -> Option<Sources> {
        starts.iter().find_map(|start| start.ancestors().find_map(Sources::at))
    }

    /// The sources above the editor's program or above the engine side it came with.
    pub fn locate(home_engine: &Path) -> Option<Sources> {
        let mut starts = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            starts.push(exe);
        }
        starts.push(std::fs::canonicalize(home_engine).unwrap_or_else(|_| home_engine.to_path_buf()));
        Sources::find(&starts)
    }

    pub fn patches(&self) -> PathBuf {
        self.root.join("patches").join("engine")
    }

    /// The files an engine side is built from, from the root, in order: the projects but their
    /// bin and obj folders, the files that change how they build, and the patches.
    pub fn files(&self) -> Vec<String> {
        let mut files = self.build_files();
        files.extend(walk(&self.root, &self.patches()));
        files.sort();
        files
    }

    /// The files copied beside the fork to be built: the projects and their build files.
    fn build_files(&self) -> Vec<String> {
        let mut files: Vec<String> = EDITOR_PROJECTS.iter().flat_map(|project| walk(&self.root, &self.root.join("src").join(project))).collect();
        files.extend(EDITOR_BUILD_FILES.iter().filter(|file| self.root.join(file).is_file()).map(|file| (*file).to_owned()));
        files.sort();
        files
    }

    /// A hash of every file the engine side is built from, their line ends made alike, so
    /// that a checkout with Windows line ends hashes as one without.
    pub fn hash(&self) -> String {
        let mut hash = Fnv::default();
        for file in self.files() {
            hash.add(file.as_bytes());
            hash.add(&[0]);
            if let Ok(bytes) = std::fs::read(self.root.join(&file)) {
                let bytes: Vec<u8> = bytes.into_iter().filter(|byte| *byte != b'\r').collect();
                hash.add(&bytes);
            }
            hash.add(&[0]);
        }
        hash.hex()
    }
}

/// The files under a folder, from `root`, with forward slashes. Bin, obj and hidden folders are
/// left out.
fn walk(root: &Path, folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !matches!(name.as_str(), "bin" | "obj") && !name.starts_with('.') {
                    pending.push(path);
                }
            } else if let Ok(inside) = path.strip_prefix(root) {
                found.push(forward_slashes(inside));
            }
        }
    }
    found
}

/// The name of an engine side's folder in the cache: a hash of the build's form, the code
/// fingerprint and the editor's sources.
pub fn key(code: &Fingerprint, sources_hash: &str) -> String {
    let mut hash = Fnv::default();
    hash.add(&FORMAT.to_le_bytes());
    hash.add(code.text().as_bytes());
    hash.add(sources_hash.as_bytes());
    hash.hex()
}

/// Which engine side opens a fork.
#[derive(Clone, Debug)]
pub enum Engine {
    /// This one.
    Ready(PathBuf),
    /// One must be built first.
    Build(Box<BuildJob>),
}

/// An engine side to build for a fork's code.
#[derive(Clone, Debug)]
pub struct BuildJob {
    pub fork: PathBuf,
    pub name: String,
    /// The fingerprint of its code when it was asked for, which the engine side is stamped with.
    pub code: Fingerprint,
    pub key: String,
    /// Its folder in the cache.
    pub folder: PathBuf,
    pub sources: Sources,
    /// Why the engine side the editor came with will not do, in words.
    pub reason: String,
}

impl BuildJob {
    /// Where the engine side goes.
    pub fn engine(&self) -> PathBuf {
        self.folder.join("engine")
    }

    /// The copy it is built in, while it builds.
    pub fn work(&self) -> PathBuf {
        self.folder.join("work")
    }

    pub fn log(&self) -> PathBuf {
        self.folder.join("build.log")
    }
}

/// Chooses the engine side for a fork: the one the editor came with (`home`) when the fork's
/// code is what it was built from, else one built for the code before, else a build. A fork
/// that is not a git checkout is refused, as is one the editor cannot build for (no sources).
/// With it a warning to show while the fork is open, when the editor's own engine side has no
/// code fingerprint to tell by.
pub fn choose(fork: &Path, name: &str, home: &Path, cache: &Cache, sources: Option<Sources>) -> Result<(Engine, Option<String>), String> {
    let code = Fingerprint::of(fork, Part::Code);
    let why = match fingerprint::code_verdict(name, &code, EngineStamp::read(home).as_ref(), fingerprint::engine_identity(home).as_deref()) {
        CodeVerdict::Same => return Ok((Engine::Ready(home.to_path_buf()), None)),
        CodeVerdict::Unstamped(warning) => return Ok((Engine::Ready(home.to_path_buf()), Some(warning))),
        CodeVerdict::Differs(why) => why,
    };
    if !code.is_git() {
        return Err(why);
    }
    let Some(sources) = sources else {
        return Err(format!("{why} The editor can't build an engine side for it, because its own sources (src/Editor.Native and patches/engine) aren't next to it."));
    };
    let key = key(&code, &sources.hash());
    let folder = cache.engine_build(&key);
    let job = BuildJob { fork: fork.to_path_buf(), name: name.to_owned(), code, key, folder, sources, reason: why };
    if is_built(&job.engine(), &job.code) {
        return Ok((Engine::Ready(job.engine()), None));
    }
    Ok((Engine::Build(Box::new(job)), None))
}

/// The key of an engine side the cache holds, or None for one elsewhere, as the editor's own.
pub fn key_of(cache: &Cache, engine: &Path) -> Option<String> {
    let build = engine.parent()?;
    let engines = cache.engine_build("-");
    (build.parent()? == engines.parent()?).then(|| file_name(build))
}

/// Whether a folder holds an engine side built and stamped for this code.
pub fn is_built(engine: &Path, code: &Fingerprint) -> bool {
    let Some(stamp) = EngineStamp::read(engine) else { return false };
    fingerprint::engine_identity(engine).as_deref() == Some(stamp.engine.as_str()) && code.differences(&stamp.code).is_empty()
}

/// The build's stages, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Copying,
    FetchingEngine,
    Patching,
    Building,
    Finishing,
}

/// Where a build is, for the screen.
#[derive(Clone, Debug)]
pub struct BuildProgress {
    pub stage: Stage,
    /// What git says while the engine is fetched.
    pub git: Option<Progress>,
    /// How many projects are built so far.
    pub built: usize,
    pub started: Instant,
}

impl BuildProgress {
    pub fn new() -> BuildProgress {
        BuildProgress { stage: Stage::Copying, git: None, built: 0, started: Instant::now() }
    }

    /// The bar's share, when there is one to tell.
    pub fn percent(&self) -> Option<u32> {
        match self.stage {
            Stage::FetchingEngine => self.git.as_ref().and_then(|git| git.percent),
            _ => None,
        }
    }

    /// One line, as "Building the engine side for Delta-v, 2 min so far: building, which takes
    /// minutes. 7 projects built".
    pub fn text(&self, name: &str) -> String {
        format!("Building the engine side for {name}, {} so far: {}", elapsed(self.started.elapsed()), self.doing(name))
    }

    /// The stage alone, as "building, which takes minutes. 7 projects built".
    pub fn doing(&self, name: &str) -> String {
        match self.stage {
            Stage::Copying => format!("copying {name}'s code"),
            Stage::FetchingEngine => match &self.git {
                Some(git) if !git.step.is_empty() => format!("fetching the engine. {}", git.text()),
                _ => "fetching the engine".to_owned(),
            },
            Stage::Patching => "patching the engine".to_owned(),
            Stage::Building => match self.built {
                0 => "building (takes a few minutes)".to_owned(),
                1 => "building (takes a few minutes). 1 project built".to_owned(),
                built => format!("building (takes a few minutes). {built} projects built"),
            },
            Stage::Finishing => "moving it into place".to_owned(),
        }
    }
}

impl Default for BuildProgress {
    fn default() -> BuildProgress {
        BuildProgress::new()
    }
}

/// A time as "40 s" or "3 min".
pub fn elapsed(time: Duration) -> String {
    let seconds = time.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}

/// Builds the engine side for a fork's code into the cache. Where it is once built.
pub fn build(git: &Git, job: &BuildJob, stopper: &Stopper, report: &mut dyn FnMut(&BuildProgress)) -> Result<PathBuf, Failure> {
    let mut progress = BuildProgress::new();
    let work = job.work();
    let partial = job.folder.join("engine.partial");
    for leftover in [&work, &partial] {
        if leftover.exists() {
            remove_tree(leftover).map_err(Failure::Failed)?;
        }
    }
    std::fs::create_dir_all(&work).map_err(|error| Failure::Failed(format!("couldn't create {}: {error}", work.display())))?;
    let mut log = Log::new(job);
    let built = build_in(git, job, &work, &partial, stopper, &mut progress, report, &mut log);
    if let Err(failure) = &built {
        log.line(&format!("failed: {}", failure.message()));
    }
    log.save();
    // The copy and its output are gigabytes. The log says what happened.
    let _ = remove_tree(&work);
    if built.is_err() {
        let _ = remove_tree(&partial);
    }
    built
}

#[allow(clippy::too_many_arguments)]
fn build_in(git: &Git, job: &BuildJob, work: &Path, partial: &Path, stopper: &Stopper, progress: &mut BuildProgress, report: &mut dyn FnMut(&BuildProgress), log: &mut Log) -> Result<PathBuf, Failure> {
    let fork = work.join("fork");
    progress.stage = Stage::Copying;
    report(progress);
    copy_fork(git, &job.fork, &fork, stopper)?;
    log.line("copied the fork's code");

    progress.stage = Stage::FetchingEngine;
    report(progress);
    fetch_engine(git, &job.fork, &fork, stopper, &mut |now| {
        progress.git = Some(now.clone());
        report(progress);
    })?;
    log.line(&format!("the engine at {}", engine_commit(&fork).unwrap_or_default()));

    progress.stage = Stage::Patching;
    progress.git = None;
    report(progress);
    let engine = fork.join(ENGINE);
    let units = native_units(&job.sources.patches()).map_err(Failure::Failed)?;
    let fits = apply_units(&engine, &units, &job.name).map_err(Failure::Failed)?;
    log.line(&format!("patched: {}", fits_text(&fits)));

    let editor = work.join("editor");
    copy_sources(&job.sources, &editor, &fork).map_err(Failure::Failed)?;
    if let Some(note) = sdk_check(&editor, &fork, &job.name).map_err(Failure::Failed)? {
        log.line(&note);
    }

    progress.stage = Stage::Building;
    report(progress);
    let mut lines = Vec::new();
    let mut published = publish(&editor, &fork, partial, stopper, &mut |line| {
        if line.contains(" -> ") {
            progress.built += 1;
            report(progress);
        }
        lines.push(line.to_owned());
    })?;
    // MSBuild may build a project twice at once and trip over its own files. Once more mends it.
    if !published && lines.iter().any(|line| line.contains("being used by another process")) {
        log.lines(&lines);
        log.line("built again because a file was in use");
        lines.clear();
        published = publish(&editor, &fork, partial, stopper, &mut |line| lines.push(line.to_owned()))?;
    }
    log.lines(&lines);
    if !published {
        return Err(Failure::Failed(build_failure(&job.name, &lines, &job.log())));
    }

    progress.stage = Stage::Finishing;
    report(progress);
    copy_tree(&engine.join("Resources"), &partial.join(ENGINE).join("Resources")).map_err(Failure::Failed)?;
    EngineStamp::write_for(partial, &job.fork, job.code.clone()).map_err(Failure::Failed)?;
    let target = job.engine();
    if target.exists() {
        remove_tree(&target).map_err(Failure::Failed)?;
    }
    std::fs::rename(partial, &target).map_err(|error| Failure::Failed(format!("couldn't move the engine side into place: {error}")))?;
    log.line(&format!("built in {}", elapsed(progress.started.elapsed())));
    Ok(target)
}

/// The build's log, written beside the engine side.
struct Log {
    file: PathBuf,
    text: String,
}

impl Log {
    fn new(job: &BuildJob) -> Log {
        let text = format!("engine side for {} ({})\n{}\n", forward_slashes(&job.fork), job.key, job.code.text());
        Log { file: job.log(), text }
    }

    fn line(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }

    fn lines(&mut self, lines: &[String]) {
        for line in lines {
            self.line(line);
        }
    }

    fn save(&self) {
        let _ = std::fs::write(&self.file, &self.text);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn run_git(git: &Git, folder: Option<&Path>, args: &[&str], stopper: &Stopper, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    git.run(folder, &args, stopper, &mut Progress::new("Fetching"), report)
}

/// The folders of the checkout the build reads: the projects and the MSBuild folder they
/// import. The files of the top folder come too.
pub fn sparse_folders(fork: &Path) -> Vec<String> {
    let mut folders = content_projects(fork);
    folders.push("MSBuild".to_owned());
    folders
}

/// Copies the fork's code at its HEAD, with the changes not committed in the code, into
/// `copy`. The checkout is only read: the copy borrows its objects (`clone --shared`).
pub fn copy_fork(git: &Git, fork: &Path, copy: &Path, stopper: &Stopper) -> Result<(), Failure> {
    let head = super::git(fork, &["rev-parse", "HEAD"]).map(|head| String::from_utf8_lossy(&head).trim().to_owned()).ok_or_else(|| Failure::Failed(format!("{} has no commit to build from.", fork.display())))?;
    let (fork_text, copy_text) = (fork.to_string_lossy().into_owned(), copy.to_string_lossy().into_owned());
    run_git(git, None, &["clone", "--shared", "--no-checkout", "--quiet", "--", &fork_text, &copy_text], stopper, &mut |_| {})?;
    let mut sparse = vec!["sparse-checkout", "set", "--cone"];
    let folders = sparse_folders(fork);
    sparse.extend(folders.iter().map(String::as_str));
    run_git(git, Some(copy), &sparse, stopper, &mut |_| {})?;
    run_git(git, Some(copy), &["checkout", "--quiet", "--detach", &head], stopper, &mut |_| {})?;
    copy_changes(fork, copy).map_err(Failure::Failed)
}

/// Copies the changes not committed in the fork's code into the copy: a changed or new file
/// as it is now, and a deleted or renamed one taken away.
pub fn copy_changes(fork: &Path, copy: &Path) -> Result<(), String> {
    let mut paths = code_paths(fork);
    paths.push("MSBuild".to_owned());
    let mut args = vec!["status", "--porcelain=v1", "-z", "--untracked-files=all", "--"];
    args.extend(paths.iter().map(String::as_str));
    let status = git(fork, &args).ok_or("git couldn't list the uncommitted changes in the code")?;
    for path in listed_paths(&status) {
        let (from, to) = (fork.join(&path), copy.join(&path));
        if from.is_file() {
            if let Some(folder) = to.parent() {
                std::fs::create_dir_all(folder).map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
            }
            std::fs::copy(&from, &to).map_err(|error| format!("couldn't copy {path}: {error}"))?;
        } else if to.is_file() {
            std::fs::remove_file(&to).map_err(|error| format!("couldn't remove {path}: {error}"))?;
        }
    }
    Ok(())
}

/// Every path `git status --porcelain=v1 -z` lists, the path a rename came from too.
pub(crate) fn listed_paths(status: &[u8]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut entries = status.split(|byte| *byte == 0).filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        paths.push(String::from_utf8_lossy(&entry[3..]).into_owned());
        // A rename or copy is followed by the path it came from, whole.
        if matches!(entry[0], b'R' | b'C') {
            if let Some(from) = entries.next() {
                paths.push(String::from_utf8_lossy(from).into_owned());
            }
        }
    }
    paths
}

/// Where the engine is fetched from: the URL the fork's .gitmodules gives, a relative one taken
/// from the fork's own remote.
pub fn engine_url(git: &Git, fork: &Path) -> Option<String> {
    let url = git.output(fork, &["config", "-f", ".gitmodules", "--get", &format!("submodule.{ENGINE}.url")])?;
    if !(url.starts_with("./") || url.starts_with("../")) {
        return Some(url);
    }
    let mut base = git.output(fork, &["remote", "get-url", "origin"])?.trim_end_matches('/').to_owned();
    for part in url.split('/') {
        match part {
            "." | "" => {}
            ".." => base.truncate(base.rfind('/')?),
            part => {
                base.push('/');
                base.push_str(part);
            }
        }
    }
    Some(base)
}

/// Puts the engine at the commit the fork uses into the copy, with its submodules: cloned from
/// the checkout's own engine when it is set up there and has the commit, else fetched.
pub fn fetch_engine(git: &Git, fork: &Path, copy: &Path, stopper: &Stopper, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
    let commit = engine_commit(fork).ok_or_else(|| Failure::Failed(format!("{} doesn't name an engine (RobustToolbox) commit.", fork.display())))?;
    let engine = copy.join(ENGINE);
    if engine.exists() {
        remove_tree(&engine).map_err(Failure::Failed)?;
    }
    let own = fork.join(ENGINE);
    let engine_text = engine.to_string_lossy().into_owned();
    let has_commit = is_git(&own) && super::git(&own, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_some();
    if has_commit {
        run_git(git, None, &["clone", "--shared", "--no-checkout", "--quiet", "--", &own.to_string_lossy(), &engine_text], stopper, report)?;
    } else {
        let url = engine_url(git, fork).ok_or_else(|| Failure::Failed(format!("{}'s .gitmodules has no URL for the engine (RobustToolbox).", fork.display())))?;
        run_git(git, None, &["init", "--quiet", &engine_text], stopper, report)?;
        // Only the commit, where the server allows it, and its whole history where not.
        if run_git(git, Some(&engine), &["fetch", "--progress", "--depth", "1", &url, &commit], stopper, report).is_err() {
            if stopper.stopped() {
                return Err(Failure::Stopped);
            }
            run_git(git, Some(&engine), &["fetch", "--progress", &url], stopper, report)?;
        }
    }
    run_git(git, Some(&engine), &["checkout", "--quiet", "--detach", &commit], stopper, report)?;
    if run_git(git, Some(&engine), &["submodule", "update", "--init", "--recursive", "--depth", "1", "--progress"], stopper, report).is_err() {
        if stopper.stopped() {
            return Err(Failure::Stopped);
        }
        run_git(git, Some(&engine), &["submodule", "update", "--init", "--recursive", "--progress"], stopper, report)?;
    }
    Ok(())
}

/// One ability the editor adds to the engine: a folder of patches/engine (its name numbered, as
/// "0004-rsi-layout-without-images") with a manifest, `unit.txt`, and one patch per shape of
/// engine it was made for. The manifest's lines:
///
/// - `adds: <words>`: what it adds.
/// - `native: yes` or `no`: whether the native engine side needs it. The browser's need only
///   Starlight's engine, and tools/engine-patches.sh applies every unit there.
/// - `has: <file of the engine> <text>`: how to tell an engine has the ability already, its own
///   or from a patch: the file holds the text. With more than one line, any one will do.
/// - `for: <file of the engine>`, if given: the unit is only for engines with that file. One
///   without it has nothing for the unit to do, and it is left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub name: String,
    pub adds: String,
    pub native: bool,
    pub has: Vec<(String, String)>,
    pub only_for: Option<String>,
    /// Its patches, tried in name order.
    pub variants: Vec<PathBuf>,
}

impl Unit {
    /// The unit in a folder, its manifest read and its patches listed.
    pub fn read(folder: &Path) -> Result<Unit, String> {
        let name = file_name(folder);
        let wrong = |why: String| format!("The engine patch unit {name} is broken: {why}");
        let text = std::fs::read_to_string(folder.join(UNIT_MANIFEST)).map_err(|error| wrong(format!("couldn't read its {UNIT_MANIFEST} ({error})")))?;
        let (mut adds, mut native, mut has, mut only_for) = (String::new(), None, Vec::new(), None);
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let Some((key, value)) = line.split_once(':') else { return Err(wrong(format!("a line with no key: {line}"))) };
            let value = value.trim();
            match key.trim() {
                "adds" => adds = value.to_owned(),
                "native" => match value {
                    "yes" => native = Some(true),
                    "no" => native = Some(false),
                    _ => return Err(wrong(format!("native must be yes or no, not {value}"))),
                },
                "has" => {
                    let Some((file, found)) = value.split_once(' ') else { return Err(wrong(format!("a has line with nothing to look for: {line}"))) };
                    has.push((file.to_owned(), found.trim().to_owned()));
                }
                "for" => only_for = Some(value.to_owned()),
                other => return Err(wrong(format!("an unknown key: {other}"))),
            }
        }
        let native = native.ok_or_else(|| wrong("it doesn't say whether native needs it".to_owned()))?;
        if has.is_empty() {
            return Err(wrong("it doesn't say how to tell whether an engine has it".to_owned()));
        }
        let mut variants: Vec<PathBuf> = std::fs::read_dir(folder).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "patch")).collect();
        variants.sort();
        if variants.is_empty() {
            return Err(wrong("it has no patch".to_owned()));
        }
        Ok(Unit { name, adds, native, has, only_for, variants })
    }

    /// Whether an engine has the ability.
    pub fn present(&self, engine: &Path) -> bool {
        self.has.iter().any(|(file, found)| std::fs::read_to_string(engine.join(file)).is_ok_and(|text| text.contains(found.as_str())))
    }

    /// Whether the unit is for an engine: it names no file it is for, or the engine has that file.
    pub fn is_for(&self, engine: &Path) -> bool {
        self.only_for.as_ref().is_none_or(|file| engine.join(file).is_file())
    }
}

/// The units of patches/engine, in name order: each folder with a manifest.
pub fn units(patches: &Path) -> Result<Vec<Unit>, String> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(patches).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.join(UNIT_MANIFEST).is_file()).collect();
    folders.sort();
    if folders.is_empty() {
        return Err(format!("{} has no engine patches.", patches.display()));
    }
    folders.iter().map(|folder| Unit::read(folder)).collect()
}

/// The units the native engine side needs.
pub fn native_units(patches: &Path) -> Result<Vec<Unit>, String> {
    Ok(units(patches)?.into_iter().filter(|unit| unit.native).collect())
}

/// How a unit went on an engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Fit {
    /// The engine had the ability.
    Present,
    /// The unit is not for this engine (its `for` file is not there).
    NotFor,
    /// This variant was applied.
    Variant(String),
}

/// Puts each unit's ability on the engine, in order: a unit the engine has is left out, else
/// the first variant git takes is applied, each checked first. A unit none fits is refused with
/// what git said of each variant. A variant that goes on and leaves the engine without what the
/// manifest looks for is refused too, so that the manifest and its patches never disagree.
pub fn apply_units(engine: &Path, units: &[Unit], name: &str) -> Result<Vec<(String, Fit)>, String> {
    let mut fits = Vec::new();
    for unit in units {
        if !unit.is_for(engine) {
            fits.push((unit.name.clone(), Fit::NotFor));
            continue;
        }
        if unit.present(engine) {
            fits.push((unit.name.clone(), Fit::Present));
            continue;
        }
        let mut said = Vec::new();
        let mut chosen = None;
        for variant in &unit.variants {
            match git_said(engine, &["apply", "--check", "--whitespace=nowarn", &variant.to_string_lossy()]) {
                Ok(()) => {
                    chosen = Some(variant);
                    break;
                }
                Err(why) => said.push(format!("{}: {why}", file_name(variant))),
            }
        }
        let Some(variant) = chosen else {
            let commit = super::git(engine, &["rev-parse", "--short", "HEAD"]).map(|commit| String::from_utf8_lossy(&commit).trim().to_owned()).unwrap_or_default();
            return Err(format!(
                "The editor's engine patch {} ({}) doesn't fit {name}'s engine (RobustToolbox at {commit}), and that engine doesn't already have it. No engine side can be built for it until someone makes a variant of the patch for that engine. What git said about each variant: {}",
                unit.name,
                unit.adds,
                said.join(" | ")
            ));
        };
        git_said(engine, &["apply", "--whitespace=nowarn", &variant.to_string_lossy()]).map_err(|said| format!("The editor's engine patch {} couldn't be applied with {}: {said}", unit.name, file_name(variant)))?;
        if !unit.present(engine) {
            return Err(format!("The editor's engine patch {} applied with {}, but the engine still lacks what its {UNIT_MANIFEST} looks for. Either the manifest or the patch is wrong.", unit.name, file_name(variant)));
        }
        fits.push((unit.name.clone(), Fit::Variant(file_name(variant))));
    }
    Ok(fits)
}

/// How each unit went, for the build's log, as "0002-pre-parsed-prototype-documents with
/// starlight.patch, 0007-simulation-reflection-hook had it".
pub fn fits_text(fits: &[(String, Fit)]) -> String {
    let each: Vec<String> = fits
        .iter()
        .map(|(unit, fit)| match fit {
            Fit::Present => format!("{unit} had it"),
            Fit::NotFor => format!("{unit} not for this engine"),
            Fit::Variant(variant) => format!("{unit} with {variant}"),
        })
        .collect();
    each.join(", ")
}

/// Runs git, giving what it wrote on its error stream when it fails.
fn git_said(folder: &Path, args: &[&str]) -> Result<(), String> {
    let done = quiet(super::git_in(folder).args(args).env("LC_ALL", "C").stdin(Stdio::null())).output().map_err(|error| format!("couldn't start git: {error}"))?;
    if done.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&done.stderr).trim().lines().take(4).collect::<Vec<_>>().join(" ");
    Err(if said.is_empty() { "nothing".to_owned() } else { said })
}

/// Copies the editor's projects into `editor`, with the fork's global.json, so that the
/// fork's SDK builds them.
pub fn copy_sources(sources: &Sources, editor: &Path, fork_copy: &Path) -> Result<(), String> {
    for file in sources.build_files() {
        let to = editor.join(&file);
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(|error| format!("couldn't create {}: {error}", folder.display()))?;
        }
        std::fs::copy(sources.root.join(&file), &to).map_err(|error| format!("couldn't copy {file}: {error}"))?;
    }
    let global = fork_copy.join("global.json");
    if global.is_file() {
        std::fs::copy(&global, editor.join("global.json")).map_err(|error| format!("couldn't copy the fork's global.json: {error}"))?;
    }
    Ok(())
}

/// The SDK version a global.json asks for, if it names one.
pub fn sdk_version(global_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(global_json).ok()?;
    value.get("sdk")?.get("version")?.as_str().map(str::to_owned)
}

/// The global.json given, let to roll forward to a newer major SDK than the one it asks for, if
/// it names one.
pub fn rolled_forward(global_json: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(global_json).ok()?;
    value.get_mut("sdk")?.as_object_mut()?.insert("rollForward".to_owned(), serde_json::Value::from("latestMajor"));
    serde_json::to_string_pretty(&value).ok()
}

/// Whether the SDK the fork's global.json asks for is there. When it is not, a newer one
/// installed builds it (an SDK builds the frameworks of those before it), which is said in the
/// build's log. The fork's copy and its engine are let to roll forward too, as the engine's
/// build runs dotnet again in its own folders. Said plainly when there is none.
fn sdk_check(editor: &Path, fork_copy: &Path, name: &str) -> Result<Option<String>, String> {
    let global = editor.join("global.json");
    let asked = std::fs::read_to_string(&global).ok();
    let wanted = asked.as_deref().and_then(sdk_version);
    let wanted_text = wanted.as_deref().map_or_else(|| "the .NET SDK".to_owned(), |version| format!("the .NET SDK {version}"));
    let version = || quiet(dotnet().arg("--version").current_dir(editor).stdin(Stdio::null())).output();
    let done = match version() {
        Ok(done) => done,
        Err(_) => {
            return Err(format!(
                "Building the engine side for {name} needs {wanted_text}, but .NET isn't installed or the editor can't find dotnet. Install the SDK from https://dotnet.microsoft.com, then open the fork again."
            ))
        }
    };
    if done.status.success() {
        return Ok(None);
    }
    if let Some(rolled) = asked.as_deref().and_then(rolled_forward) {
        if std::fs::write(&global, &rolled).is_ok() {
            if let Ok(newer) = version() {
                if newer.status.success() {
                    for other in [fork_copy.join("global.json"), fork_copy.join(ENGINE).join("global.json")] {
                        if let Some(rolled) = std::fs::read_to_string(&other).ok().as_deref().and_then(rolled_forward) {
                            std::fs::write(&other, rolled).map_err(|error| format!("couldn't write {}: {error}", other.display()))?;
                        }
                    }
                    let used = String::from_utf8_lossy(&newer.stdout).trim().to_owned();
                    return Ok(Some(format!("{wanted_text} isn't installed, so it was built with {used}")));
                }
            }
        }
        if let Some(asked) = &asked {
            let _ = std::fs::write(&global, asked);
        }
    }
    let installed = quiet(dotnet().arg("--list-sdks").stdin(Stdio::null())).output().map(|listed| String::from_utf8_lossy(&listed.stdout).lines().filter_map(|line| line.split(' ').next()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    Err(format!(
        "Building the engine side for {name} needs {wanted_text} (its global.json asks for it), and that isn't installed{}. Install it from https://dotnet.microsoft.com, then open the fork again.",
        if installed.is_empty() { String::new() } else { format!(" (installed: {installed})") }
    ))
}

fn dotnet() -> Command {
    let mut command = Command::new("dotnet");
    command.env("DOTNET_CLI_UI_LANGUAGE", "en").env("DOTNET_NOLOGO", "1").env("DOTNET_CLI_TELEMETRY_OPTOUT", "1").env("MSBUILDDISABLENODEREUSE", "1");
    command
}

/// The runtime the engine side is published for.
pub fn runtime() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "osx-arm64"
    } else if cfg!(target_os = "macos") {
        "osx-x64"
    } else if cfg!(target_os = "linux") {
        "linux-x64"
    } else {
        "win-x64"
    }
}

/// Publishes the engine side against the fork's copy, each line it writes given as it comes.
/// Whether it built.
fn publish(editor: &Path, fork: &Path, output: &Path, stopper: &Stopper, line: &mut dyn FnMut(&str)) -> Result<bool, Failure> {
    let mut fork_root = forward_slashes(fork);
    fork_root.push('/');
    let mut command = dotnet();
    command
        .current_dir(editor)
        .args(["publish", "src/Editor.Native", "-c", "Release", "-r", runtime(), "--self-contained", "false", "-p:PublishReadyToRun=true", "-p:TreatWarningsAsErrors=false"])
        .arg(format!("-p:ForkRoot={fork_root}"))
        .arg("-o")
        .arg(output)
        .args(["-nologo", "-v:m", "-nodeReuse:false", "-clp:NoSummary"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = stopper.spawn(&mut command).map_err(|failure| match failure {
        Failure::Failed(why) => Failure::Failed(format!("couldn't start dotnet: {why}")),
        stopped => stopped,
    })?;
    // The error stream on a thread of its own, so that neither pipe fills while the other is read.
    let errors = child.stderr.take().map(|stderr| std::thread::spawn(move || BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<String>>()));
    if let Some(stdout) = child.stdout.take() {
        for text in BufReader::new(stdout).lines().map_while(Result::ok) {
            line(&text);
        }
    }
    for text in errors.and_then(|errors| errors.join().ok()).unwrap_or_default() {
        line(&text);
    }
    let status = child.wait();
    stopper.ended();
    if stopper.stopped() {
        return Err(Failure::Stopped);
    }
    Ok(status.is_ok_and(|status| status.success()))
}

/// What a failed build says: its first errors, each once, without their paths.
pub fn build_failure(name: &str, lines: &[String], log: &Path) -> String {
    let errors = first_errors(lines, 5);
    let said = if errors.is_empty() { lines.iter().rev().find(|line| !line.trim().is_empty()).cloned().unwrap_or_else(|| "nothing".to_owned()) } else { errors.join(" | ") };
    format!(
        "The engine side couldn't be built for {name}'s code, so the fork can't be opened safely. Its code may use the engine in ways the editor doesn't know about yet. First errors: {said}. The full log is in {}.",
        forward_slashes(log)
    )
}

/// The first errors of a build's output, each once, as "File.cs(12,5): error CS0103: ...".
pub fn first_errors(lines: &[String], most: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in lines {
        let Some(at) = line.find(": error ") else { continue };
        let (place, said) = line.split_at(at);
        let place = place.trim().rsplit(['/', '\\']).next().unwrap_or(place).to_owned();
        let said = said.find(" [").map_or(said, |end| &said[..end]);
        let error = format!("{place}{said}");
        if !found.contains(&error) {
            found.push(error);
            if found.len() == most {
                break;
            }
        }
    }
    found
}

/// Copies a folder with all in it.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let mut pending = vec![from.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let inside = folder.strip_prefix(from).unwrap_or(Path::new(""));
        std::fs::create_dir_all(to.join(inside)).map_err(|error| format!("couldn't create {}: {error}", to.join(inside).display()))?;
        let entries = std::fs::read_dir(&folder).map_err(|error| format!("couldn't read {}: {error}", folder.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let target = to.join(path.strip_prefix(from).unwrap_or(&path));
                std::fs::copy(&path, &target).map_err(|error| format!("couldn't copy {}: {error}", path.display()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/forks/engines.rs"]
mod tests;
