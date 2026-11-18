//! Fix grid atmos: when it can run, the dialog's choice of grids and its count, the one edit it
//! asks for, Validate Map's Fix, and the grids whose air is read again after it.

use super::entity::opened;
use super::*;
use crate::air::{AirAnswer, GridAirJson, GridsOf, Mix};
use crate::validate::{Finding, Rule};

/// A second grid beside the test map's, a shuttle of four floors.
const SHUTTLE: i32 = 3;

/// The test map with the shuttle added.
fn two_grids() -> String {
    let mut scene: Value = serde_json::from_str(&scene()).unwrap();
    let shuttle = json!({ "uid": SHUTTLE, "name": "Shuttle", "x": 20, "y": 0, "rotation": 0, "tileIds": ["Space", "Plating"], "tiles": [0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0] });
    scene["grids"].as_array_mut().unwrap().push(shuttle);
    scene.to_string()
}

/// The air of grids as the engine reads it: station air on one floor of each, and whether
/// each has an atmosphere.
fn air(grids: &[(i32, Option<bool>)]) -> AirAnswer {
    let mix = Mix { temperature: 293.15, volume: 2500.0, immutable: false, moles: vec![21.824879, 82.10312] };
    AirAnswer {
        gases: vec!["Oxygen".into(), "Nitrogen".into()],
        grids: grids.iter().map(|&(grid, atmosphere)| GridAirJson { grid, mixes: vec![mix.clone()], tiles: vec![0, 0, 0], atmosphere, ..Default::default() }).collect(),
        map_is_space: true,
    }
}

/// The same, with these grids told to have no atmosphere and to be able to take one.
fn air_addable(grids: &[(i32, Option<bool>)], addable: &[i32]) -> AirAnswer {
    let mut answer = air(grids);
    for grid in answer.grids.iter_mut().filter(|grid| addable.contains(&grid.grid)) {
        (grid.atmosphere, grid.addable) = (Some(false), Some(true));
    }
    answer
}

/// The kinds of an edit's ops and their grids, in order.
fn kinds(ops: &[Value]) -> Vec<(&str, i64)> {
    ops.iter().map(|op| (op["kind"].as_str().unwrap(), op["grid"].as_i64().unwrap())).collect()
}

