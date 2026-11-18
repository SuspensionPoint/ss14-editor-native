//! Forks the editor downloads itself: a git clone into a folder of its own under the user data
//! folder, shallow and with its submodules, and Update, which fetches and moves the branch
//! forward. Only forks in that folder are ever fetched, moved or deleted. A mapper's own
//! checkout is only read.
//!
//! Git runs as a child process with `--progress`, and what it writes is read as it comes into
//! a [`Progress`] the start screen shows. A [`Stopper`] ends it from another thread: the
//! process and those it started are killed, and a download stopped part way is removed.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::popular::{popular_by_url, popular_name};
use super::{not_a_fork, quiet, LONG_PATHS};
use crate::files::user_data;

/// The folder downloaded forks go in.
pub fn forks_folder() -> PathBuf {
    user_data().join("forks")
}

/// Whether a folder is one the editor downloaded: inside its forks folder. A fork elsewhere is
/// a mapper's own checkout, never changed, whatever the settings say.
pub fn is_in(root: &Path, folder: &Path) -> bool {
    let tidy = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.trim_end_matches('/').to_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let (root, folder) = (tidy(root), tidy(folder));
    folder.len() > root.len() + 1 && folder.starts_with(&root) && folder.as_bytes()[root.len()] == b'/' && !folder[root.len()..].contains("/..")
}

/// What is wrong with a URL and branch as typed, or None if they can be tried.
pub fn not_a_url(url: &str, branch: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return Some("Type the fork's git URL, or download one of the popular forks.".to_owned());
    }
    if url.starts_with('-') || url.contains(char::is_whitespace) {
        return Some(format!("{url} isn't a git URL."));
    }
    let branch = branch.trim();
    if branch.starts_with('-') || branch.contains(char::is_whitespace) {
        return Some(format!("{branch} isn't a valid branch name."));
    }
    None
}

/// The name a downloaded fork is shown by: a popular fork's own, or the repository's.
pub fn display_name(url: &str) -> String {
    if let Some((fork, at)) = popular_by_url(url) {
        return popular_name(fork, at);
    }
    let url = tidy_url(url);
    segments(&url).last().map_or_else(|| "fork".to_owned(), |name| (*name).to_owned())
}

/// The folder a download goes in, named from its URL: the owner and the repository, and the
/// branch when one is given, as `space-wizards-space-station-14`. Letters, digits, `.`, `_`
/// and `-` only.
pub fn folder_name(url: &str, branch: Option<&str>) -> String {
    let url = tidy_url(url);
    let parts = segments(&url);
    let mut name = parts[parts.len().saturating_sub(2)..].join("-");
    if let Some(branch) = branch.map(str::trim).filter(|branch| !branch.is_empty()) {
        name.push('-');
        name.push_str(branch);
    }
    let mut tidy = String::new();
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' };
        if !(c == '-' && tidy.ends_with('-')) {
            tidy.push(c);
        }
    }
    let tidy = tidy.trim_matches(|c| c == '-' || c == '.');
    if tidy.is_empty() {
        "fork".to_owned()
    } else {
        tidy.to_owned()
    }
}

pub(crate) fn tidy_url(url: &str) -> String {
    let url = url.trim().trim_end_matches(['/', '\\']);
    url.strip_suffix(".git").unwrap_or(url).to_owned()
}

/// The parts of a URL's path: after the host for a web or ssh URL, all of a folder's.
fn segments(url: &str) -> Vec<&str> {
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        // The ssh form, git@host:owner/repo, but not a Windows drive.
        None => match url.split_once(':') {
            Some((host, path)) if host.len() > 1 && !host.contains(['/', '\\']) => path,
            _ => url,
        },
    };
    path.split(['/', '\\']).filter(|part| !part.is_empty() && *part != "." && *part != "..").collect()
}

