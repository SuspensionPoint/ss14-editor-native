//! The cable tool, and the path the cable and pipe tools drag out.

use super::*;

/// A session whose engine knows these prototypes, with no tool chosen for it.
pub(super) fn knowing(ids: &[&str]) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.set_prototypes(ids.iter().map(|id| Prototype { id: (*id).into(), ..Default::default() }).collect());
    (session, engine)
}

/// Puts an entity of a prototype in the mirror, on the middle of a cell of the grid.
pub(super) fn put(session: &mut Session, uid: i32, prototype: &str, cell: (i32, i32), rotation: f64) {
    let entity = Entity { uid, yaml_uid: uid, prototype: prototype.into(), parent: GRID, x: cell.0 as f32 + 0.5, y: cell.1 as f32 + 0.5, rotation, anchored: true, contained: false, name: None };
    session.doc.as_mut().unwrap().entities.insert(uid, entity);
}

/// Presses a button over the first cell, moves through the others and lets go over the last.
pub(super) fn path(session: &mut Session, button: Button, cells: &[(i32, i32)]) {
    session.pointer_down(over(session, cells[0]), VIEW, button);
    for &cell in &cells[1..] {
        session.pointer_move(over(session, cell), VIEW);
    }
    session.pointer_up(over(session, *cells.last().unwrap()), VIEW);
}

/// The prototype and the cell of each spawn op, in order.
pub(super) fn spawns(ops: &[Value]) -> Vec<(String, (f64, f64))> {
    ops.iter().filter(|op| op["kind"] == "spawn").map(|op| (op["prototype"].as_str().unwrap().to_owned(), (op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap()))).collect()
}

fn cables() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = knowing(&["CableApcExtension", "CableHV", "CableMV"]);
    session.choose_cable(CableKind::High);
    (session, engine)
}

#[test]
fn a_drag_lays_one_cable_on_each_cell_as_one_edit_when_let_go() {
    let (mut session, engine) = cables();
    assert_eq!(session.tool(), Tool::Cable);
    session.pointer_down(over(&session, (0, 2)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 2)), VIEW);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    // Nothing is asked while it is dragged.
    assert!(edits(&engine).is_empty());
    session.pointer_up(over(&session, (2, 3)), VIEW);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Lay HV cable", None));
    let at = |x: f64, y: f64| ("CableHV".to_owned(), (x, y));
    assert_eq!(spawns(&edits[0].2), [at(0.5, 2.5), at(1.5, 2.5), at(2.5, 2.5), at(2.5, 3.5)]);
    assert_eq!(edits[0].2[0], json!({ "kind": "spawn", "prototype": "CableHV", "parent": GRID, "x": 0.5, "y": 2.5, "rotation": 0.0 }));
}

#[test]
fn a_cell_that_has_the_cable_or_no_tile_is_skipped() {
    let (mut session, engine) = cables();
    put(&mut session, 50, "CableHV", (1, 4), 0.0);
    // Another kind on a cell does not count.
    put(&mut session, 51, "CableMV", (2, 4), 0.0);
    // From the floor out over space, and back over a cell crossed before.
    path(&mut session, Button::Primary, &[(3, 4), (0, 4), (-2, 4), (3, 4)]);

    let ops = asked(&engine);
    assert_eq!(ops.len(), 1);
    let cells: Vec<(f64, f64)> = spawns(&ops[0]).into_iter().map(|(_, at)| at).collect();
    assert_eq!(cells, [(3.5, 4.5), (2.5, 4.5), (0.5, 4.5)]);
}

#[test]
fn a_fast_drag_leaves_no_gap_and_no_corner_cut() {
    let (mut session, engine) = cables();
    // One move from corner to corner, as a fast hand gives it.
    path(&mut session, Button::Primary, &[(0, 0), (3, 3)]);

    let cells: Vec<(f64, f64)> = spawns(&asked(&engine)[0]).into_iter().map(|(_, at)| at).collect();
    assert_eq!(cells.len(), 7);
    // Each cell is next to the one before by a side, so the cables join.
    assert!(cells.windows(2).all(|pair| (pair[0].0 - pair[1].0).abs() + (pair[0].1 - pair[1].1).abs() == 1.0), "{cells:?}");
}