fn send(session: &mut Session, answers: &Sender<FromEngine>, reply: FromEngine) {
    answers.send(reply).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// How many times the session asked the engine to count what a fix changes, since the last call.
fn previews_asked(from: &Receiver<ToEngine>) -> usize {
    from.try_iter().filter(|command| matches!(command, ToEngine::AirFixPreview)).count()
}

fn preview(grid: i32, tiles: usize, outside: usize, refused: Option<&str>) -> FixPreview {
    FixPreview { grid, refused: refused.map(str::to_owned), tiles, outside, addable: false }
}

/// The grids an edit's ops fix, in order.
fn fixed(ops: &[Value]) -> Vec<i64> {
    assert!(ops.iter().all(|op| op["kind"] == "air.fix"), "{ops:?}");
    ops.iter().map(|op| op["grid"].as_i64().unwrap()).collect()
}

#[test]
fn the_fix_waits_for_the_air_and_needs_a_grid_with_an_atmosphere() {
    let (mut session, _answers, engine) = opened(&scene());
    assert_eq!(session.fix_air_unavailable().as_deref(), Some("The map's air is still loading."));
    assert!(!session.open_fix_air());
    assert_eq!(session.status_line(), "The map's air is still loading.");

    // A grid with no atmosphere that cannot be given one, as a planet's ground, cannot be fixed.
    session.air_arrived(1, None, Ok(air(&[(GRID, Some(false))])));
    assert_eq!(session.fix_air_unavailable().as_deref(), Some("This map has no grid with an atmosphere, and none that can take one."));
    session.backdrop = Some(BackdropInfo { name: "Default".into(), layers: 0, skipped: 0, moves: false, paint_ms: 0, chosen: false, planet: true });
    assert_eq!(session.fix_air_unavailable().as_deref(), Some("This planet map has no grid with an atmosphere. Its ground uses the map's air, and the game gives it none."));
    assert!(!session.open_fix_air() && !session.fix_air_open());
    session.fix_grid_air(GRID);
    assert_eq!(session.status_line(), "Air not fixed: This planet map has no grid with an atmosphere. Its ground uses the map's air, and the game gives it none.");
    assert!(edits(&engine).is_empty());

    // A new map's grid has none and can be given one: the menu item is not greyed.
    session.backdrop = None;
    session.air_arrived(1, None, Ok(air_addable(&[(GRID, None)], &[GRID])));
    assert_eq!(session.fix_air_unavailable(), None);

    // An engine from before it told whether a grid has one is taken to say yes.
    session.air_arrived(1, None, Ok(air(&[(GRID, None)])));
    assert_eq!(session.fix_air_unavailable(), None);
}

#[test]
fn the_dialog_counts_what_will_change_and_the_fix_is_one_edit_of_every_grid() {
    let (mut session, answers, engine) = opened(&scene());
    session.air_arrived(1, None, Ok(air(&[(GRID, Some(true))])));
    assert!(session.open_fix_air() && session.fix_air_open());
    assert_eq!(previews_asked(&engine), 1);

    // One grid: no choice of grids, and the count is on its way.
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!((dialog.all_label.as_str(), dialog.one_label.as_deref(), dialog.all), ("All grids (1)", None, true));
    assert_eq!((dialog.does, dialog.count.as_str(), dialog.blocked.as_deref()), (FIX_AIR_DOES, "Counting the tiles it will change...", None));

    send(&mut session, &answers, FromEngine::AirFixPreview { document: 1, previews: Ok(vec![preview(GRID, 1234, 1, None)]) });
    assert_eq!(session.fix_air_dialog().unwrap().count, "The air of 1,234 tiles will change. 1 cell off the grid will lose the air stored there.");
    send(&mut session, &answers, FromEngine::AirFixPreview { document: 1, previews: Ok(vec![preview(GRID, 0, 0, None)]) });
    assert_eq!(session.fix_air_dialog().unwrap().count, "No tile's air will change.");
    // A count for another map is passed over.
    send(&mut session, &answers, FromEngine::AirFixPreview { document: 9, previews: Ok(vec![preview(GRID, 5, 0, None)]) });
    assert_eq!(session.fix_air_dialog().unwrap().count, "No tile's air will change.");

    session.confirm_fix_air();
    assert!(!session.fix_air_open() && session.fix_air_dialog().is_none());
    let asked = edits(&engine);
    assert_eq!((labels(&asked), asked[0].1), (vec![FIX_AIR], None));
    assert_eq!(fixed(&asked[0].2), [GRID as i64]);

    // Once the engine answers, the status bar says what was fixed.
    let seq = session.next_seq;
    send(&mut session, &answers, FromEngine::Changed { list: None, delta: None, what: FIX_AIR, seq, asked: Instant::now(), timing: Timing::default() });
    assert_eq!(session.status_line(), "Fixed the air of grid (2)");

    // Cancelled, nothing is asked.
    assert!(session.open_fix_air());
    session.cancel_fix_air();
    assert!(!session.fix_air_open() && edits(&engine).is_empty());
}

#[test]
fn of_two_grids_all_or_the_one_under_the_mouse_is_fixed_and_one_with_no_atmosphere_is_left_alone() {
    let (mut session, answers, engine) = opened(&two_grids());
    assert_eq!(session.doc.as_ref().unwrap().grids.len(), 2);
    session.air_arrived(1, None, Ok(air(&[(GRID, Some(true)), (SHUTTLE, Some(true))])));

    // The mouse over the shuttle: it is the one grid offered, and all are chosen first.
    session.hover = Some((20.5, 0.5));
    assert!(session.open_fix_air());
    send(&mut session, &answers, FromEngine::AirFixPreview { document: 1, previews: Ok(vec![preview(GRID, 3, 0, None), preview(SHUTTLE, 4, 0, None)]) });
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!((dialog.all_label.as_str(), dialog.one_label.as_deref(), dialog.all, dialog.count.as_str()), ("All grids (2)", Some("Shuttle (3)"), true, "The air of 7 tiles will change."));
    session.choose_fix_air_grids(false);
    assert_eq!(session.fix_air_dialog().unwrap().count, "The air of 4 tiles will change.");
    session.confirm_fix_air();
    assert_eq!(fixed(&asked(&engine)[0]), [SHUTTLE as i64]);

    // Away from both, the Grids tab's grid is offered. All grids is one edit of both.
    session.hover = Some((50.0, 50.0));
    assert!(session.open_fix_air());
    assert_eq!(session.fix_air_dialog().unwrap().one_label.as_deref(), Some("grid (2)"));
    session.confirm_fix_air();
    let asked = edits(&engine);
    assert_eq!((asked.len(), fixed(&asked[0].2)), (1, vec![GRID as i64, SHUTTLE as i64]));

    // The shuttle with no atmosphere is left alone, and said to be. Chosen alone, it blocks the fix.
    session.air_arrived(1, None, Ok(air(&[(GRID, Some(true)), (SHUTTLE, Some(false))])));
    session.hover = Some((20.5, 0.5));
    assert!(session.open_fix_air());
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!((dialog.left_alone.as_slice(), dialog.blocked.as_deref()), (&["Shuttle (3): It has no atmosphere, so the game's fixgridatmos refuses it.".to_owned()][..], None));
    session.choose_fix_air_grids(false);
    assert_eq!(session.fix_air_dialog().unwrap().blocked.as_deref(), Some("Shuttle (3): It has no atmosphere, so the game's fixgridatmos refuses it."));
    session.confirm_fix_air();
    assert!(edits(&engine).is_empty());

    // A grid the engine would refuse, as for a marker of a mix the game lacks, is left alone too.
    assert!(session.open_fix_air());
    send(&mut session, &answers, FromEngine::AirFixPreview { document: 1, previews: Ok(vec![preview(GRID, 0, 0, Some("The marker at 1, 1 asks for mix 12."))]) });
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!(dialog.left_alone.len(), 2);
    assert_eq!(dialog.blocked.as_deref(), Some("grid (2): The marker at 1, 1 asks for mix 12."));

    // Another map opened closes the dialog.
    send(&mut session, &answers, FromEngine::Opened { scene: Box::new(serde_json::from_str(&scene()).unwrap()), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) });
    assert!(!session.fix_air_open());
}