/// Whether git can be run.
pub fn git_installed() -> bool {
    quiet(Command::new("git").arg("--version")).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

pub const NO_GIT: &str = "Git isn't installed, or the editor can't find it. The editor uses git to download forks and to read your checkouts, so install it from git-scm.com and restart the editor.";

/// Where git is in its work, from the lines it writes with `--progress`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// What the run does, as "Downloading" or "Fetching".
    pub doing: String,
    /// The submodule the lines are about now, None for the fork itself.
    pub part: Option<String>,
    /// What git says it does, as "Receiving objects".
    pub step: String,
    pub percent: Option<u32>,
    /// What git says after the count, such as the size and speed.
    pub detail: Option<String>,
}

/// The steps git reports with a count, which a line must start with to be one.
const STEPS: [&str; 8] = ["Enumerating objects", "Counting objects", "Compressing objects", "Receiving objects", "Resolving deltas", "Updating files", "Checking out files", "Filtering content"];

impl Progress {
    pub fn new(doing: &str) -> Progress {
        Progress { doing: doing.to_owned(), ..Progress::default() }
    }

    /// Reads one line git wrote. True when it was progress and changed what is shown.
    pub fn feed(&mut self, line: &str) -> bool {
        let line = line.trim();
        let line = line.strip_prefix("remote:").map_or(line, str::trim);
        let before = self.clone();
        if let Some(rest) = line.strip_prefix("Submodule '") {
            // Submodule 'RobustToolbox' (url) registered for path 'RobustToolbox'
            let path = rest.rsplit_once("for path '").map(|(_, path)| path.trim_end_matches('\'')).unwrap_or_else(|| rest.split('\'').next().unwrap_or(rest));
            self.part = Some(path.to_owned());
            self.clear_step("Starting");
        } else if let Some(rest) = line.strip_prefix("Fetching submodule ") {
            self.part = Some(rest.trim().to_owned());
            self.clear_step("Fetching");
        } else if line.starts_with("Cloning into ") {
            self.clear_step("Starting");
        } else if let Some((step, rest)) = line.split_once(':').filter(|(step, _)| STEPS.contains(step)) {
            self.step = step.to_owned();
            let rest = rest.trim();
            let rest = rest.strip_suffix(", done.").or_else(|| rest.strip_suffix(", done")).unwrap_or(rest);
            self.percent = rest.split_once('%').and_then(|(number, _)| number.trim().parse().ok());
            self.detail = match self.percent {
                Some(_) => rest.split_once("), ").map(|(_, detail)| detail.trim().to_owned()).filter(|detail| !detail.is_empty()),
                None => None,
            };
        } else {
            return false;
        }
        *self != before
    }

    fn clear_step(&mut self, step: &str) {
        self.step = step.to_owned();
        self.percent = None;
        self.detail = None;
    }

    /// One line for the screen, as "Downloading RobustToolbox: receiving objects, 45%, 1.20 MiB".
    pub fn text(&self) -> String {
        let mut text = self.doing.clone();
        if let Some(part) = &self.part {
            text.push(' ');
            text.push_str(part);
        }
        if !self.step.is_empty() {
            text.push_str(": ");
            let mut step = self.step.chars();
            if let Some(first) = step.next() {
                text.extend(first.to_lowercase());
                text.push_str(step.as_str());
            }
        }
        if let Some(percent) = self.percent {
            text.push_str(&format!(", {percent}%"));
        }
        if let Some(detail) = &self.detail {
            text.push_str(&format!(", {detail}"));
        }
        text
    }
}

/// Ends a run of git from another thread. Its process and every process it started are killed.
#[derive(Clone, Default)]
pub struct Stopper {
    stopped: Arc<AtomicBool>,
    /// The process running now, held while one is started so a stop cannot fall between.
    running: Arc<Mutex<Option<u32>>>,
}

