use super::*;
use crate::forks::tests::{commit, folder, run, write};

/// Lets git take repositories from folders, which it refuses for submodules by default.
const FILE: &str = "protocol.file.allow=always";

fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("file://{}{path}", if path.starts_with('/') { "" } else { "/" })
}

fn git() -> Git {
    Git { config: vec![FILE.to_owned()] }
}

fn unit(patches: &Path, name: &str, manifest: &str, variants: &[(&str, &str)]) {
    write(&patches.join(name).join(UNIT_MANIFEST), manifest);
    for (variant, text) in variants {
        write(&patches.join(name).join(variant), text);
    }
}

#[test]
fn units_are_read_in_order_and_the_browser_s_left_out() {
    let patches = folder("engines-units");
    unit(&patches, "0002-b", "adds: b\nnative: yes\nhas: Robust.Shared/B.cs class B\n", &[("wizden.patch", ""), ("starlight.patch", ""), ("notes.txt", "")]);
    unit(&patches, "0001-a", "# for a browser\nadds: a\nnative: no\nhas: A.cs a\nhas: Other/A.cs a a\n", &[("starlight.patch", "")]);
    write(&patches.join("0003-not-a-unit/x.patch"), "");
    let all = units(&patches).unwrap();
    assert_eq!(all.iter().map(|unit| unit.name.as_str()).collect::<Vec<_>>(), ["0001-a", "0002-b"]);
    assert_eq!(all[0].has, [("A.cs".to_owned(), "a".to_owned()), ("Other/A.cs".to_owned(), "a a".to_owned())]);
    assert_eq!(all[1].variants.iter().map(|variant| file_name(variant)).collect::<Vec<_>>(), ["starlight.patch", "wizden.patch"]);
    assert_eq!(native_units(&patches).unwrap().iter().map(|unit| unit.name.as_str()).collect::<Vec<_>>(), ["0002-b"]);
    assert!(units(&folder("engines-units-none")).is_err());

    for (manifest, why) in [
        ("adds: c\nhas: C.cs c\n", "whether native needs it"),
        ("adds: c\nnative: maybe\nhas: C.cs c\n", "native must be yes or no"),
        ("adds: c\nnative: yes\n", "how to tell whether an engine has it"),
        ("adds: c\nnative: yes\nhas: C.cs\n", "a has line with nothing to look for"),
        ("adds: c\nnative: yes\nhas: C.cs c\nsize: big\n", "an unknown key: size"),
    ] {
        let bad = folder("engines-units-bad");
        unit(&bad, "0001-c", manifest, &[("starlight.patch", "")]);
        let said = units(&bad).expect_err(manifest);
        assert!(said.starts_with("The engine patch unit 0001-c is broken: ") && said.contains(why), "{said}");
    }
    let bad = folder("engines-units-no-patch");
    unit(&bad, "0001-c", "adds: c\nnative: yes\nhas: C.cs c\n", &[]);
    assert!(units(&bad).unwrap_err().contains("it has no patch"));
}

#[test]
fn this_repository_s_units_are_whole() {
    let sources = Sources::at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("this repository's sources");
    let native: Vec<String> = native_units(&sources.patches()).unwrap().iter().map(|unit| unit.name[..4].to_owned()).collect();
    assert_eq!(native, ["0002", "0003", "0004", "0007", "0008", "0009", "0010"]);
    assert_eq!(units(&sources.patches()).unwrap().len(), 7, "every unit is one the native engine side needs");
}

#[test]
fn a_unit_for_engines_with_a_file_is_left_out_of_one_without_it() {
    let engine = engine_repo("engines-for", "class B\n{\n}\n");
    let patches = folder("engines-for-patches");
    unit(&patches, "0001-elsewhere", "adds: x\nnative: yes\nfor: Robust.Shared/Variants.cs\nhas: Robust.Shared/Variants.cs lock\n", &[("starlight.patch", &patch_of("class A", "int x;"))]);
    unit(&patches, "0002-here", "adds: y\nnative: yes\nfor: Robust.Shared/Code.cs\nhas: Robust.Shared/Code.cs int y;\n", &[("starlight.patch", &patch_of("class B", "int y;"))]);
    let all = units(&patches).unwrap();
    assert_eq!(all[0].only_for.as_deref(), Some("Robust.Shared/Variants.cs"));
    let fits = apply_units(&engine, &all, "Test").expect("applied");
    assert_eq!(fits, [("0001-elsewhere".to_owned(), Fit::NotFor), ("0002-here".to_owned(), Fit::Variant("starlight.patch".to_owned()))]);
    assert_eq!(fits_text(&fits), "0001-elsewhere not for this engine, 0002-here with starlight.patch");
}

