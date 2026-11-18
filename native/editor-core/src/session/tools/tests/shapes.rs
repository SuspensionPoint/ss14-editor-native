//! The line and circle tools.

use super::super::shapes::{disc_cells, line_cells};
use super::*;

/// Where spawn ops put their entity, in the order of the ops.
fn spawned_at(ops: &[Value]) -> Vec<(f64, f64)> {
    assert!(ops.iter().all(|op| op["kind"] == "spawn" && op["prototype"] == "Table"), "{ops:?}");
    ops.iter().map(|op| (op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect()
}

#[test]
fn a_line_is_the_web_editor_s() {
    // Flat, steep, the four diagonal ways and a single cell.
    assert_eq!(line_cells((0, 0), (3, 0)), [(0, 0), (1, 0), (2, 0), (3, 0)]);
    assert_eq!(line_cells((0, 0), (1, 3)), [(0, 0), (0, 1), (1, 2), (1, 3)]);
    assert_eq!(line_cells((0, 0), (2, 2)), [(0, 0), (1, 1), (2, 2)]);
    assert_eq!(line_cells((0, 0), (-2, 2)), [(0, 0), (-1, 1), (-2, 2)]);
    assert_eq!(line_cells((0, 0), (2, -2)), [(0, 0), (1, -1), (2, -2)]);
    assert_eq!(line_cells((0, 0), (-2, -2)), [(0, 0), (-1, -1), (-2, -2)]);
    assert_eq!(line_cells((4, 7), (4, 7)), [(4, 7)]);
    // As `bresenham` in `lineTool.ts` steps it, cell by cell.
    assert_eq!(line_cells((0, 0), (5, 2)), [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)]);
}

#[test]
fn a_disc_is_the_web_editor_s() {
    assert_eq!(disc_cells((3, 4), 0), [(3, 4)]);
    assert_eq!(disc_cells((0, 0), 1), [(0, -1), (-1, 0), (0, 0), (1, 0), (0, 1)]);
    assert_eq!(
        disc_cells((0, 0), 2),
        [(0, -2), (-1, -1), (0, -1), (1, -1), (-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)]
    );
    assert_eq!(disc_cells((0, 0), 3).len(), 29);
}

#[test]
fn a_line_of_tiles_is_one_edit_from_end_to_end() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Line);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (5, 2)), VIEW);
    // Nothing is asked for while it is dragged, and its cells are filled over the map.
    assert!(asked(&engine).is_empty());
    assert_eq!(session.overlay_quads().1.len() / FLOATS_PER_QUAD, 6);
    session.pointer_up(over(&session, (5, 2)), VIEW);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Line of tiles", None));
    assert_eq!(cells(&edits[0].2), [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)]);
    assert!(edits[0].2.iter().all(|op| op["tile"] == "FloorWood"));
    assert!(session.overlay_quads().1.is_empty());
}

#[test]
fn a_circle_of_tiles_takes_its_radius_from_the_cursor_s_cell() {
    // One across and one up is the square root of two away, which rounds to 1. Two across and
    // one up is the square root of five away, which rounds to 2. Three across is 3.
    for (to, radius) in [((2, 2), 0), ((3, 3), 1), ((4, 3), 2), ((5, 2), 3)] {
        let (mut session, engine) = Session::offline(&scene());
        session.choose_tile("FloorWood");
        session.set_tool(Tool::Circle);
        drag(&mut session, (2, 2), to);

        let edits = edits(&engine);
        assert_eq!(edits.len(), 1, "{to:?}");
        assert_eq!((edits[0].0, edits[0].1), ("Circle of tiles", None), "{to:?}");
        let wanted: Vec<(i64, i64)> = disc_cells((2, 2), radius).into_iter().map(|(x, y)| (i64::from(x), i64::from(y))).collect();
        assert_eq!(cells(&edits[0].2), wanted, "{to:?}");
    }
}

