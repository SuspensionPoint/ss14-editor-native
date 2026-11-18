//! Nudging: the arrow keys move the selection or the region a cell, or with Shift a pixel.

use super::super::area::move_ops;
use super::super::nudge::FINE_STEP;
use super::decals::decals_on_map;
use super::*;

/// The entity each `move` op of an edit moves, and where to.
fn moves(ops: &[Value]) -> Vec<(i64, f64, f64)> {
    ops.iter().filter(|op| op["kind"] == "move").map(|op| (op["uid"].as_i64().unwrap(), op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect()
}

/// What the status bar will say when the engine answers the last edit.
fn says(session: &Session) -> &str {
    session.edit_says.as_ref().map_or("", |(_, says)| says.as_str())
}

/// A session that is answered: the scene has been opened in it as the engine opens a map,
/// with the chair and the wall drawn and the select tool in hand.
fn answered() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) }).ok().expect("a session to send to");
    assert!(session.pump(VIEW));
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.set_tool(Tool::Select);
    (session, answers, asked)
}

/// The engine says it has made the edit of this number.
fn answer(session: &mut Session, answers: &Sender<FromEngine>, seq: u64) {
    answers.send(FromEngine::Changed { list: None, delta: None, what: "Move", seq, asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump(VIEW);
}

#[test]
fn a_press_moves_the_selection_one_cell_as_one_edit_with_no_stroke() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.nudge(Arrow::Right, false);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Move", None));
    assert_eq!(moves(&edits[0].2), [(CHAIR as i64, 2.5, 1.5)]);
    assert_eq!(says(&session), "Moved 1 entity one cell east");
}

#[test]
fn each_answered_press_is_its_own_edit_and_presses_in_flight_are_dropped() {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR, WALL];
    session.nudge(Arrow::Up, false);
    let first = numbers(&engine);
    assert_eq!(first.len(), 1);
    assert_eq!(says(&session), "Moved 2 entities one cell north");

    // The mirror still has them where they were, so the repeats of a held key ask nothing.
    session.nudge(Arrow::Up, false);
    session.nudge(Arrow::Up, false);
    assert!(asked(&engine).is_empty());

    // Answered, the next press is a new edit, a step of its own to undo.
    answer(&mut session, &answers, first[0]);
    session.nudge(Arrow::Up, false);
    let edits = edits(&engine);
    assert_eq!((edits.len(), edits[0].1), (1, None));

    // An undo not yet answered holds the next press back too.
    let last = session.last_edit;
    answer(&mut session, &answers, last);
    session.undo();
    session.nudge(Arrow::Up, false);
    assert!(asked(&engine).is_empty());
}

#[test]
fn shift_steps_one_pixel_only_when_nothing_selected_is_anchored() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.nudge(Arrow::Up, true);
    assert_eq!(moves(&one_edit(&engine)), [(CHAIR as i64, 1.5, 1.5 + f64::from(FINE_STEP))]);
    assert_eq!(says(&session), "Moved 1 entity one pixel north");

    // With the wall, which is anchored, it is a whole cell for both.
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.nudge(Arrow::Left, true);
    assert_eq!(moves(&one_edit(&engine)), [(CHAIR as i64, 0.5, 1.5), (WALL as i64, 2.5, 1.5)]);
    assert_eq!(says(&session), "Moved 2 entities one cell west");
}

#[test]
fn an_anchored_selection_stays_on_the_middle_of_cells() {
    let (mut session, engine) = selecting();
    session.selection = vec![WALL];
    for arrow in [Arrow::Down, Arrow::Right] {
        session.last_answered = session.last_edit;
        session.nudge(arrow, false);
    }
    assert_eq!(asked(&engine).iter().map(|ops| moves(ops)).collect::<Vec<_>>(), [vec![(WALL as i64, 3.5, 0.5)], vec![(WALL as i64, 4.5, 1.5)]]);
}

#[test]
fn a_region_moves_one_cell_with_its_tiles_as_a_drag_moves_it() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    session.region = Region::new(GRID, [(1, 1), (2, 1)]);
    let wanted = move_ops(session.doc.as_ref().unwrap(), session.region.as_ref().unwrap(), (0, 1));
    // Shift is no fine step for a region.
    session.nudge(Arrow::Up, true);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1, &edits[0].2), ("Move region", None, &wanted));
    assert!(edits[0].2.iter().any(|op| op["kind"] == "tile") && moves(&edits[0].2) == [(CHAIR as i64, 1.5, 2.5)]);
    let mut cells: Vec<(i32, i32)> = session.region.as_ref().unwrap().cells().iter().copied().collect();
    cells.sort_unstable();
    assert_eq!(cells, [(1, 2), (2, 2)]);
    assert_eq!(says(&session), "Moved region one cell north");
}