/// An engine in git with one file, and a patch of that file from one text to another.
fn engine_repo(name: &str, code: &str) -> PathBuf {
    let engine = folder(name);
    write(&engine.join("Robust.Shared/Code.cs"), code);
    run(&engine, &["init", "-q"]);
    commit(&engine);
    engine
}

fn patch_of(from: &str, to: &str) -> String {
    format!("diff --git a/Robust.Shared/Code.cs b/Robust.Shared/Code.cs\n--- a/Robust.Shared/Code.cs\n+++ b/Robust.Shared/Code.cs\n@@ -1,3 +1,4 @@\n {from}\n {{\n+    {to}\n }}\n")
}

#[test]
fn each_unit_is_left_out_where_the_engine_has_it_else_its_first_variant_that_fits_goes_on() {
    let engine = engine_repo("engines-apply", "class B\n{\n}\n");
    write(&engine.join("Robust.Shared/Had.cs"), "class Had { int had; }\n");
    let patches = folder("engines-apply-patches");
    // Had already: its patch, which would not fit, is never tried.
    unit(&patches, "0001-had", "adds: had\nnative: yes\nhas: Robust.Shared/Had.cs int had;\n", &[("starlight.patch", &patch_of("class A", "int had;"))]);
    // The first variant is for another shape of engine, the second fits.
    unit(&patches, "0002-added", "adds: added\nnative: yes\nhas: Nowhere.cs x\nhas: Robust.Shared/Code.cs int added;\n", &[("a-starlight.patch", &patch_of("class A", "int added;")), ("b-wizden.patch", &patch_of("class B", "int added;"))]);
    let fits = apply_units(&engine, &units(&patches).unwrap(), "Test").expect("applied");
    assert_eq!(fits, [("0001-had".to_owned(), Fit::Present), ("0002-added".to_owned(), Fit::Variant("b-wizden.patch".to_owned()))]);
    assert_eq!(fits_text(&fits), "0001-had had it, 0002-added with b-wizden.patch");
    assert!(std::fs::read_to_string(engine.join("Robust.Shared/Code.cs")).unwrap().contains("int added;"));

    // Once on, the unit is had: applied again, nothing changes.
    assert_eq!(apply_units(&engine, &units(&patches).unwrap(), "Test").unwrap()[1].1, Fit::Present);
}

#[test]
fn a_unit_no_variant_fits_is_refused_by_name_with_what_git_said_of_each() {
    let engine = engine_repo("engines-refuse", "class C\n{\n}\n");
    let patches = folder("engines-refuse-patches");
    unit(&patches, "0004-layout", "adds: sheets laid out\nnative: yes\nhas: Robust.Shared/Code.cs int x;\n", &[("starlight.patch", &patch_of("class A", "int x;")), ("wizden.patch", &patch_of("class B", "int x;"))]);
    let said = apply_units(&engine, &units(&patches).unwrap(), "Goob").expect_err("refused");
    assert!(said.starts_with("The editor's engine patch 0004-layout (sheets laid out) doesn't fit Goob's engine (RobustToolbox at "), "{said}");
    assert!(said.contains("What git said about each variant: starlight.patch: error: patch failed: Robust.Shared/Code.cs:1") && said.contains(" | wizden.patch: error: patch failed"), "{said}");
    assert_eq!(std::fs::read_to_string(engine.join("Robust.Shared/Code.cs")).unwrap(), "class C\n{\n}\n", "nothing changed");

    // A patch that goes on without what the manifest looks for is a mistake of the unit.
    let wrong = folder("engines-refuse-wrong");
    unit(&wrong, "0005-wrong", "adds: y\nnative: yes\nhas: Robust.Shared/Code.cs int y;\n", &[("starlight.patch", &patch_of("class C", "int z;"))]);
    let said = apply_units(&engine, &units(&wrong).unwrap(), "Goob").expect_err("refused");
    assert!(said.contains("applied with starlight.patch, but the engine still lacks what its unit.txt looks for"), "{said}");
}

/// A small tree of the editor's sources.
fn sources(name: &str) -> Sources {
    let root = folder(name);
    write(&root.join("src/Editor.Native/Editor.Native.csproj"), "<Project />\n");
    write(&root.join("src/Editor.Kernel/Kernel.cs"), "class Kernel {}\n");
    write(&root.join("src/Directory.Build.props"), "<Project />\n");
    write(&root.join("Directory.Packages.props"), "<Project />\n");
    write(&root.join("patches/engine/0002-b.patch"), "diff\n");
    Sources::at(&root).expect("sources")
}

