use super::*;
use crate::forks::tests::{commit, folder, fork, write};

fn value<'a>(print: &'a Fingerprint, name: &str) -> &'a str {
    print.lines.iter().find(|(have, _)| have == name).map(|(_, value)| value.as_str()).unwrap_or_else(|| panic!("no line {name} in {print:?}"))
}

#[test]
fn the_content_projects_are_followed_from_the_three_the_engine_side_is_built_against() {
    let fork = fork("projects");
    assert_eq!(content_projects(&fork), ["Content.Client", "Content.Server", "Content.Shared", "Content.Shared.Database"]);
}

#[test]
fn a_fork_s_modules_are_followed_too() {
    let fork = fork("projects-modules");
    write(&fork.join("Content.Goob.Server/Content.Goob.Server.csproj"), "<Project><ProjectReference Include=\"..\\Content.Goob.Common\\Content.Goob.Common.csproj\" /></Project>\n");
    write(&fork.join("Content.Goob.Common/Content.Goob.Common.csproj"), "<Project />\n");
    write(&fork.join("Content.Goob.Client/Content.Goob.Client.csproj"), "<Project />\n");
    // Neither a module nor referred to.
    write(&fork.join("Content.Server.Tools/Content.Server.Tools.csproj"), "<Project />\n");
    write(&fork.join("Content.Other.Server/Readme.txt"), "no project\n");
    assert_eq!(content_projects(&fork), ["Content.Client", "Content.Goob.Client", "Content.Goob.Common", "Content.Goob.Server", "Content.Server", "Content.Shared", "Content.Shared.Database"]);
}

#[test]
fn a_project_s_references_are_read_as_written() {
    let text = "<ProjectReference Include=\"..\\A\\A.csproj\" />\n<ProjectReference Include=\"../B/B.csproj\" OutputItemType=\"Analyzer\" />";
    assert_eq!(references(text), ["..\\A\\A.csproj", "../B/B.csproj"]);
}

#[test]
fn prototypes_change_with_their_files_and_not_with_the_textures() {
    let fork = fork("prototypes");
    let first = Fingerprint::of(&fork, Part::Prototypes);
    assert!(first.is_git());
    assert_eq!(value(&first, "changes"), "none");
    assert_eq!(value(&first, "Resources/migration.yml"), "-");
    assert_ne!(value(&first, "Resources/Prototypes"), "-");
    assert_ne!(value(&first, "RobustToolbox/Resources/EnginePrototypes"), "-");
    assert_eq!(Fingerprint::of(&fork, Part::Prototypes), first, "the same checkout gives the same fingerprint");

    // A texture changed leaves the prototypes as they were.
    write(&fork.join("Resources/Textures/Tiles/steel.png"), "another picture");
    assert_eq!(Fingerprint::of(&fork, Part::Prototypes), first);
    assert_ne!(value(&Fingerprint::of(&fork, Part::Textures), "changes"), "none");

    // A prototype changed and not committed, then changed again.
    write(&fork.join("Resources/Prototypes/walls.yml"), "- type: entity\n  id: Wall2\n");
    let edited = Fingerprint::of(&fork, Part::Prototypes);
    assert_ne!(edited.key(), first.key());
    assert_eq!(edited.differences(&first), ["changes"]);
    write(&fork.join("Resources/Prototypes/walls.yml"), "- type: entity\n  id: Wall3\n");
    assert_ne!(Fingerprint::of(&fork, Part::Prototypes).key(), edited.key(), "a second edit of a changed file counts");

    // A new file not yet added counts too.
    write(&fork.join("Resources/Prototypes/new.yml"), "- type: entity\n  id: New\n");
    let added = Fingerprint::of(&fork, Part::Prototypes);
    commit(&fork);
    let committed = Fingerprint::of(&fork, Part::Prototypes);
    assert_eq!(value(&committed, "changes"), "none");
    assert_eq!(committed.differences(&first), ["Resources/Prototypes"]);
    assert_ne!(committed.key(), added.key());
}

#[test]
fn the_code_counts_the_engine_s_commit_and_not_its_working_tree() {
    let fork = fork("code");
    let first = Fingerprint::of(&fork, Part::Code);
    let names: Vec<&str> = first.lines.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["Content.Client", "Content.Server", "Content.Shared", "Content.Shared.Database", "Directory.Packages.props", "Directory.Build.props", "global.json", "changes", "RobustToolbox"]);

    // The engine patched in its working tree, as the editor's build checkout has it.
    let engine = fork.join("RobustToolbox");
    write(&engine.join("Robust.Shared/Code.cs"), "class A { int patched; }\n");
    assert_eq!(Fingerprint::of(&fork, Part::Code), first);

    // A test project changed is not the engine side's code.
    write(&fork.join("Content.Tests/Content.Tests.csproj"), "<Project><!-- changed --></Project>\n");
    assert_eq!(Fingerprint::of(&fork, Part::Code), first);

    // Content changed, then the engine's commit.
    write(&fork.join("Content.Shared/Thing.cs"), "class Thing { int more; }\n");
    assert_eq!(Fingerprint::of(&fork, Part::Code).differences(&first), ["changes"]);
    commit(&engine);
    let moved = Fingerprint::of(&fork, Part::Code);
    assert_eq!(moved.differences(&first), ["changes", "RobustToolbox"]);
}