#[test]
fn a_right_click_or_drag_removes_that_kind_of_cable() {
    let (mut session, engine) = cables();
    put(&mut session, 50, "CableHV", (1, 4), 0.0);
    put(&mut session, 51, "CableMV", (1, 4), 0.0);
    put(&mut session, 52, "CableHV", (2, 4), 0.0);
    put(&mut session, 53, "CableHV", (4, 4), 0.0);
    path(&mut session, Button::Secondary, &[(1, 4)]);
    path(&mut session, Button::Secondary, &[(2, 4), (5, 4)]);

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Remove HV cable"; 2]);
    assert_eq!(edits[0].2, [json!({ "kind": "delete", "uid": 50 })]);
    assert_eq!(edits[1].2, [json!({ "kind": "delete", "uid": 52 }), json!({ "kind": "delete", "uid": 53 })]);
    assert!(edits.iter().all(|edit| edit.1.is_none()));

    // The medium cable goes with that kind chosen.
    session.choose_cable(CableKind::Medium);
    path(&mut session, Button::Secondary, &[(1, 4)]);
    assert_eq!(asked(&engine), [[json!({ "kind": "delete", "uid": 51 })]]);
}

#[test]
fn the_path_is_shown_while_it_is_dragged() {
    let (mut session, _engine) = cables();
    session.pointer_down(over(&session, (0, 2)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 2)), VIEW);
    session.pointer_move(over(&session, (0, 2)), VIEW);
    // Each cell once, in the cable's colour.
    let (_, over_map) = session.overlay_quads();
    assert_eq!(over_map.len(), 4 * FLOATS_PER_QUAD);
    assert_eq!(over_map[8..11], [1.0, 0x88 as f32 / 255.0, 0.0]);
    // The cell under the cursor keeps its outline.
    assert_eq!(outline(&session).0, OutlineStyle::Hover);

    session.pointer_up(over(&session, (0, 2)), VIEW);
    assert!(session.overlay_quads().1.is_empty());

    // A path that removes is red.
    session.pointer_down(over(&session, (0, 2)), VIEW, Button::Secondary);
    assert_eq!(session.overlay_quads().1[8..11], [1.0, 0.25, 0.25]);
}

#[test]
fn a_path_is_labelled_with_its_cells_on_a_tile_by_the_cursor() {
    let (mut session, _engine) = cables();
    assert_eq!(session.drag_label(), None);
    session.pointer_down(over(&session, (0, 4)), VIEW, Button::Primary);
    assert_eq!(session.drag_label(), Some(((0.5, 5.0), "1".to_owned())));
    // Out over space and back over cells crossed before: those count once, space not at all.
    session.pointer_move(over(&session, (3, 4)), VIEW);
    session.pointer_move(over(&session, (-2, 4)), VIEW);
    assert_eq!(session.drag_label(), Some(((-1.5, 5.0), "4".to_owned())));
    session.pointer_up(over(&session, (-2, 4)), VIEW);
    assert_eq!(session.drag_label(), None);

    // The pipe tool's path too, and one that removes.
    session.set_tool(Tool::Pipe);
    session.pointer_down(over(&session, (1, 2)), VIEW, Button::Secondary);
    session.pointer_move(over(&session, (1, 3)), VIEW);
    assert_eq!(session.drag_label(), Some(((1.5, 4.0), "2".to_owned())));
}

#[test]
fn a_kind_the_fork_lacks_lays_nothing_and_says_so() {
    let (mut session, engine) = knowing(&["CableHV"]);
    assert!(session.cable_available(CableKind::High) && !session.cable_available(CableKind::Apc));
    session.choose_cable(CableKind::Apc);
    path(&mut session, Button::Primary, &[(0, 2), (3, 2)]);

    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not laid: the fork has no CableApcExtension");
}

#[test]
fn k_and_j_take_the_cable_and_pipe_tools() {
    let (mut session, _engine) = Session::offline(&scene());
    assert!(session.key_down('k', false));
    assert_eq!(session.tool(), Tool::Cable);
    assert!(session.key_down('J', false));
    assert_eq!(session.tool(), Tool::Pipe);
}

#[test]
fn a_path_steps_from_cell_to_cell_by_their_sides() {
    use crate::session::tools::infra::side_steps;
    assert_eq!(side_steps((0, 0), (3, 0)), [(0, 0), (1, 0), (2, 0), (3, 0)]);
    // Across a corner, the cell beside it is put in.
    assert_eq!(side_steps((0, 0), (2, 2)), [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)]);
    assert_eq!(side_steps((0, 0), (0, 0)), [(0, 0)]);
    for (from, to) in [((0, 0), (5, -3)), ((2, 7), (-4, 1)), ((0, 0), (1, 6))] {
        let steps = side_steps(from, to);
        assert_eq!((steps[0], *steps.last().unwrap()), (from, to));
        assert!(steps.windows(2).all(|pair| (pair[0].0 - pair[1].0).abs() + (pair[0].1 - pair[1].1).abs() == 1), "{steps:?}");
    }
}