#[test]
fn a_line_and_a_circle_put_down_entities_when_one_is_chosen() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Line);
    choose(&mut session, "SnapgridCenter");
    assert_eq!(session.tool(), Tool::Line);
    drag(&mut session, (0, 0), (5, 0));
    session.set_tool(Tool::Circle);
    // A radius of 1 about (0, 4). The cell left of it has no tile.
    drag(&mut session, (0, 4), (1, 4));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Line of entities", "Circle of entities"]);
    assert!(edits.iter().all(|edit| edit.1.is_none()));
    assert_eq!(spawned_at(&edits[0].2), [(0.5, 0.5), (1.5, 0.5), (2.5, 0.5), (3.5, 0.5), (4.5, 0.5), (5.5, 0.5)]);
    assert_eq!(spawned_at(&edits[1].2), [(0.5, 3.5), (0.5, 4.5), (1.5, 4.5), (0.5, 5.5)]);
}

#[test]
fn a_shape_of_more_than_ten_thousand_entities_is_refused_by_its_name() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Line);
    drag(&mut session, (0, 0), (10_000, 0));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not placed: the line has more than 10,000 cells");

    session.set_tool(Tool::Circle);
    drag(&mut session, (0, 0), (60, 0));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not placed: the circle has more than 10,000 cells");
}

#[test]
fn a_shape_keeps_the_brush_it_was_pressed_with() {
    for (tool, entities, tiles) in [(Tool::Line, "Line of entities", "Line of tiles"), (Tool::Circle, "Circle of entities", "Circle of tiles")] {
        let (mut session, engine) = Session::offline(&scene());
        choose(&mut session, "SnapgridCenter");
        session.set_tool(tool);
        session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
        session.pointer_move(over(&session, (1, 0)), VIEW);
        session.choose_tile("FloorWood");
        session.pointer_up(over(&session, (1, 0)), VIEW);
        assert_eq!(labels(&edits(&engine)), [entities], "{tool:?}");

        // And the other way about.
        session.pointer_down(over(&session, (2, 4)), VIEW, Button::Primary);
        session.pointer_move(over(&session, (3, 4)), VIEW);
        session.choose_entity(0);
        assert_eq!(session.tool(), tool);
        session.pointer_up(over(&session, (3, 4)), VIEW);
        assert_eq!(labels(&edits(&engine)), [tiles], "{tool:?}");
    }
}

#[test]
fn a_shape_let_go_off_the_map_asks_nothing() {
    for tool in [Tool::Line, Tool::Circle] {
        let (mut session, engine) = Session::offline(&scene());
        session.choose_tile("FloorWood");
        session.set_tool(tool);
        session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
        session.pointer_move(over(&session, (3, 2)), VIEW);
        // The cursor leaves the map with the button down, and the button comes up out there.
        session.pointer_left();
        session.pointer_up(over(&session, (4, 4)), VIEW);

        assert!(asked(&engine).is_empty(), "{tool:?}");
        assert!(session.overlay_quads().1.is_empty(), "{tool:?}");
        assert_eq!(session.drag_label(), None, "{tool:?}");
    }
}

#[test]
fn a_line_is_labelled_with_its_length_and_a_circle_with_its_radius() {
    let (mut session, _engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Line);
    session.pointer_move(over(&session, (1, 1)), VIEW);
    assert_eq!(session.drag_label(), None);

    // Three cells, said over the middle of the top edge of the cursor's cell.
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 2)), VIEW);
    assert_eq!(session.drag_label(), Some(((3.5, 3.0), "3".to_owned())));
    // The cell pressed and the cell it ends on are outlined.
    let ends: Vec<(OutlineStyle, (f32, f32))> = session.outlines().iter().map(|outline| (outline.style, outline.corners[0])).collect();
    assert_eq!(ends, [(OutlineStyle::Line, (1.0, 1.0)), (OutlineStyle::Line, (3.0, 2.0))]);
    session.pointer_up(over(&session, (3, 2)), VIEW);
    assert_eq!(session.drag_label(), None);

    session.set_tool(Tool::Circle);
    session.pointer_down(over(&session, (2, 2)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (4, 3)), VIEW);
    assert_eq!(session.drag_label(), Some(((4.5, 4.0), "r=2".to_owned())));
    // The centre alone is outlined.
    assert_eq!(outline(&session), (OutlineStyle::Circle, [(2.0, 2.0), (3.0, 2.0), (3.0, 3.0), (2.0, 3.0)]));
}

#[test]
fn l_and_c_choose_the_tools() {
    let (mut session, _engine) = Session::offline(&scene());
    assert!(session.key_down('L', false));
    assert_eq!(session.tool(), Tool::Line);
    assert!(session.key_down('c', false));
    assert_eq!(session.tool(), Tool::Circle);
}
