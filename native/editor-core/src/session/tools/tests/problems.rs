//! The Problems overlay, its switch kept per fork, and Validate Map's fixes of the rules of walls.

use super::*;
use crate::forks::settings::Settings;
use crate::forks::tests::folder;
use crate::validate::{Fix, Rule};

const EXTRA_WALL: i32 = 20;
const CABLE: i32 = 21;
const CABLE_TWICE: i32 = 22;
const LOOSE_WALL: i32 = 23;
const ON_THE_MAP: i32 = 24;

/// An entity of the test scene, standing on the middle of a cell of its grid or of the map.
fn standing(uid: i32, prototype: &str, parent: i32, cell: (i32, i32), anchored: bool) -> Entity {
    Entity { uid, yaml_uid: uid, prototype: prototype.into(), parent, x: cell.0 as f32 + 0.5, y: cell.1 as f32 + 0.5, rotation: 0.0, anchored, contained: false, name: None }
}

/// The test scene with problems: a second wall on the wall's cell (3, 1), a cable twice on
/// (2, 2), and a wall loose on (0, 5), whose tile is gone. A wall stands on the map, off the
/// grid. The engine knows walls by their tag, and has plating.
fn troubled() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.tiles = vec![Tile { id: PLATING.into(), sprite: None, allows_rotation: false, can_crowbar: false }];
    let wall = Prototype { id: "WallSolid".into(), tags: vec![Arc::from("Wall")], ..Default::default() };
    session.set_prototypes(vec![wall, Prototype { id: "CableHV".into(), ..Default::default() }]);
    let doc = session.doc.as_mut().unwrap();
    doc.grids[0].tiles.remove(&(0, 5));
    for entity in [
        standing(EXTRA_WALL, "WallSolid", GRID, (3, 1), true),
        standing(CABLE, "CableHV", GRID, (2, 2), true),
        standing(CABLE_TWICE, "CableHV", GRID, (2, 2), true),
        standing(LOOSE_WALL, "WallSolid", GRID, (0, 5), false),
        standing(ON_THE_MAP, "WallSolid", 1, (9, 9), false),
    ] {
        doc.entities.insert(entity.uid, entity);
    }
    (session, engine)
}

fn found(session: &Session, rule: Rule) -> Vec<crate::validate::Finding> {
    session.validate().into_iter().filter(|finding| finding.rule == rule).collect()
}

#[test]
fn removing_extras_is_one_edit_through_the_delete_op() {
    let (mut session, engine) = troubled();
    // The second wall is both stacked and a duplicate: it is deleted once.
    let mut findings = found(&session, Rule::StackedWalls);
    findings.extend(found(&session, Rule::Duplicates));
    assert_eq!(findings.len(), 3);
    session.fix_findings(&findings);
    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0], ("Remove extras", None, vec![json!({ "kind": "delete", "uid": EXTRA_WALL }), json!({ "kind": "delete", "uid": CABLE_TWICE })]));
    assert_eq!(session.edit_says.as_ref().map(|(_, says)| says.as_str()), Some("Removed 2 extras"));

    // One finding's button removes its own extras only.
    let cable = found(&session, Rule::Duplicates).into_iter().find(|finding| finding.cell == (2, 2)).unwrap();
    session.fix_finding(&cable);
    assert_eq!(asked(&engine), [vec![json!({ "kind": "delete", "uid": CABLE_TWICE })]]);
}

#[test]
fn laying_plating_anchors_what_stood_loose_in_the_same_edit() {
    let (mut session, engine) = troubled();
    let no_floor = found(&session, Rule::WallWithoutFloor);
    let fixes: Vec<Option<Fix>> = no_floor.iter().map(|finding| finding.fix()).collect();
    assert_eq!(fixes, [Some(Fix::LayPlating), Some(Fix::Select(ON_THE_MAP))]);

    session.fix_finding(&no_floor[0]);
    let edits = edits(&engine);
    assert_eq!(
        edits,
        [("Lay plating", None, vec![json!({ "kind": "tile", "grid": GRID, "x": 0, "y": 5, "tile": PLATING }), json!({ "kind": "anchor", "uid": LOOSE_WALL })])]
    );
    assert_eq!(session.edit_says.as_ref().map(|(_, says)| says.as_str()), Some("Laid plating on 1 cell"));

    // The wall on the map belongs to no grid: its button selects it.
    session.fix_finding(&no_floor[1]);
    assert!(asked(&engine).is_empty());
    assert_eq!((session.tool(), &session.selection[..]), (Tool::Select, &[ON_THE_MAP][..]));
}

