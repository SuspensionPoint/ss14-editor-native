//! Copy, cut and paste.

use super::*;

const SHIFT: Held = Held { shift: true, ctrl: false, space: false };

/// The text the engine answers a copy with. The core never reads it.
const TEXT: &str = "entities: [chair, wall, crowbar]";

/// A session that is answered, with the scene opened in it as the engine opens a map, its
/// chair and wall drawn where they stand, and the select tool in hand.
pub(super) fn answered() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    open(&mut session, &answers);
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.set_tool(Tool::Select);
    (session, answers, asked)
}

/// The engine says it has opened the scene.
pub(super) fn open(session: &mut Session, answers: &Sender<FromEngine>) {
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) }).ok().expect("a session to send to");
    assert!(session.pump(VIEW));
}

/// The one thing asked of the engine since the last call, which has to be a copy: its number
/// and the entities it is of.
#[track_caller]
pub(super) fn one_copy(from: &Receiver<ToEngine>) -> (u64, Vec<i32>) {
    let commands: Vec<ToEngine> = from.try_iter().collect();
    match &commands[..] {
        [ToEngine::Copy { uids, seq }] => (*seq, uids.clone()),
        _ => panic!("{} things asked, and not one copy alone", commands.len()),
    }
}

/// The engine answers a copy: the chair, and the wall with the crowbar in it, measured from cell (1, 1).
fn answer(session: &mut Session, answers: &Sender<FromEngine>, seq: u64) {
    answers.send(FromEngine::Copied { seq, text: TEXT.into(), roots: 2, entities: 3, from: (1, 1) }).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// A session as `answered` gives it, whose chair and wall have been copied, with nothing
/// selected after.
fn copied() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR, WALL];
    session.copy_selection();
    let (seq, _) = one_copy(&engine);
    answer(&mut session, &answers, seq);
    session.selection.clear();
    (session, answers, engine)
}

/// The box around each rectangle the ghost is drawn with, (left, bottom, right, top) to the
/// thousandth, and how faint each is.
fn ghost(session: &mut Session) -> Vec<((f32, f32, f32, f32), f32)> {
    let round = |value: f32| (value * 1000.0).round() / 1000.0;
    let (_, over) = session.overlay_quads();
    over.chunks_exact(FLOATS_PER_QUAD)
        .map(|quad| {
            let (xs, ys) = ([quad[0], quad[2], quad[4], quad[6]], [quad[1], quad[3], quad[5], quad[7]]);
            let low = |values: [f32; 4]| round(values.iter().copied().fold(f32::MAX, f32::min));
            let high = |values: [f32; 4]| round(values.iter().copied().fold(f32::MIN, f32::max));
            ((low(xs), low(ys), high(xs), high(ys)), quad[11])
        })
        .collect()
}

/// The quarters of each paste op asked for since the last call.
fn quarters(from: &Receiver<ToEngine>) -> Vec<i64> {
    let ops: Vec<Value> = asked(from).concat();
    assert!(ops.iter().all(|op| op["kind"] == "paste"), "{ops:?}");
    ops.iter().map(|op| op["quarters"].as_i64().unwrap()).collect()
}

/// Moves the cursor over a map position.
pub(super) fn hover(session: &mut Session, world: (f32, f32)) {
    session.pointer_move(at(session, world), VIEW);
}

#[test]
fn ctrl_c_asks_for_a_copy_of_what_is_selected() {
    let (mut session, _answers, engine) = answered();
    session.selection = vec![WALL, CHAIR];
    session.copy_selection();
    // A question and not an edit, of what is selected as it is.
    assert_eq!(one_copy(&engine).1, [WALL, CHAIR]);
    // The clipboard waits for the answer.
    assert!(session.clipboard().is_none());
}

#[test]
fn ctrl_c_with_nothing_selected_asks_nothing() {
    let (mut session, _answers, engine) = answered();
    session.copy_selection();
    session.cut_selection();
    assert_eq!(engine.try_iter().count(), 0);
    assert!(session.clipboard().is_none());
}

#[test]
fn the_clipboard_is_filled_when_the_answer_comes() {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR, WALL];
    session.copy_selection();
    let (first, _) = one_copy(&engine);
    session.copy_selection();
    let (second, _) = one_copy(&engine);

    // Only the answer to the last copy asked for counts.
    answer(&mut session, &answers, first);
    assert!(session.clipboard().is_none());
    answer(&mut session, &answers, second);
    let clipboard = session.clipboard().expect("a clipboard");
    assert_eq!((clipboard.text(), clipboard.roots(), clipboard.entities(), clipboard.from()), (TEXT, 2, 3, (1, 1)));
    assert_eq!(session.status_line(), "Copied 3 entities");
    // A copy changes nothing.
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_clipboard_outlives_the_map() {
    let (mut session, answers, _engine) = copied();
    open(&mut session, &answers);
    assert_eq!(session.clipboard().map(Clipboard::text), Some(TEXT));
    session.begin_paste();
    assert!(session.is_pasting());
}