#[test]
fn the_sources_are_found_above_the_program_or_the_engine_side() {
    let sources = sources("engines-find");
    let deep = sources.root.join("src/Editor.Native/bin/r2r");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(Sources::find(&[folder("engines-find-elsewhere"), deep]), Some(sources.clone()));
    assert_eq!(Sources::find(&[folder("engines-find-none")]), None);
    assert!(Sources::at(&sources.root.join("src")).is_none());
}

#[test]
fn the_key_follows_the_code_and_the_editor_s_sources() {
    let sources = sources("engines-key");
    let hash = sources.hash();
    assert_eq!(sources.files(), ["Directory.Packages.props", "patches/engine/0002-b.patch", "src/Directory.Build.props", "src/Editor.Kernel/Kernel.cs", "src/Editor.Native/Editor.Native.csproj"]);

    // Built output and line ends do not count.
    write(&sources.root.join("src/Editor.Native/bin/Release/Editor.Native.dll"), "built");
    write(&sources.root.join("src/Editor.Kernel/obj/project.assets.json"), "{}");
    write(&sources.root.join("src/Editor.Kernel/Kernel.cs"), "class Kernel {}\r\n");
    assert_eq!(sources.hash(), hash);

    // A change of code does.
    write(&sources.root.join("src/Editor.Kernel/Kernel.cs"), "class Kernel { int a; }\n");
    let changed = sources.hash();
    assert_ne!(changed, hash);
    write(&sources.root.join("patches/engine/wizden/0007-g.patch"), "diff\n");
    assert_ne!(sources.hash(), changed);

    let code = Fingerprint { lines: vec![("Content.Shared".into(), "abc".into())] };
    let other = Fingerprint { lines: vec![("Content.Shared".into(), "abd".into())] };
    assert_eq!(key(&code, &hash), key(&code.clone(), &hash));
    assert_ne!(key(&code, &hash), key(&other, &hash));
    assert_ne!(key(&code, &hash), key(&code, &changed));
}

#[test]
fn an_engine_side_in_the_cache_is_known_by_its_key() {
    let cache = Cache::new(PathBuf::from("C:/cache"));
    assert_eq!(key_of(&cache, &cache.engine_build("abc").join("engine")).as_deref(), Some("abc"));
    assert_eq!(key_of(&cache, Path::new("C:/editor/src/Editor.Native/bin/r2r")), None);
}

#[test]
fn renamed_paths_are_listed_with_where_they_came_from() {
    let status = b"R  new.cs\0old.cs\0 M a b.cs\0?? c.cs\0D  gone.cs\0";
    assert_eq!(listed_paths(status), ["new.cs", "old.cs", "a b.cs", "c.cs", "gone.cs"]);
}