impl Stopper {
    pub fn stop(&self) {
        let running = self.running.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(id) = *running {
            kill_tree(id);
        }
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Starts a process that a stop kills, with those it starts. Refused once stopped.
    pub(crate) fn spawn(&self, command: &mut Command) -> Result<std::process::Child, Failure> {
        let mut running = self.running.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.stopped() {
            return Err(Failure::Stopped);
        }
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(command, 0);
        let child = quiet(command).spawn().map_err(|error| Failure::Failed(format!("{error}")))?;
        *running = Some(child.id());
        Ok(child)
    }

    /// The process [`Self::spawn`] started has ended.
    pub(crate) fn ended(&self) {
        *self.running.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

/// Kills a process with those it started. Git starts others for the transfer and for each
/// submodule, which would go on writing if only git itself were killed.
fn kill_tree(id: u32) {
    #[cfg(windows)]
    let _ = quiet(Command::new("taskkill").args(["/T", "/F", "/PID", &id.to_string()])).stdout(Stdio::null()).stderr(Stdio::null()).status();
    // Git was started as the head of a process group of its own.
    #[cfg(not(windows))]
    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{id}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Why a run did not end well.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// Cancel was pressed.
    Stopped,
    /// What went wrong, to show as it is.
    Failed(String),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Stopped => "Stopped.".to_owned(),
            Failure::Failed(why) => why.clone(),
        }
    }
}

/// A run of git, with settings given to every git it starts (as `-c name=value`).
#[derive(Clone, Debug, Default)]
pub struct Git {
    pub config: Vec<String>,
}

impl Git {
    /// Runs git in a folder, reading its progress as it comes. Git's own last words when it fails.
    pub(crate) fn run(&self, folder: Option<&Path>, args: &[OsString], stopper: &Stopper, progress: &mut Progress, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
        let mut command = Command::new("git");
        if let Some(folder) = folder {
            command.arg("-C").arg(folder);
        }
        for setting in self.settings() {
            command.arg("-c").arg(setting);
        }
        command.args(args);
        // Git's words in English, which `Progress` reads, and no question for a password in a
        // terminal there is not.
        command.env("LC_ALL", "C").env("LANGUAGE", "C").env("GIT_TERMINAL_PROMPT", "0");
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = {
            let mut running = self.running(stopper);
            if stopper.stopped() {
                return Err(Failure::Stopped);
            }
            let child = quiet(&mut command).spawn().map_err(|error| Failure::Failed(format!("Couldn't start git: {error}")))?;
            *running = Some(child.id());
            child
        };
        let mut said: Vec<String> = Vec::new();
        if let Some(mut stderr) = child.stderr.take() {
            let mut buffer = [0u8; 4096];
            let mut line: Vec<u8> = Vec::new();
            loop {
                let read = match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                for byte in &buffer[..read] {
                    if *byte == b'\r' || *byte == b'\n' {
                        take_line(&mut line, progress, &mut said, report);
                    } else {
                        line.push(*byte);
                    }
                }
            }
            take_line(&mut line, progress, &mut said, report);
        }
        let status = child.wait();
        *self.running(stopper) = None;
        if stopper.stopped() {
            return Err(Failure::Stopped);
        }
        match status {
            Ok(status) if status.success() => Ok(()),
            _ => Err(Failure::Failed(said_last(&said))),
        }
    }

    /// What each git run is given with `-c`: long paths on Windows (`forks::LONG_PATHS`), then
    /// this one's own. Git hands `-c` on to the gits it starts itself, so the engine's
    /// submodule gets it too.
    fn settings(&self) -> impl Iterator<Item = &str> {
        let long_paths = cfg!(windows).then_some(LONG_PATHS);
        long_paths.into_iter().chain(self.config.iter().map(String::as_str))
    }