#[test]
fn cut_deletes_only_after_the_copy_has_come() {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR, WALL, CROWBAR];
    session.cut_selection();
    // The copy alone is asked for first.
    let (seq, uids) = one_copy(&engine);
    assert_eq!(uids, [CHAIR, WALL, CROWBAR]);

    answer(&mut session, &answers, seq);
    // One edit deletes the chair and the wall. The crowbar inside the wall goes with it.
    let delete = |uid: i32| json!({ "kind": "delete", "uid": uid });
    assert_eq!(edits(&engine), [("Cut 3 entities", None, vec![delete(CHAIR), delete(WALL)])]);
    assert_eq!(session.clipboard().map(Clipboard::entities), Some(3));
}

#[test]
fn a_refused_copy_cuts_nothing() {
    let (mut session, answers, engine) = answered();
    session.selection = vec![CHAIR, WALL];
    session.cut_selection();
    let (seq, _) = one_copy(&engine);
    answers.send(FromEngine::Refused { seq, what: "Copy", error: "Nothing copied: the entities are on two grids.".into() }).ok().expect("a session to send to");
    session.pump(VIEW);

    assert_eq!(session.status_line(), "Copy: the engine refused: Nothing copied: the entities are on two grids.");
    assert!(session.clipboard().is_none());
    // Nothing waits for that copy any more.
    answer(&mut session, &answers, seq);
    assert!(session.clipboard().is_none());
    assert!(asked(&engine).is_empty());
}

#[test]
fn ctrl_v_takes_select_in_hand_and_begins_pasting() {
    let (mut session, _answers, engine) = copied();
    session.set_tool(Tool::Paint);
    session.begin_paste();
    assert_eq!((session.tool(), session.is_pasting()), (Tool::Select, true));

    // Ctrl+V again while pasting keeps the turn.
    tap_r(&mut session, false);
    session.begin_paste();
    click(&mut session, (2.5, 4.5));
    assert_eq!(quarters(&engine), [3]);
}

#[test]
fn ctrl_v_with_no_clipboard_does_nothing() {
    let (mut session, _answers, engine) = answered();
    session.set_tool(Tool::Paint);
    session.begin_paste();
    assert_eq!((session.tool(), session.is_pasting()), (Tool::Paint, false));
    assert_eq!(engine.try_iter().count(), 0);
}

#[test]
fn a_click_stamps_one_paste_op_at_the_cursor_s_cell() {
    let (mut session, _answers, engine) = copied();
    session.begin_paste();
    click(&mut session, (2.25, 4.75));
    let paste = json!({ "kind": "paste", "text": TEXT, "grid": GRID, "fromX": 1, "fromY": 1, "x": 2.0, "y": 4.0, "quarters": 0 });
    // One edit with no stroke, which is one step to undo.
    assert_eq!(edits(&engine), [("Paste 3 entities", None, vec![paste])]);
}

#[test]
fn shift_pastes_at_the_cursor_itself() {
    let (mut session, _answers, engine) = copied();
    session.begin_paste();
    session.held = SHIFT;
    click(&mut session, (2.25, 4.75));
    // The middle of the origin cell is on the cursor.
    let ops = asked(&engine).concat();
    let place = |op: &Value| ((op["x"].as_f64().unwrap() * 1000.0).round() / 1000.0, (op["y"].as_f64().unwrap() * 1000.0).round() / 1000.0);
    assert_eq!(ops.iter().map(place).collect::<Vec<_>>(), [(1.75, 4.25)]);
}

#[test]
fn the_mode_stays_for_a_second_stamp() {
    let (mut session, answers, engine) = copied();
    session.begin_paste();
    click(&mut session, (2.5, 4.5));
    let first = numbers(&engine);
    assert_eq!(first.len(), 1);

    // The engine refuses the first. The status line says why, and the mode stays.
    let error = "Nothing pasted: 2 of the 3 entities would be on a cell with no tile.";
    answers.send(FromEngine::Refused { seq: first[0], what: "Paste 3 entities", error: error.into() }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.status_line(), format!("Paste 3 entities: the engine refused: {error}"));
    assert!(session.is_pasting());

    click(&mut session, (0.5, 4.5));
    let ops = asked(&engine);
    assert_eq!(ops.len(), 1);
    assert_eq!((ops[0][0]["kind"].as_str(), ops[0][0]["x"].as_f64()), (Some("paste"), Some(0.0)));
}

