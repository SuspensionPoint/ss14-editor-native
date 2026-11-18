use std::path::PathBuf;
use std::process::Command;

use super::*;
use crate::forks::fingerprint::EngineStamp;

/// A folder of its own under the system's temporary folder, empty.
pub(crate) fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("editor-core-forks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

pub(crate) fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Runs git in a test's own checkout, as a made-up author and with no signing.
pub(crate) fn run(repo: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .expect("git to run");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

pub(crate) fn commit(repo: &Path) {
    run(repo, &["add", "-A"]);
    run(repo, &["commit", "-q", "-m", "test"]);
}

/// A small fork in git: prototypes, a texture, three Content projects that refer to a
/// database project and to the engine, and the engine as a checkout of its own inside it.
pub(crate) fn fork(name: &str) -> PathBuf {
    let fork = folder(name);
    let engine = fork.join("RobustToolbox");
    write(&engine.join("Resources/EnginePrototypes/engine.yml"), "- type: entity\n  id: EngineThing\n");
    write(&engine.join("Robust.Shared/Robust.Shared.csproj"), "<Project />\n");
    write(&engine.join("Robust.Shared/Code.cs"), "class A {}\n");
    run(&engine, &["init", "-q"]);
    commit(&engine);

    write(&fork.join("Resources/Prototypes/walls.yml"), "- type: entity\n  id: Wall\n");
    write(&fork.join("Resources/Textures/Tiles/steel.png"), "not really a png");
    write(&fork.join("Resources/Maps/bagel.yml"), "meta: {}\n");
    write(
        &fork.join("Content.Shared/Content.Shared.csproj"),
        "<Project>\n  <ProjectReference Include=\"..\\Content.Shared.Database\\Content.Shared.Database.csproj\" />\n  <ProjectReference Include=\"..\\RobustToolbox\\Robust.Shared\\Robust.Shared.csproj\" />\n</Project>\n",
    );
    write(&fork.join("Content.Shared/Thing.cs"), "class Thing {}\n");
    write(&fork.join("Content.Shared.Database/Content.Shared.Database.csproj"), "<Project />\n");
    write(&fork.join("Content.Server/Content.Server.csproj"), "<Project><ProjectReference Include=\"../Content.Shared/Content.Shared.csproj\" /></Project>\n");
    write(&fork.join("Content.Client/Content.Client.csproj"), "<Project><ProjectReference Include=\"..\\Content.Shared\\Content.Shared.csproj\" /></Project>\n");
    write(&fork.join("Content.Tests/Content.Tests.csproj"), "<Project />\n");
    write(&fork.join(".gitignore"), "RobustToolbox/\n");
    run(&fork, &["init", "-q", "-b", "mapping"]);
    commit(&fork);
    fork
}

#[test]
fn a_fork_has_prototypes_the_engine_and_content() {
    let fork = fork("looks");
    assert_eq!(not_a_fork(&fork), None);
    let other = folder("looks-other");
    write(&other.join("Resources/Prototypes/a.yml"), "");
    let why = not_a_fork(&other).expect("refused");
    assert!(why.ends_with("doesn't look like a fork checkout. It's missing RobustToolbox, Content.Shared."), "{why}");
    assert!(not_a_fork(&other.join("missing")).is_some());
}

#[test]
fn a_fork_is_named_for_its_folder_and_shows_its_branch() {
    let fork = fork("branch");
    assert_eq!(default_name(&fork), fork.file_name().unwrap().to_string_lossy());
    assert_eq!(branch(&fork).as_deref(), Some("mapping"));
    run(&fork, &["checkout", "-q", "--detach"]);
    assert!(branch(&fork).is_some_and(|branch| branch.starts_with("no branch, at ")));
    assert_eq!(branch(&folder("branch-none")), None);
}

#[test]
fn the_hash_stays_the_same_from_one_build_to_the_next() {
    // FNV-1a's published values for these.
    let mut empty = Fnv::default();
    empty.add(b"");
    assert_eq!(empty.hex(), "cbf29ce484222325");
    let mut a = Fnv::default();
    a.add(b"a");
    assert_eq!(a.hex(), "af63dc4c8601ec8c");
}

/// Prints the fingerprints of the fork this repository pins, with how long each took.
#[test]
#[ignore = "reads upstream/starlight"]
fn the_pinned_fork_s_fingerprints() {
    let fork = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/starlight");
    for part in [fingerprint::Part::Prototypes, fingerprint::Part::Textures, fingerprint::Part::Code] {
        let begun = std::time::Instant::now();
        let print = Fingerprint::of(&fork, part);
        println!("{part:?} in {} ms, key {}:\n{}", begun.elapsed().as_millis(), print.key(), print.text());
    }
    println!("branch {:?}", branch(&fork));
}

/// Checks the pinned fork against the published engine side, as the start screen does, and
/// makes its pack and textures in a cache of its own, as the session does before the engine
/// starts. About a minute, most of it the bake.
#[test]
#[ignore = "reads upstream/starlight and runs the published engine side"]
fn the_pinned_fork_is_made_ready_in_a_cache() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fork = root.join("upstream/starlight");
    let engine = root.join("src/Editor.Native/bin/r2r");
    let cache = Cache::new(folder("pinned-cache"));
    let opening = check(&fork, "Starlight", &engine, (None, None), &cache).expect("opened");
    println!("warning: {:?}", opening.warning);
    let prepared = opening.prepared;
    assert!(prepared.pack_stage.is_some() && prepared.textures_stage.is_some());
    let begun = std::time::Instant::now();
    cache::make_pack(&engine, &fork, &prepared.pack).expect("packed");
    println!("packed in {} ms", begun.elapsed().as_millis());
    let begun = std::time::Instant::now();
    cache::make_textures(&fork, &engine, &prepared.textures).expect("baked");
    println!("baked in {} ms", begun.elapsed().as_millis());
    let textures = crate::textures::BakedTextures::open(&prepared.textures).expect("opened");
    assert!(textures.len() > 10_000 && std::fs::metadata(&prepared.pack).unwrap().len() > 1_000_000);
    let again = check(&fork, "Starlight", &engine, (Some(&prepared.prototypes_key), Some(&prepared.textures_key)), &cache).expect("opened");
    assert_eq!((again.prepared.pack_stage, again.prepared.textures_stage), (None, None), "the second time finds both");
}

#[test]
fn a_fork_whose_code_differs_from_the_engine_side_s_is_refused() {
    let fork = fork("check");
    let engine = folder("check-engine");
    write(&engine.join("Editor.Native.dll"), "the engine");
    let cache = Cache::new(folder("check-cache"));

    // No fingerprint beside the engine side: opened, with a warning.
    let opening = check(&fork, "Test", &engine, (None, None), &cache).expect("opened");
    assert!(matches!(&opening.engine, Engine::Ready(folder) if folder == &engine));
    assert!(opening.warning.is_some_and(|warning| warning.contains("no code fingerprint")));
    assert!(opening.prepared.pack_stage.is_some() && opening.prepared.textures_stage.is_some());

    // Built from this checkout: opened with no warning.
    EngineStamp::write(&engine, &fork).unwrap();
    assert_eq!(check(&fork, "Test", &engine, (None, None), &cache).expect("opened").warning, None);

    // The code changed since: with no sources to build from, refused.
    write(&fork.join("Content.Server/Server.cs"), "class Server {}\n");
    let why = check_with(&fork, "Test", &engine, (None, None), &cache, None).expect_err("refused");
    assert!(why.contains("It differs in the uncommitted changes."), "{why}");
    assert!(why.ends_with("its own sources (src/Editor.Native and patches/engine) aren't next to it."), "{why}");

    // With the editor's sources: an engine side to build, in the cache by its code.
    let sources = engines::Sources::at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("this repository's sources");
    let opening = check_with(&fork, "Test", &engine, (None, None), &cache, Some(sources)).expect("opened with a build");
    let Engine::Build(job) = opening.engine else { panic!("a build") };
    assert_eq!(job.engine(), cache.engine_build(&job.key).join("engine"));
    assert!(job.reason.contains("It differs in the uncommitted changes."));

    // Not a fork at all.
    assert!(check(&folder("check-empty"), "Empty", &engine, (None, None), &cache).is_err());
}