#[test]
fn a_folder_that_is_not_a_git_checkout_is_told_by_its_files() {
    let fork = folder("not-git");
    write(&fork.join("Resources/Prototypes/walls.yml"), "- type: entity\n");
    let first = Fingerprint::of(&fork, Part::Prototypes);
    assert!(!first.is_git());
    assert_eq!(value(&first, "Resources/Prototypes").split(' ').next(), Some("1"));
    assert_eq!(value(&first, "RobustToolbox/Resources/Prototypes"), "-");
    write(&fork.join("Resources/Prototypes/more.yml"), "- type: entity\n");
    assert_ne!(Fingerprint::of(&fork, Part::Prototypes), first);
    assert_eq!(Fingerprint::of(&fork, Part::Code).lines.len(), 1, "code is not told by files");
}

#[test]
fn git_s_list_of_changes_gives_each_path_once() {
    let status = b" M a.yml\0R  new.yml\0old.yml\0?? b c.yml\0";
    assert_eq!(changed_paths(status), ["a.yml", "new.yml", "b c.yml"]);
}

#[test]
fn a_fingerprint_is_read_back_from_its_text() {
    let print = Fingerprint { lines: vec![("Content.Shared".into(), "abc".into()), ("changes".into(), "none".into())] };
    assert_eq!(Fingerprint::parse(&print.text()), print);
    let stamp = EngineStamp { engine: "123 456".into(), from: "C:/forks/starlight".into(), code: print };
    assert_eq!(EngineStamp::parse(&stamp.text()), Some(stamp));
    assert_eq!(EngineStamp::parse("not a stamp"), None);
}

#[test]
fn a_stamp_is_written_beside_the_engine_side_for_its_build() {
    let fork = fork("stamp");
    let engine = folder("stamp-engine");
    assert!(EngineStamp::write(&engine, &fork).is_err(), "no engine side to stamp");
    write(&engine.join("Editor.Native.dll"), "the engine");
    let written = EngineStamp::write(&engine, &fork).unwrap();
    assert_eq!(EngineStamp::read(&engine), Some(written.clone()));
    assert_eq!(engine_identity(&engine).as_deref(), Some(written.engine.as_str()));
    assert_eq!(written.code, Fingerprint::of(&fork, Part::Code));

    // Unzipping a release writes the dll again with a new time: still the build stamped.
    let dll = engine.join("Editor.Native.dll");
    std::fs::File::options().write(true).open(&dll).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400)).unwrap();
    assert_eq!(engine_identity(&engine).as_deref(), Some(written.engine.as_str()));
    // Another build of the same size is another build.
    write(&dll, "the engin2");
    assert_ne!(engine_identity(&engine).as_deref(), Some(written.engine.as_str()));
}

fn stamp(code: &Fingerprint) -> EngineStamp {
    EngineStamp { engine: "100 200".into(), from: "C:/forks/starlight".into(), code: code.clone() }
}

#[test]
fn the_same_code_opens() {
    let code = Fingerprint { lines: vec![("Content.Shared".into(), "abc".into()), ("RobustToolbox".into(), "def".into())] };
    assert_eq!(code_verdict("Starlight", &code, Some(&stamp(&code)), Some("100 200")), CodeVerdict::Same);
}

#[test]
fn other_code_is_refused_with_what_differs() {
    let code = Fingerprint { lines: vec![("Content.Shared".into(), "abc".into()), ("changes".into(), "none".into()), ("RobustToolbox".into(), "def".into())] };
    let other = Fingerprint { lines: vec![("Content.Shared".into(), "xyz".into()), ("changes".into(), "1234".into()), ("RobustToolbox".into(), "uvw".into())] };
    let CodeVerdict::Differs(why) = code_verdict("Starlight", &other, Some(&stamp(&code)), Some("100 200")) else { panic!("not refused") };
    assert!(why.starts_with("Starlight's code isn't the code the engine side was built from."), "{why}");
    assert!(why.contains("Content.Shared, the uncommitted changes and the engine's commit (RobustToolbox)"), "{why}");
    assert!(why.contains("C:/forks/starlight"), "{why}");
}

#[test]
fn an_engine_side_with_no_fingerprint_or_an_old_one_opens_with_a_warning() {
    let code = Fingerprint { lines: vec![("Content.Shared".into(), "abc".into())] };
    assert!(matches!(code_verdict("Starlight", &code, None, Some("100 200")), CodeVerdict::Unstamped(why) if why.contains("no code fingerprint")));
    // Published again by hand: the stamp is for another build, and even other code is let through.
    let other = Fingerprint { lines: vec![("Content.Shared".into(), "xyz".into())] };
    assert!(matches!(code_verdict("Starlight", &other, Some(&stamp(&code)), Some("999 200")), CodeVerdict::Unstamped(why) if why.contains("published again")));
}

#[test]
fn a_fork_that_is_not_a_git_checkout_is_refused() {
    let code = Fingerprint { lines: vec![("not-git".into(), "C:/forks/zip".into())] };
    let verdict = code_verdict("Zip", &code, Some(&stamp(&code)), Some("100 200"));
    assert!(matches!(verdict, CodeVerdict::Differs(why) if why.starts_with("Zip isn't a git checkout")));
}