#[test]
fn selected_decals_move_with_the_entities() {
    let (mut session, engine) = decals_on_map(&[(3, "Grass", (3.5, 4.0))]);
    session.set_tool(Tool::Select);
    session.selection = vec![CHAIR];
    session.selected_decals = vec![(GRID, 3)];
    session.nudge(Arrow::Down, false);

    let ops = one_edit(&engine);
    assert_eq!(moves(&ops), [(CHAIR as i64, 1.5, 0.5)]);
    let decal = ops.iter().find(|op| op["kind"] == "decal.update").unwrap();
    assert_eq!((decal["uid"].as_i64(), decal["x"].as_f64(), decal["y"].as_f64()), (Some(3), Some(3.5), Some(3.0)));
    assert_eq!(says(&session), "Moved 1 entity and 1 decal one cell south");

    // Decals alone step a pixel with Shift.
    session.selection.clear();
    session.last_answered = session.last_edit;
    session.nudge(Arrow::Right, true);
    let ops = one_edit(&engine);
    assert_eq!(ops[0]["x"].as_f64(), Some(f64::from(3.5 + FINE_STEP)));
    assert_eq!(says(&session), "Moved 1 decal one pixel east");
}

#[test]
fn with_nothing_selected_or_another_tool_the_keys_do_nothing_and_do_not_pan() {
    let (mut session, engine) = selecting();
    let camera = (session.camera.x, session.camera.y);
    session.nudge(Arrow::Left, false);
    // The area tool with no region.
    session.set_tool(Tool::Area);
    session.nudge(Arrow::Left, false);
    // A selection with another tool in hand.
    session.selection = vec![CHAIR];
    session.set_tool(Tool::Place);
    session.nudge(Arrow::Left, false);

    assert!(asked(&engine).is_empty());
    assert_eq!((session.camera.x, session.camera.y), camera);
}

#[test]
fn a_field_a_dialog_or_a_menu_takes_the_keys() {
    assert!(arrows_reach_map(false, false, false));
    assert!(!arrows_reach_map(false, false, true));
    assert!(!arrows_reach_map(false, true, false));
    assert!(!arrows_reach_map(true, false, false));

    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.dialog_open = true;
    session.nudge(Arrow::Right, false);
    assert!(asked(&engine).is_empty());
}

#[test]
fn nothing_moves_during_a_drag() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.nudge(Arrow::Right, false);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_turned_grid_turns_the_directions() {
    let (mut session, engine) = selecting();
    // Turned a quarter against the clock, the grid's east is up the map.
    session.doc.as_mut().unwrap().grids[0].rotation = std::f32::consts::FRAC_PI_2;
    session.selection = vec![CHAIR];
    session.nudge(Arrow::Right, false);

    let (_, x, y) = moves(&one_edit(&engine))[0];
    let grid = &session.doc.as_ref().unwrap().grids[0];
    let (from, to) = (grid.to_world((1.5, 1.5)), grid.to_world((x as f32, y as f32)));
    assert!((to.0 - from.0).abs() < 1e-5 && (to.1 - from.1 - 1.0).abs() < 1e-5, "{from:?} {to:?}");
}

#[test]
fn a_move_the_engine_refuses_says_so_as_a_drag_s_does() {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR];
    session.nudge(Arrow::Right, false);
    let seq = numbers(&engine)[0];
    answers.send(FromEngine::Refused { seq, what: "Move", error: "A decal needs a tile under it.".into() }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.status_line(), "Move: the engine refused: A decal needs a tile under it.");

    // The refusal is an answer: the next press is asked for.
    session.nudge(Arrow::Right, false);
    assert_eq!(numbers(&engine).len(), 1);
}

/// The ops of the one edit asked for since the last call.
#[track_caller]
fn one_edit(from: &Receiver<ToEngine>) -> Vec<Value> {
    let mut edits = asked(from);
    assert_eq!(edits.len(), 1);
    edits.remove(0)
}
