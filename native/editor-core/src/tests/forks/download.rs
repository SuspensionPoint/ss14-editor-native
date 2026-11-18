use super::*;
use crate::forks::tests::{commit, folder, run, write};

#[test]
fn git_is_given_long_paths_on_windows_first() {
    // Goob Station's deepest texture is 196 characters, past 260 under the editor's folder.
    let git = Git { config: vec!["protocol.file.allow=always".into()] };
    let settings: Vec<&str> = git.settings().collect();
    if cfg!(windows) {
        assert_eq!(settings, ["core.longpaths=true", "protocol.file.allow=always"]);
    } else {
        assert_eq!(settings, ["protocol.file.allow=always"]);
    }
}

#[test]
fn a_download_s_folder_is_named_from_its_url() {
    assert_eq!(folder_name("https://github.com/space-wizards/space-station-14", None), "space-wizards-space-station-14");
    assert_eq!(folder_name("https://github.com/ss14Starlight/space-station-14.git", None), "ss14Starlight-space-station-14");
    assert_eq!(folder_name("https://github.com/ss14Starlight/space-station-14/", Some("")), "ss14Starlight-space-station-14");
    assert_eq!(folder_name("git@github.com:owner/repo.git", None), "owner-repo");
    assert_eq!(folder_name("https://github.com/owner/repo", Some("feature/new walls")), "owner-repo-feature-new-walls");
    assert_eq!(folder_name("file:///C:/temp/tests/fork.git", None), "tests-fork");
    assert_eq!(folder_name("C:\\temp\\fork", None), "temp-fork");
    assert_eq!(folder_name("https://example.com/", None), "fork");
    assert_eq!(folder_name("https://example.com/../..", None), "fork");
}

#[test]
fn a_download_is_named_for_a_known_fork_or_its_repository() {
    assert_eq!(display_name("https://github.com/ss14Starlight/space-station-14.git"), "Starlight");
    assert_eq!(display_name("https://github.com/space-wizards/space-station-14/"), "Space Wizards");
    assert_eq!(display_name("https://github.com/someone/their-station"), "their-station");
    // No longer listed: named for its repository as any other.
    assert_eq!(display_name("https://github.com/funky-station/funky-station"), "funky-station");
    assert_eq!(display_name("https://github.com/funky-station/forky-station"), "Funky Station");
}

#[test]
fn urls_and_branches_are_checked_before_git_sees_them() {
    assert!(not_a_url("", "").is_some());
    assert!(not_a_url("--upload-pack=evil", "").is_some());
    assert!(not_a_url("https://github.com/a/b", "-x").is_some());
    assert!(not_a_url("https://github.com/a/b", "two words").is_some());
    assert_eq!(not_a_url(" https://github.com/a/b ", "main"), None);
}

#[test]
fn only_folders_inside_the_forks_folder_are_the_editor_s() {
    let root = Path::new("C:/data/ss14-editor/forks");
    assert!(is_in(root, Path::new("C:/data/ss14-editor/forks/starlight")));
    assert!(is_in(root, Path::new("C:\\data\\ss14-editor\\forks\\starlight\\")));
    assert!(!is_in(root, root));
    assert!(!is_in(root, Path::new("C:/data/ss14-editor/forks-other/starlight")));
    assert!(!is_in(root, Path::new("C:/data/ss14-editor/forks/../mine")));
    assert!(!is_in(root, Path::new("C:/projects/starlight")));
}