#[test]
fn every_fix_of_a_group_is_one_edit() {
    let (mut session, engine) = troubled();
    let mut all = found(&session, Rule::StackedWalls);
    all.extend(found(&session, Rule::WallWithoutFloor));
    session.fix_findings(&all);
    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    let kinds: Vec<&str> = edits[0].2.iter().map(|op| op["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["delete", "tile", "anchor"]);
    assert_eq!(session.edit_says.as_ref().map(|(_, says)| says.as_str()), Some("Removed 1 extra and laid plating on 1 cell"));
}

#[test]
fn no_plating_is_laid_in_a_fork_without_it() {
    let (mut session, engine) = troubled();
    session.tiles.clear();
    session.fix_findings(&found(&session, Rule::WallWithoutFloor));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Plating not laid: this fork has no Plating tile.");
}

#[test]
fn the_overlay_marks_each_problem_cell_once_while_it_is_on() {
    let (mut session, _engine) = troubled();
    // Off at first: nothing is checked or drawn.
    let (off, quads) = session.problem_overlay(32.0);
    assert!(quads.is_empty());
    assert_eq!(session.problems_checked().1, 0);

    session.show_problems(true);
    let (on, quads) = session.problem_overlay(32.0);
    assert!(on != off && !quads.is_empty());
    // The wall's cell (stacked, a duplicate and a floor under each wall), the cables' cell, the
    // loose wall's cell and the cell of the map under the wall off the grid.
    assert_eq!(session.problems_checked().1, 4);
    // Nothing changed: the same rectangles.
    assert_eq!(session.problem_overlay(32.0).0, on);

    // The mouse over the stacked walls' badge, in the top right of their cell.
    session.hover = Some((3.71, 1.71));
    let lines = session.problem_hover().unwrap();
    assert_eq!(lines[0], "4 problems here");
    assert!(lines.iter().any(|line| line == "WallSolid and WallSolid are stacked at (3, 1). One cell holds one wall, window or door."), "{lines:?}");
    // Over the rest of the cell, nothing.
    session.hover = Some((3.2, 1.2));
    assert_eq!(session.problem_hover(), None);

    // A change from the engine checks again: the extra wall gone, its cell keeps the floor under it.
    session.doc.as_mut().unwrap().entities.remove(&EXTRA_WALL);
    session.changes += 1;
    let (changed, _) = session.problem_overlay(32.0);
    assert!(changed != on);
    session.hover = Some((3.71, 1.71));
    assert_eq!(session.problem_hover().unwrap()[0], "Floor tiles under walls");

    session.show_problems(false);
    assert!(session.problem_overlay(32.0).1.is_empty());
    assert_eq!(session.problem_hover(), None);
}

#[test]
fn the_switch_is_kept_in_the_settings_under_the_fork() {
    let file = folder("problems").join("settings.json");
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), Some("p".into()), None);
    settings.save(&file).unwrap();

    let (mut session, _engine) = troubled();
    session.problems = crate::session::problems::Problems::load(file.clone(), "C:/forks/starlight");
    assert!(!session.problems_shown());
    session.show_problems(true);
    let read = Settings::load(&file);
    // What else the file holds stays.
    assert_eq!(read.forks[0].prototypes.as_deref(), Some("p"));
    assert!(read.problems_shown("C:\\forks\\starlight\\") && !read.problems_shown("C:/checkouts/wizden"));

    // A session of this fork starts with it on, one of another fork with it off.
    let (mut again, _again_engine) = troubled();
    again.problems = crate::session::problems::Problems::load(file.clone(), "C:/forks/starlight");
    assert!(again.problems_shown());
    let (mut other, _other_engine) = troubled();
    other.problems = crate::session::problems::Problems::load(file.clone(), "C:/checkouts/wizden");
    assert!(!other.problems_shown());

    again.show_problems(false);
    assert!(!Settings::load(&file).problems_shown("C:/forks/starlight"));
}