#[test]
fn a_failed_build_says_its_first_errors_once_without_their_paths() {
    let lines: Vec<String> = [
        "  Content.Shared -> C:/cache/fork/bin/Content.Shared.dll",
        r"C:\cache\editor\src\Editor.Kernel\Documents.cs(261,14): error CS1061: 'SharedMapSystem' does not contain a definition for 'CreateGridEntity' [C:\cache\editor\src\Editor.Kernel\Editor.Kernel.csproj]",
        r"C:\cache\editor\src\Editor.Kernel\Documents.cs(261,14): error CS1061: 'SharedMapSystem' does not contain a definition for 'CreateGridEntity' [C:\cache\editor\src\Editor.Native\Editor.Native.csproj]",
        "/cache/editor/src/Editor.Kernel/Hosting.cs(60,72): error CS1061: 'IReflectionManager' has no IsAttributeDefined",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(first_errors(&lines, 5), [
        "Documents.cs(261,14): error CS1061: 'SharedMapSystem' does not contain a definition for 'CreateGridEntity'",
        "Hosting.cs(60,72): error CS1061: 'IReflectionManager' has no IsAttributeDefined",
    ]);
    assert_eq!(first_errors(&lines, 1).len(), 1);
    let said = build_failure("Delta-v", &lines, Path::new("C:/cache/engines/k/build.log"));
    assert!(said.starts_with("The engine side couldn't be built for Delta-v's code, so the fork can't be opened safely."), "{said}");
    assert!(said.contains("First errors: Documents.cs(261,14)") && said.ends_with("The full log is in C:/cache/engines/k/build.log."), "{said}");
    assert!(build_failure("A", &["".to_owned(), "it stopped".to_owned()], Path::new("log")).contains("First errors: it stopped."));
}

#[test]
fn the_build_tells_its_stage_and_time() {
    let mut progress = BuildProgress::new();
    assert_eq!(progress.text("Delta-v"), "Building the engine side for Delta-v, 0 s so far: copying Delta-v's code");
    progress.stage = Stage::FetchingEngine;
    let mut git = Progress::new("Fetching");
    git.feed("Receiving objects:  45% (450/1000), 1.20 MiB | 1.00 MiB/s");
    progress.git = Some(git);
    assert_eq!(progress.percent(), Some(45));
    assert!(progress.text("A").ends_with("fetching the engine. Fetching: receiving objects, 45%, 1.20 MiB | 1.00 MiB/s"), "{}", progress.text("A"));
    progress.stage = Stage::Building;
    progress.built = 7;
    progress.started = Instant::now() - Duration::from_secs(150);
    assert_eq!(progress.text("A"), "Building the engine side for A, 2 min so far: building (takes a few minutes). 7 projects built");
    assert_eq!(progress.percent(), None);
}

#[test]
fn the_sdk_is_the_one_the_fork_s_global_json_asks_for() {
    assert_eq!(sdk_version(r#"{ "sdk": { "version": "10.0.100", "rollForward": "latestFeature" } }"#).as_deref(), Some("10.0.100"));
    assert_eq!(sdk_version("{}"), None);
    assert_eq!(sdk_version("not json"), None);
}

#[test]
fn a_missing_sdk_rolls_forward_to_a_newer_major_one() {
    let rolled = rolled_forward(r#"{ "sdk": { "version": "9.0.100", "rollForward": "latestFeature" } }"#).expect("names an SDK");
    let value: serde_json::Value = serde_json::from_str(&rolled).unwrap();
    assert_eq!(value["sdk"]["rollForward"], "latestMajor");
    assert_eq!(sdk_version(&rolled).as_deref(), Some("9.0.100"));
    assert_eq!(rolled_forward("{}"), None);
    assert_eq!(rolled_forward("not json"), None);
}

#[test]
fn old_engine_sides_are_moved_aside_and_deleted() {
    let root = folder("engines-prune");
    let cache = Cache::new(root.clone());
    for key in ["kept", "old1", "old2", "old3", "old4"] {
        write(&cache.engine_build(key).join("engine/Editor.Native.dll"), "built");
        std::thread::sleep(Duration::from_millis(20));
    }
    write(&root.join("engines/stale.old/engine/Editor.Native.dll"), "left by a prune that stopped");
    cache.prune(&["kept".to_owned()]);
    let mut left: Vec<String> = std::fs::read_dir(root.join("engines")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["kept", "old3", "old4"], "the one a fork uses and the two built last");
}

/// A fork on a "server" whose engine is a submodule from another, which lets a commit be
/// fetched by its hash, and a checkout of it made without the engine set up, as Delta-V's is.
struct Setup {
    engine_bare: PathBuf,
    fork_url: String,
    checkout: PathBuf,
    top: PathBuf,
}

fn setup(name: &str) -> Setup {
    let top = folder(name);
    let (engine_bare, fork_bare) = (top.join("engine.git"), top.join("fork.git"));
    for bare in [&engine_bare, &fork_bare] {
        std::fs::create_dir_all(bare).unwrap();
        run(bare, &["init", "-q", "--bare", "-b", "main"]);
    }
    run(&engine_bare, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let engine_work = top.join("engine");
    run(&top, &["clone", "-q", &file_url(&engine_bare), "engine"]);
    write(&engine_work.join("Robust.Shared/Code.cs"), "class A\n{\n}\n");
    write(&engine_work.join("Resources/EnginePrototypes/engine.yml"), "- type: entity\n  id: EngineThing\n");
    commit(&engine_work);
    run(&engine_work, &["push", "-q", "origin", "HEAD:main"]);

    let fork_work = top.join("fork");
    run(&top, &["clone", "-q", &file_url(&fork_bare), "fork"]);
    write(&fork_work.join("Resources/Prototypes/walls.yml"), "- type: entity\n  id: Wall\n");
    write(&fork_work.join("Content.Shared/Content.Shared.csproj"), "<Project />\n");
    write(&fork_work.join("Content.Shared/Thing.cs"), "class Thing {}\n");
    write(&fork_work.join("Content.Server/Content.Server.csproj"), "<Project><ProjectReference Include=\"../Content.Shared/Content.Shared.csproj\" /></Project>\n");
    write(&fork_work.join("Content.Client/Content.Client.csproj"), "<Project />\n");
    write(&fork_work.join("Content.Client/Old.cs"), "class Old {}\n");
    write(&fork_work.join("MSBuild/Content.props"), "<Project />\n");
    write(&fork_work.join("global.json"), "{ \"sdk\": { \"version\": \"10.0.100\" } }\n");
    run(&fork_work, &["-c", FILE, "submodule", "add", "-q", &file_url(&engine_bare), "RobustToolbox"]);
    commit(&fork_work);
    run(&fork_work, &["push", "-q", "origin", "HEAD:main"]);

    run(&top, &["clone", "-q", &file_url(&fork_bare), "checkout"]);
    Setup { engine_bare, fork_url: file_url(&fork_bare), checkout: top.join("checkout"), top }
}

fn status(repo: &Path) -> String {
    String::from_utf8_lossy(&super::super::git(repo, &["status", "--porcelain", "--untracked-files=all"]).unwrap()).into_owned()
}

#[test]
fn the_fork_s_code_is_copied_with_its_changes_and_its_engine_fetched() {
    let setup = setup("engines-copy");
    let checkout = &setup.checkout;
    assert!(!is_git(&checkout.join("RobustToolbox")), "the engine is not set up in the checkout");
    let commit_named = engine_commit(checkout).expect("the commit the fork names");
    assert_eq!(commit_named.len(), 40);
    let prototypes = Fingerprint::of(checkout, Part::Prototypes);
    assert!(prototypes.lines.contains(&("RobustToolbox".to_owned(), commit_named.clone())), "{}", prototypes.text());

    // Changes not committed: in the code, and in the prototypes, which are not code.
    write(&checkout.join("Content.Shared/Thing.cs"), "class Thing { int changed; }\n");
    write(&checkout.join("Content.Server/New.cs"), "class New {}\n");
    std::fs::remove_file(checkout.join("Content.Client/Old.cs")).unwrap();
    write(&checkout.join("Resources/Prototypes/walls.yml"), "changed\n");
    let before = status(checkout);

    let copy = setup.top.join("work/fork");
    copy_fork(&git(), checkout, &copy, &Stopper::default()).expect("copied");
    assert_eq!(std::fs::read_to_string(copy.join("Content.Shared/Thing.cs")).unwrap(), "class Thing { int changed; }\n");
    assert!(copy.join("Content.Server/New.cs").is_file() && !copy.join("Content.Client/Old.cs").exists());
    assert!(copy.join("MSBuild/Content.props").is_file() && copy.join("global.json").is_file());
    assert!(!copy.join("Resources").exists(), "only the code is checked out");

    let mut reports = 0;
    fetch_engine(&git(), checkout, &copy, &Stopper::default(), &mut |_| reports += 1).expect("fetched");
    assert_eq!(engine_commit(&copy).as_deref(), Some(commit_named.as_str()));
    assert!(copy.join("RobustToolbox/Robust.Shared/Code.cs").is_file());
    assert!(copy.join("RobustToolbox/Resources/EnginePrototypes/engine.yml").is_file());

    // The checkout is as it was.
    assert_eq!(status(checkout), before);
    assert!(!is_git(&checkout.join("RobustToolbox")));

    // A unit goes on the fetched engine.
    let engine = copy.join("RobustToolbox");
    let patches = setup.top.join("patches");
    unit(&patches, "0002-fits", "adds: a field\nnative: yes\nhas: Robust.Shared/Code.cs int patched;\n", &[("starlight.patch", &patch_of("class A", "int patched;"))]);
    let fits = apply_units(&engine, &units(&patches).unwrap(), "Test").expect("applied");
    assert_eq!(fits[0].1, Fit::Variant("starlight.patch".to_owned()));

    // The editor's projects go beside it with the fork's global.json.
    let sources = sources("engines-copy-sources");
    let editor = setup.top.join("work/editor");
    copy_sources(&sources, &editor, &copy).unwrap();
    assert!(editor.join("src/Editor.Native/Editor.Native.csproj").is_file() && editor.join("Directory.Packages.props").is_file());
    assert!(!editor.join("patches").exists());
    assert_eq!(sdk_version(&std::fs::read_to_string(editor.join("global.json")).unwrap()).as_deref(), Some("10.0.100"));
}

#[test]
fn an_engine_set_up_in_the_checkout_is_cloned_from_it() {
    let setup = setup("engines-own");
    let checkout = setup.top.join("with-engine");
    run(&setup.top, &["-c", FILE, "clone", "-q", "--recurse-submodules", &setup.fork_url, "with-engine"]);
    assert!(is_git(&checkout.join("RobustToolbox")));
    // The server's engine is gone: only the checkout's own can give it.
    remove_tree(&setup.engine_bare).unwrap();
    let copy = setup.top.join("work-own/fork");
    copy_fork(&git(), &checkout, &copy, &Stopper::default()).expect("copied");
    fetch_engine(&git(), &checkout, &copy, &Stopper::default(), &mut |_| {}).expect("cloned from the checkout");
    assert_eq!(engine_commit(&copy), engine_commit(&checkout));
    assert!(copy.join("RobustToolbox/Robust.Shared/Code.cs").is_file());
}

#[test]
fn a_shallow_checkout_is_copied_too() {
    let setup = setup("engines-shallow");
    run(&setup.top, &["clone", "-q", "--depth", "1", &setup.fork_url, "shallow"]);
    let checkout = setup.top.join("shallow");
    assert!(checkout.join(".git/shallow").is_file());
    let copy = setup.top.join("work-shallow/fork");
    copy_fork(&git(), &checkout, &copy, &Stopper::default()).expect("copied");
    assert!(copy.join("Content.Shared/Thing.cs").is_file());
    fetch_engine(&git(), &checkout, &copy, &Stopper::default(), &mut |_| {}).expect("fetched");
    assert!(copy.join("RobustToolbox/Robust.Shared/Code.cs").is_file());
}

#[test]
fn a_stopped_build_runs_nothing_more() {
    let setup = setup("engines-stop");
    let stopper = Stopper::default();
    stopper.stop();
    let copy = setup.top.join("work-stop/fork");
    assert_eq!(copy_fork(&git(), &setup.checkout, &copy, &stopper), Err(Failure::Stopped));
}

/// Builds the engine side for a fork through the editor's own build, into the editor's cache,
/// where the editor finds it, and prints how long each stage took. The fork is the checkout
/// EDITOR_BUILD_FORK names. With none set, the test says so and passes. An editor's own engine
/// side without a stamp would be taken for any fork, so the build is asked for directly then.
#[test]
#[ignore = "builds an engine side with dotnet, minutes"]
fn a_fork_s_engine_side_is_built() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(fork) = std::env::var_os("EDITOR_BUILD_FORK").map(PathBuf::from) else {
        println!("set EDITOR_BUILD_FORK to a fork's checkout to build its engine side");
        return;
    };
    let home = root.join("src/Editor.Native/bin/r2r");
    let cache = Cache::standard();
    let sources = Sources::at(&root).expect("this repository's sources");
    let job = match choose(&fork, &file_name(&fork), &home, &cache, Some(sources.clone())).expect("can be opened") {
        (Engine::Build(job), _) => *job,
        (Engine::Ready(engine), _) if engine != home => {
            println!("built already: {}", engine.display());
            return;
        }
        (Engine::Ready(_), _) => {
            let code = Fingerprint::of(&fork, Part::Code);
            let key = key(&code, &sources.hash());
            let folder = cache.engine_build(&key);
            let job = BuildJob { fork: fork.clone(), name: file_name(&fork), code, key, folder, sources, reason: "the editor's own engine side has no stamp".to_owned() };
            if is_built(&job.engine(), &job.code) {
                println!("built already: {}", job.engine().display());
                return;
            }
            job
        }
    };
    println!("building into {}: {}", job.folder.display(), job.reason);
    let begun = Instant::now();
    let mut last: Option<(Stage, Instant)> = None;
    let built = build(&Git::default(), &job, &Stopper::default(), &mut |progress| {
        if last.is_none_or(|(stage, _)| stage != progress.stage) {
            if let Some((stage, since)) = last {
                println!("  {stage:?} took {:.1} s", since.elapsed().as_secs_f64());
            }
            last = Some((progress.stage, Instant::now()));
            println!("{}", progress.text(&job.name));
        }
    });
    if let Some((stage, since)) = last {
        println!("  {stage:?} took {:.1} s", since.elapsed().as_secs_f64());
    }
    match built {
        Ok(folder) => println!("built {} in {:.1} s", folder.display(), begun.elapsed().as_secs_f64()),
        Err(failure) => panic!("{}", failure.message()),
    }
    assert!(is_built(&job.engine(), &job.code));
}