#[test]
fn git_s_progress_is_read_line_by_line() {
    let mut progress = Progress::new("Downloading");
    assert!(progress.feed("Cloning into 'C:/data/forks/starlight'..."));
    assert_eq!(progress.text(), "Downloading: starting");
    assert!(progress.feed("remote: Enumerating objects: 15433, done."));
    assert_eq!((progress.step.as_str(), progress.percent), ("Enumerating objects", None));
    assert!(progress.feed("remote: Compressing objects:  12% (1500/12000)"));
    assert_eq!(progress.percent, Some(12));
    assert!(progress.feed("Receiving objects:  45% (450/1000), 1.20 MiB | 1.00 MiB/s"));
    assert_eq!(progress.text(), "Downloading: receiving objects, 45%, 1.20 MiB | 1.00 MiB/s");
    assert!(!progress.feed("Receiving objects:  45% (450/1000), 1.20 MiB | 1.00 MiB/s"), "the same again changes nothing");
    assert!(progress.feed("Resolving deltas: 100% (5/5), done."));
    assert_eq!((progress.percent, progress.detail.clone()), (Some(100), None));
    assert!(progress.feed("Submodule 'RobustToolbox' (https://github.com/space-wizards/RobustToolbox.git) registered for path 'RobustToolbox'"));
    assert_eq!(progress.part.as_deref(), Some("RobustToolbox"));
    assert!(progress.feed("Updating files:  30% (300/1000)"));
    assert_eq!(progress.text(), "Downloading RobustToolbox: updating files, 30%");
    assert!(!progress.feed("fatal: repository 'https://example.com/x' not found"));
    assert!(!progress.feed("warning: --depth is ignored in local clones"));
    assert!(!progress.feed(""));
    assert!(progress.feed("Fetching submodule RobustToolbox"));
    assert_eq!(progress.text(), "Downloading RobustToolbox: fetching");
}

#[test]
fn git_s_last_error_is_what_a_failure_says() {
    let said = ["Cloning into 'x'...".to_owned(), "fatal: repository 'x' not found".to_owned(), "something after".to_owned()];
    assert_eq!(said_last(&said), "Git said: fatal: repository 'x' not found");
    assert_eq!(said_last(&said[..1]), "Git said: Cloning into 'x'...");
    assert!(said_last(&[]).starts_with("Git stopped"));
}

/// A URL git reads a folder by as it would a server, so that the clone is shallow as a real one.
fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("file://{}{path}", if path.starts_with('/') { "" } else { "/" })
}

/// Lets git take submodules from folders, which it refuses by default.
const FILE: &str = "protocol.file.allow=always";

/// A fork on a "server": a bare repository with the engine as a submodule from another, and a
/// working copy of each to push changes from.
struct Server {
    fork_url: String,
    engine_work: PathBuf,
    fork_work: PathBuf,
}

fn server(name: &str) -> Server {
    let top = folder(name);
    let (engine_bare, fork_bare) = (top.join("engine.git"), top.join("fork.git"));
    for bare in [&engine_bare, &fork_bare] {
        std::fs::create_dir_all(bare).unwrap();
        run(bare, &["init", "-q", "--bare", "-b", "main"]);
    }
    let engine_work = top.join("engine");
    run(&top, &["clone", "-q", &file_url(&engine_bare), "engine"]);
    write(&engine_work.join("Robust.Shared/Code.cs"), "class A {}\n");
    commit(&engine_work);
    run(&engine_work, &["push", "-q", "origin", "HEAD:main"]);

    let fork_work = top.join("fork");
    run(&top, &["clone", "-q", &file_url(&fork_bare), "fork"]);
    write(&fork_work.join("Resources/Prototypes/walls.yml"), "- type: entity\n  id: Wall\n");
    write(&fork_work.join("Content.Shared/Thing.cs"), "class Thing {}\n");
    run(&fork_work, &["-c", FILE, "submodule", "add", "-q", &file_url(&engine_bare), "RobustToolbox"]);
    commit(&fork_work);
    run(&fork_work, &["push", "-q", "origin", "HEAD:main"]);
    Server { fork_url: file_url(&fork_bare), engine_work, fork_work }
}

/// A change on the server: a new prototype in the fork, and the engine moved on with it.
fn push_change(server: &Server, file: &str) {
    write(&server.engine_work.join(format!("Robust.Shared/{file}.cs")), "class B {}\n");
    commit(&server.engine_work);
    run(&server.engine_work, &["push", "-q", "origin", "HEAD:main"]);
    let engine_in_fork = server.fork_work.join("RobustToolbox");
    run(&engine_in_fork, &["-c", FILE, "pull", "-q", "origin", "main"]);
    write(&server.fork_work.join(format!("Resources/Prototypes/{file}.yml")), "- type: entity\n  id: New\n");
    commit(&server.fork_work);
    run(&server.fork_work, &["push", "-q", "origin", "HEAD:main"]);
}

fn git() -> Git {
    Git { config: vec![FILE.to_owned()] }
}