    fn running<'a>(&self, stopper: &'a Stopper) -> std::sync::MutexGuard<'a, Option<u32>> {
        stopper.running.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs git in a folder and gives what it wrote, or None when it failed.
    pub(crate) fn output(&self, folder: &Path, args: &[&str]) -> Option<String> {
        let mut command = Command::new("git");
        command.arg("-C").arg(folder);
        for setting in self.settings() {
            command.arg("-c").arg(setting);
        }
        let output = quiet(command.args(args).env("LC_ALL", "C").env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null())).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

fn take_line(line: &mut Vec<u8>, progress: &mut Progress, said: &mut Vec<String>, report: &mut dyn FnMut(&Progress)) {
    let text = String::from_utf8_lossy(line).trim().to_owned();
    line.clear();
    if text.is_empty() {
        return;
    }
    if progress.feed(&text) {
        report(progress);
    } else if !text.contains('%') {
        said.push(text);
        if said.len() > 20 {
            said.remove(0);
        }
    }
}

/// Git's last error, or its last line when it gave none.
fn said_last(said: &[String]) -> String {
    let error = said.iter().rev().find(|line| line.starts_with("fatal:") || line.starts_with("error:")).or_else(|| said.last());
    match error {
        Some(line) => format!("Git said: {line}"),
        None => "Git stopped with an error but gave no message.".to_owned(),
    }
}

/// A fork to download: from where, which branch (None for the default), into which folder of
/// the forks folder `root`.
#[derive(Clone, Debug)]
pub struct Download {
    pub url: String,
    pub branch: Option<String>,
    pub root: PathBuf,
    pub name: String,
}

impl Download {
    /// A download into the folder named from its URL and branch, or the first of that name
    /// with `-2`, `-3` and on after it that is free, when a download removed from the list but
    /// kept on the disk has it.
    pub fn new(root: PathBuf, url: &str, branch: &str) -> Download {
        let branch = Some(branch.trim().to_owned()).filter(|branch| !branch.is_empty());
        let base = folder_name(url, branch.as_deref());
        let name = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|name| !root.join(name).exists()).unwrap_or(base);
        Download { url: url.trim().to_owned(), branch, root, name }
    }

    /// The folder a download of this URL and branch goes in first, to tell whether a known
    /// fork is it already.
    pub fn first_folder(root: &Path, url: &str, branch: &str) -> PathBuf {
        root.join(folder_name(url, Some(branch)))
    }

    pub fn folder(&self) -> PathBuf {
        self.root.join(&self.name)
    }

    /// Where it is cloned to, and moved from to its folder once whole and checked.
    pub fn partial(&self) -> PathBuf {
        self.root.join(format!("{}.partial", self.name))
    }
}

/// Clones a fork with its submodules, shallow, beside its folder, and moves it there once it is
/// whole and checked to be a fork. What a download stopped before left goes first. A download
/// stopped, failed or not a fork is removed.
pub fn download(git: &Git, job: &Download, stopper: &Stopper, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
    if !git_installed() {
        return Err(Failure::Failed(NO_GIT.to_owned()));
    }
    let (partial, folder) = (job.partial(), job.folder());
    if folder.exists() {
        return Err(Failure::Failed(format!("{} already exists.", folder.display())));
    }
    if partial.exists() {
        remove_tree(&partial).map_err(Failure::Failed)?;
    }
    std::fs::create_dir_all(&job.root).map_err(|error| Failure::Failed(format!("couldn't create {}: {error}", job.root.display())))?;
    let mut args: Vec<OsString> = ["clone", "--progress", "--recurse-submodules", "--shallow-submodules", "--depth", "1"].map(OsString::from).to_vec();
    // Kept in the clone's own settings too, for git run there by hand later.
    if cfg!(windows) {
        args.extend([OsString::from("--config"), OsString::from(LONG_PATHS)]);
    }
    if let Some(branch) = &job.branch {
        args.extend([OsString::from("--branch"), OsString::from(branch)]);
    }
    args.extend([OsString::from("--"), OsString::from(&job.url), partial.clone().into_os_string()]);
    let mut progress = Progress::new("Downloading");
    let cloned = git
        .run(None, &args, stopper, &mut progress, report)
        .and_then(|()| match not_a_fork(&partial) {
            None => Ok(()),
            Some(_) => Err(Failure::Failed(format!("{} downloaded, but it isn't a fork because it has no {}. Its files were deleted.", job.url, super::FORK_FOLDERS.join(", ")))),
        })
        .and_then(|()| move_folder(&partial, &folder).map_err(Failure::Failed));
    if cloned.is_err() {
        // A process git started may hold a file for a moment after it was killed.
        let _ = remove_tree(&partial);
    }
    cloned
}