#[test]
fn escape_goes_to_a_menu_then_the_dialogs_then_a_field_and_the_map_last() {
    use crate::session::{escape_to, EscapeTo};
    assert_eq!(escape_to(false, false, false), EscapeTo::Map);
    // Typed in a side panel's field, the field alone takes it, and the selection stays.
    assert_eq!(escape_to(false, false, true), EscapeTo::Field);
    // A field of a dialog: the dialog closes.
    assert_eq!(escape_to(false, true, true), EscapeTo::Dialog);
    assert_eq!(escape_to(false, true, false), EscapeTo::Dialog);
    for (dialog, typing) in [(false, false), (true, true), (false, true)] {
        assert_eq!(escape_to(true, dialog, typing), EscapeTo::Menu);
    }
}

#[test]
fn a_right_click_escape_and_a_change_of_tool_end_the_mode() {
    let (mut session, _answers, engine) = copied();
    session.selection = vec![CHAIR];

    // A right-click ends it and keeps the selection.
    session.begin_paste();
    session.pointer_down(over(&session, (2, 4)), VIEW, Button::Secondary);
    assert_eq!((session.is_pasting(), session.selection.clone()), (false, vec![CHAIR]));

    // So does Escape. Escape again clears the selection.
    session.begin_paste();
    session.escape();
    assert_eq!((session.is_pasting(), session.selection.clone()), (false, vec![CHAIR]));
    session.escape();
    assert!(session.selection.is_empty());

    // Taking Select again keeps it. Another tool ends it, and Select taken back does not begin it.
    session.begin_paste();
    session.set_tool(Tool::Select);
    assert!(session.is_pasting());
    session.set_tool(Tool::Place);
    session.set_tool(Tool::Select);
    assert!(!session.is_pasting());
    click(&mut session, (2.5, 4.5));
    assert!(asked(&engine).is_empty());
}

#[test]
fn r_turns_the_ghost_and_the_paste_a_quarter() {
    let (mut session, _answers, engine) = copied();
    session.begin_paste();
    hover(&mut session, (2.5, 4.5));
    let chair = ((2.0, 4.0, 3.0, 5.0), 0.5);
    assert_eq!(ghost(&mut session), [chair, ((4.0, 4.0, 5.0, 5.0), 0.5)]);

    // R turns with the clock: the wall, to the right of the origin cell, goes below it. The
    // engine counts that as three quarters against the clock.
    tap_r(&mut session, false);
    assert_eq!(ghost(&mut session), [chair, ((2.0, 2.0, 3.0, 3.0), 0.5)]);
    click(&mut session, (2.5, 4.5));
    assert_eq!(quarters(&engine), [3]);

    // Shift+R turns the other way, twice: the wall goes above.
    tap_r(&mut session, true);
    tap_r(&mut session, true);
    assert_eq!(ghost(&mut session), [chair, ((2.0, 6.0, 3.0, 7.0), 0.5)]);
    click(&mut session, (2.5, 4.5));
    // Nothing selected was turned: the only edit is the paste.
    assert_eq!(quarters(&engine), [1]);
}

#[test]
fn a_press_while_pasting_selects_nothing() {
    let (mut session, _answers, engine) = copied();
    hover(&mut session, (1.5, 1.5));
    assert!(session.hover_label().is_some());

    session.begin_paste();
    // The chair is under the cursor, and is not named while pasting.
    assert!(session.hover_label().is_none());
    click(&mut session, (1.5, 1.5));
    assert!(session.selection.is_empty());
    assert_eq!(quarters(&engine), [0]);
}

#[test]
fn a_stamp_off_every_grid_asks_nothing() {
    let (mut session, _answers, engine) = copied();
    session.begin_paste();
    hover(&mut session, (10.5, 10.5));
    assert!(ghost(&mut session).is_empty());
    click(&mut session, (10.5, 10.5));
    click(&mut session, (-3.5, 2.5));
    assert!(asked(&engine).is_empty());
    assert!(session.is_pasting());
}

#[test]
fn the_ghost_is_the_copied_pictures_at_the_cursor() {
    let (mut session, _answers, _engine) = copied();
    hover(&mut session, (2.25, 4.75));
    // Not before pasting.
    assert!(ghost(&mut session).is_empty());

    // The pictures as they were drawn at the copy, though the engine draws them no more.
    draw(&mut session, &[]);
    session.begin_paste();
    assert_eq!(ghost(&mut session), [((2.0, 4.0, 3.0, 5.0), 0.5), ((4.0, 4.0, 5.0, 5.0), 0.5)]);

    // With Shift, the middle of the origin cell is on the cursor.
    session.held = SHIFT;
    assert_eq!(ghost(&mut session), [((1.75, 4.25, 2.75, 5.25), 0.5), ((3.75, 4.25, 4.75, 5.25), 0.5)]);
}