#[test]
fn a_fork_is_downloaded_with_its_engine_and_brought_up_to_date() {
    let server = server("dl-server");
    let root = folder("dl-root");
    let job = Download::new(root.clone(), &server.fork_url, "");
    let fork = job.folder();
    let mut reports = Vec::new();
    download(&git(), &job, &Stopper::default(), &mut |progress| reports.push(progress.text())).expect("downloaded");
    assert_eq!(not_a_fork(&fork), None);
    assert!(fork.join("RobustToolbox/Robust.Shared/Code.cs").is_file(), "the submodule came with it");
    assert!(fork.join(".git/shallow").is_file(), "the fork is shallow");
    assert!(reports.iter().any(|report| report.starts_with("Downloading")), "{reports:?}");
    assert!(is_in(&root, &fork));

    let said = update(&git(), &fork, &Stopper::default(), &mut |_| {}).expect("up to date");
    assert_eq!(said, "main was already up to date.");

    push_change(&server, "second");
    let said = update(&git(), &fork, &Stopper::default(), &mut |_| {}).expect("updated");
    assert!(said.starts_with("Updated main from "), "{said}");
    assert!(fork.join("Resources/Prototypes/second.yml").is_file());
    assert!(fork.join("RobustToolbox/Robust.Shared/second.cs").is_file(), "the submodule moved with it");

    // A commit of its own: it cannot move forward, and nothing changes.
    write(&fork.join("Resources/Prototypes/mine.yml"), "- type: entity\n  id: Mine\n");
    commit(&fork);
    push_change(&server, "third");
    let refused = update(&git(), &fork, &Stopper::default(), &mut |_| {}).expect_err("refused");
    assert!(refused.message().contains("has its own commits"), "{}", refused.message());
    assert!(!fork.join("Resources/Prototypes/third.yml").exists());
}

#[test]
fn a_branch_is_downloaded_into_a_folder_of_its_own() {
    let server = server("dl-branch-server");
    run(&server.fork_work, &["push", "-q", "origin", "HEAD:mapping"]);
    let job = Download::new(folder("dl-branch-root"), &server.fork_url, "mapping");
    assert!(job.name.ends_with("-mapping"), "{}", job.name);
    download(&git(), &job, &Stopper::default(), &mut |_| {}).expect("downloaded");
    assert_eq!(crate::forks::branch(&job.folder()).as_deref(), Some("mapping"));

    let missing = Download::new(folder("dl-missing-root"), &server.fork_url, "no-such-branch");
    let failed = download(&git(), &missing, &Stopper::default(), &mut |_| {}).expect_err("no such branch");
    assert!(failed.message().starts_with("Git said: "), "{}", failed.message());
    assert!(!missing.folder().exists(), "the failed download was removed");
}

#[test]
fn a_stopped_or_wrong_download_leaves_nothing() {
    let server = server("dl-stop-server");
    let root = folder("dl-stop-root");
    let job = Download::new(root.clone(), &server.fork_url, "");
    // What a download stopped before left is cleared first.
    write(&job.partial().join("half/done.txt"), "left over");
    let stopper = Stopper::default();
    stopper.stop();
    assert_eq!(download(&git(), &job, &stopper, &mut |_| {}), Err(Failure::Stopped));
    assert!(!job.folder().exists() && !job.partial().exists());

    // Stopped while git runs: at its first report, git and those it started are killed.
    let stopper = Stopper::default();
    let stop = stopper.clone();
    assert_eq!(download(&git(), &job, &stopper, &mut |_| stop.stop()), Err(Failure::Stopped));
    assert!(!job.folder().exists() && !job.partial().exists());

    let engine = Download::new(root, &file_url(&server.engine_work.with_file_name("engine.git")), "");
    let refused = download(&git(), &engine, &Stopper::default(), &mut |_| {}).expect_err("not a fork");
    assert!(refused.message().contains("isn't a fork"), "{}", refused.message());
    assert!(!engine.folder().exists() && !engine.partial().exists());
}

#[test]
fn a_download_kept_on_the_disk_is_not_overwritten() {
    let root = folder("dl-kept-root");
    let url = "https://github.com/owner/repo";
    assert_eq!(Download::new(root.clone(), url, "").name, "owner-repo");
    write(&root.join("owner-repo/kept.txt"), "kept");
    write(&root.join("owner-repo-2/kept.txt"), "kept");
    let job = Download::new(root.clone(), url, " ");
    assert_eq!(job.name, "owner-repo-3");
    assert_eq!(Download::first_folder(&root, url, ""), root.join("owner-repo"));
}