#[test]
fn a_grid_with_no_atmosphere_is_given_one_first_in_the_same_edit_unless_the_choice_is_unticked() {
    let (mut session, answers, engine) = opened(&two_grids());
    // The shuttle has none, as a grid made in the editor, and can be given one.
    session.air_arrived(1, None, Ok(air_addable(&[(GRID, Some(true)), (SHUTTLE, None)], &[SHUTTLE])));
    assert!(session.open_fix_air());
    let mut told = preview(SHUTTLE, 4, 0, Some("The grid Shuttle (3) has no atmosphere."));
    told.addable = true;
    send(&mut session, &answers, FromEngine::AirFixPreview { document: 1, previews: Ok(vec![preview(GRID, 3, 0, None), told]) });

    // Listed, with the choice ticked at first, and counted with the rest.
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!((dialog.without.as_slice(), dialog.add_first, dialog.add_label), (&["Shuttle (3)".to_owned()][..], true, ADD_FIRST));
    assert_eq!((dialog.count.as_str(), dialog.left_alone.len(), dialog.blocked.as_deref()), ("The air of 7 tiles will change.", 0, None));
    session.confirm_fix_air();
    let asked = edits(&engine);
    assert_eq!((labels(&asked), asked.len()), (vec![FIX_AIR], 1));
    assert_eq!(kinds(&asked[0].2), [("air.add", SHUTTLE as i64), ("air.fix", GRID as i64), ("air.fix", SHUTTLE as i64)]);
    let seq = session.next_seq;
    send(&mut session, &answers, FromEngine::Changed { list: None, delta: None, what: FIX_AIR, seq, asked: Instant::now(), timing: Timing::default() });
    assert_eq!(session.status_line(), "Fixed the air of 2 grids, 1 given an atmosphere first");

    // Unticked, it is left alone and said to be. Chosen alone, it blocks the fix.
    assert!(session.open_fix_air());
    session.set_fix_air_add_first(false);
    let dialog = session.fix_air_dialog().unwrap();
    assert_eq!((dialog.without.len(), dialog.add_first), (1, false));
    assert_eq!(dialog.left_alone, ["Shuttle (3): It has no atmosphere, and none will be added first."]);
    session.confirm_fix_air();
    assert_eq!(kinds(&edits(&engine)[0].2), [("air.fix", GRID as i64)]);
    session.hover = Some((20.5, 0.5));
    assert!(session.open_fix_air());
    session.choose_fix_air_grids(false);
    assert_eq!(session.fix_air_dialog().unwrap().count, "Counting the tiles it will change...");
    session.set_fix_air_add_first(false);
    assert_eq!(session.fix_air_dialog().unwrap().blocked.as_deref(), Some("Shuttle (3): It has no atmosphere, and none will be added first."));
    // Ticked again, it is the one edit of add and fix.
    session.set_fix_air_add_first(true);
    session.confirm_fix_air();
    assert_eq!(kinds(&edits(&engine)[0].2), [("air.add", SHUTTLE as i64), ("air.fix", SHUTTLE as i64)]);

    // Validate Map's Fix gives it one first too.
    session.fix_grid_air(SHUTTLE);
    assert_eq!(kinds(&edits(&engine)[0].2), [("air.add", SHUTTLE as i64), ("air.fix", SHUTTLE as i64)]);
    let finding = Finding { rule: Rule::NoAtmosphere, message: String::new(), grid: SHUTTLE, cell: (0, 0), entity: None, decal: None };
    assert_eq!(finding.fixes_air(), Some(SHUTTLE));
}

#[test]
fn validate_s_floors_with_no_air_fix_their_grid_at_once() {
    let (mut session, _answers, engine) = opened(&scene());
    session.air_arrived(1, None, Ok(air(&[(GRID, Some(true))])));
    let finding = |rule| Finding { rule, message: String::new(), grid: GRID, cell: (1, 1), entity: None, decal: None };
    assert_eq!((finding(Rule::UnfilledFloor).fixes_air(), finding(Rule::UnevenAir).fixes_air()), (Some(GRID), None));

    session.fix_grid_air(GRID);
    let asked = edits(&engine);
    assert_eq!((labels(&asked), fixed(&asked[0].2)), (vec![FIX_AIR], vec![GRID as i64]));
    assert!(!session.fix_air_open());
}

#[test]
fn a_change_that_names_a_grid_s_air_has_it_read_again() {
    let mut grids = GridsOf::new(&serde_json::from_str(&scene()).unwrap());
    let delta: Delta = serde_json::from_value(json!({
        "tiles": [], "entities": [], "removed": [], "gridsChanged": [], "created": [], "air": [GRID],
        "history": { "canUndo": true, "canRedo": false, "revision": 1, "undoLabel": FIX_AIR, "redoLabel": null }
    }))
    .unwrap();
    assert_eq!(grids.touched(&delta), [GRID]);
}