/// Renames a folder, tried a few times, since a process git started or a virus scanner may
/// hold a file in it for a moment. The submodules' links are relative, so they still hold.
fn move_folder(from: &Path, to: &Path) -> Result<(), String> {
    let mut last = None;
    for _ in 0..10 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => last = Some(error),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Err(format!("couldn't move the download to {}: {}", to.display(), last.map_or_else(String::new, |error| error.to_string())))
}

/// Brings a downloaded fork up to date: fetches its branch and moves it forward, and its
/// submodules to the commits it names. Refused when the branch has commits of its own, or
/// when changes in its files would be overwritten. What changed, in words.
pub fn update(git: &Git, folder: &Path, stopper: &Stopper, report: &mut dyn FnMut(&Progress)) -> Result<String, Failure> {
    if !git_installed() {
        return Err(Failure::Failed(NO_GIT.to_owned()));
    }
    let failed = |why: &str| Failure::Failed(why.to_owned());
    let branch = git.output(folder, &["rev-parse", "--abbrev-ref", "HEAD"]).ok_or_else(|| failed("This download isn't a git checkout anymore."))?;
    if branch == "HEAD" {
        return Err(failed("This download isn't on a branch, so there's nothing to update it from."));
    }
    let upstream = git.output(folder, &["rev-parse", "--abbrev-ref", "@{upstream}"]).ok_or_else(|| Failure::Failed(format!("The branch {branch} has no upstream branch to update from.")))?;
    let before = git.output(folder, &["rev-parse", "HEAD"]).unwrap_or_default();
    let mut progress = Progress::new("Fetching");
    git.run(Some(folder), &["fetch", "--progress"].map(OsString::from), stopper, &mut progress, report)?;
    let after = git.output(folder, &["rev-parse", "@{upstream}"]).unwrap_or_default();
    if after != before {
        if git.output(folder, &["merge-base", "--is-ancestor", "HEAD", "@{upstream}"]).is_none() {
            return Err(Failure::Failed(format!("{branch} has its own commits that {upstream} doesn't have, so it can't simply be moved forward. Nothing was changed.")));
        }
        let mut progress = Progress::new("Updating");
        git.run(Some(folder), &["merge", "--ff-only", "--no-stat", "@{upstream}"].map(OsString::from), stopper, &mut progress, report).map_err(|failure| match failure {
            Failure::Failed(why) => Failure::Failed(format!("{branch} couldn't be moved forward, so nothing was changed. Local changes to its files may be in the way. {why}")),
            stopped => stopped,
        })?;
    }
    let mut progress = Progress::new("Updating");
    progress.part = Some("the submodules".to_owned());
    git.run(Some(folder), &["submodule", "update", "--init", "--recursive", "--depth", "1", "--progress"].map(OsString::from), stopper, &mut progress, report)?;
    let short = |commit: &str| commit.chars().take(9).collect::<String>();
    Ok(if after == before { format!("{branch} was already up to date.") } else { format!("Updated {branch} from {} to {}.", short(&before), short(&after)) })
}

/// Deletes a folder with everything in it. Git makes some of its files read only, and a
/// process it started may hold one for a moment after it ends, so it is tried a few times.
pub fn remove_tree(folder: &Path) -> Result<(), String> {
    let mut last = None;
    for attempt in 0..10 {
        match std::fs::remove_dir_all(folder) {
            Ok(()) => return Ok(()),
            Err(_) if !folder.exists() => return Ok(()),
            Err(error) => last = Some(error),
        }
        if attempt == 0 {
            make_writable(folder);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(format!("couldn't delete {}: {}", folder.display(), last.map_or_else(String::new, |error| error.to_string())))
}

#[cfg(windows)]
fn make_writable(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            make_writable(&entry.path());
        } else if let Ok(meta) = entry.metadata() {
            let mut permissions = meta.permissions();
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(entry.path(), permissions);
            }
        }
    }
}

#[cfg(not(windows))]
fn make_writable(_folder: &Path) {}

#[cfg(test)]
#[path = "../tests/forks/download.rs"]
mod tests;
