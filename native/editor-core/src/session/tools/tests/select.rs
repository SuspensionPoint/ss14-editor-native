//! The select tool.

use super::*;

#[test]
fn a_click_selects_the_entity_under_it_and_a_click_on_nothing_selects_nothing() {
    let (mut session, engine) = selecting();
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    click(&mut session, (0.5, 4.5));
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_shift_click_adds_to_the_selection_or_takes_away_from_it() {
    let (mut session, engine) = selecting();
    click(&mut session, (1.5, 1.5));
    session.held.shift = true;
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.selection, [CHAIR, WALL]);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_press_that_moves_four_pixels_is_a_click_and_moves_nothing() {
    let (mut session, engine) = selecting();
    let pressed = at(&session, (1.5, 1.5));
    session.pointer_down(pressed, VIEW, Button::Primary);
    session.pointer_move((pressed.0 + 4.0, pressed.1), VIEW);
    session.pointer_up((pressed.0 + 4.0, pressed.1), VIEW);

    assert_eq!(session.selection, [CHAIR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_move_begins_past_four_pixels_and_then_follows_the_cursor_anywhere() {
    let (mut session, engine) = selecting();
    // With Shift the chair goes by the cursor, so that every pixel of the way shows. It is
    // selected already, since a press with Shift on one that is not only adds it.
    session.selection = vec![CHAIR];
    session.held.shift = true;
    let pressed = at(&session, (1.5, 1.5));
    session.pointer_down(pressed, VIEW, Button::Primary);
    session.pointer_move((pressed.0 + 4.0, pressed.1), VIEW);
    assert!(asked(&engine).is_empty());
    session.pointer_move((pressed.0 + 5.0, pressed.1), VIEW);
    session.pointer_move(pressed, VIEW);
    session.pointer_move((pressed.0 + 1.0, pressed.1), VIEW);
    session.pointer_up((pressed.0 + 1.0, pressed.1), VIEW);

    let xs: Vec<f64> = asked(&engine).iter().map(|ops| ops[0]["x"].as_f64().unwrap()).collect();
    assert_eq!(xs, [1.65625, 1.5, 1.53125]);
}

#[test]
fn a_drag_moves_the_selected_entity_from_where_it_stood() {
    let (mut session, engine) = selecting();
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    // A cell and a quarter across and a quarter up. Then on, to a cell and three quarters
    // across and a cell and a quarter up.
    session.pointer_move(at(&session, (2.75, 1.75)), VIEW);
    session.pointer_move(at(&session, (3.25, 2.75)), VIEW);
    session.pointer_up(at(&session, (3.25, 2.75)), VIEW);

    // Each states the whole way from where the chair stood, and not the way since the last.
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Move"; 2]);
    assert!(one_stroke(&edits));
    assert_eq!(edits[0].2, [json!({ "kind": "move", "uid": CHAIR, "x": 2.5, "y": 1.5 })]);
    assert_eq!(edits[1].2, [json!({ "kind": "move", "uid": CHAIR, "x": 3.5, "y": 2.5 })]);
}

#[test]
fn a_move_goes_by_whole_cells() {
    let (mut session, engine) = selecting();
    // The chair stands loose, and off the middle of its cell.
    let chair = session.doc.as_mut().unwrap().entities.get_mut(&CHAIR).unwrap();
    (chair.x, chair.y) = (1.25, 1.75);
    draw(&mut session, &[(CHAIR, (1.25, 1.75), 0)]);

    // Seven sixteenths of a cell across. Then on, to a cell and five sixteenths across and a
    // quarter down.
    session.pointer_down(at(&session, (1.25, 1.75)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (1.6875, 1.75)), VIEW);
    session.pointer_move(at(&session, (2.5625, 1.5)), VIEW);
    session.pointer_up(at(&session, (2.5625, 1.5)), VIEW);

    // Short of half a cell it stays. Then it goes one cell across, and stands as far off the
    // middle of that cell as it stood off the middle of its own.
    let stays = [json!({ "kind": "move", "uid": CHAIR, "x": 1.25, "y": 1.75 })];
    let goes = [json!({ "kind": "move", "uid": CHAIR, "x": 2.25, "y": 1.75 })];
    assert_eq!(asked(&engine), [stays, goes]);
}

#[test]
fn a_move_takes_every_selected_entity_by_the_same_whole_cells() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    // Three quarters of a cell across and a quarter up.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    session.pointer_up(at(&session, (2.25, 1.75)), VIEW);

    let moves = [json!({ "kind": "move", "uid": CHAIR, "x": 2.5, "y": 1.5 }), json!({ "kind": "move", "uid": WALL, "x": 4.5, "y": 1.5 })];
    assert_eq!(asked(&engine), [moves]);
}

#[test]
fn shift_frees_a_move_of_what_is_not_anchored() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.held.shift = true;
    // Pressed on the chair, which is selected, and dragged three quarters of a cell across
    // and a quarter up.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    session.pointer_up(at(&session, (2.25, 1.75)), VIEW);

    assert_eq!(asked(&engine), [[json!({ "kind": "move", "uid": CHAIR, "x": 2.25, "y": 1.75 })]]);
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn shift_does_not_free_what_is_anchored() {
    let (mut session, engine) = selecting();
    session.selection = vec![WALL];
    session.held.shift = true;
    // Three quarters of a cell across and a quarter up.
    session.pointer_down(at(&session, (3.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.25, 1.75)), VIEW);
    session.pointer_up(at(&session, (4.25, 1.75)), VIEW);

    assert_eq!(asked(&engine), [[json!({ "kind": "move", "uid": WALL, "x": 4.5, "y": 1.5 })]]);
}

#[test]
fn an_anchored_entity_keeps_the_whole_selection_on_cells_with_shift_held() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.held.shift = true;
    // Pressed on the chair, which is loose, and dragged three quarters of a cell across and
    // a quarter up. The chair goes as the wall has to, so that the two stay as they stood.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    session.pointer_up(at(&session, (2.25, 1.75)), VIEW);

    let moves = [json!({ "kind": "move", "uid": CHAIR, "x": 2.5, "y": 1.5 }), json!({ "kind": "move", "uid": WALL, "x": 4.5, "y": 1.5 })];
    assert_eq!(asked(&engine), [moves]);
    assert_eq!(session.selection, [CHAIR, WALL]);
}

#[test]
fn shift_let_go_mid_drag_snaps_the_move_back_to_cells() {
    let (mut session, engine) = selecting();
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    // Three quarters of a cell across and a quarter up, which is one cell across.
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    // Shift goes down, and the next move goes by the cursor.
    session.held.shift = true;
    session.pointer_move(at(&session, (2.3125, 1.8125)), VIEW);
    // Shift is let go, and the next move is by whole cells again from where the chair stood.
    session.held.shift = false;
    session.pointer_move(at(&session, (2.375, 1.8125)), VIEW);
    session.pointer_up(at(&session, (2.375, 1.8125)), VIEW);

    let edits = edits(&engine);
    assert!(one_stroke(&edits));
    let places: Vec<(f64, f64)> = edits.iter().map(|edit| (edit.2[0]["x"].as_f64().unwrap(), edit.2[0]["y"].as_f64().unwrap())).collect();
    assert_eq!(places, [(2.5, 1.5), (2.3125, 1.8125), (2.5, 1.5)]);
}

#[test]
fn a_shift_drag_on_a_selected_entity_moves_it_and_does_not_take_it_out_of_the_selection() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.held.shift = true;
    // Dragged away, and back to where it was pressed. It is let go there, as a click would be.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    session.pointer_move(at(&session, (1.5, 1.5)), VIEW);
    session.pointer_up(at(&session, (1.5, 1.5)), VIEW);

    let away = [json!({ "kind": "move", "uid": CHAIR, "x": 2.25, "y": 1.75 })];
    let back = [json!({ "kind": "move", "uid": CHAIR, "x": 1.5, "y": 1.5 })];
    assert_eq!(asked(&engine), [away, back]);
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn a_shift_click_on_a_selected_entity_still_takes_it_out() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.held.shift = true;
    // A press that moves four pixels is a click.
    let pressed = at(&session, (1.5, 1.5));
    session.pointer_down(pressed, VIEW, Button::Primary);
    session.pointer_move((pressed.0 + 4.0, pressed.1), VIEW);
    session.pointer_up((pressed.0 + 4.0, pressed.1), VIEW);

    assert_eq!(session.selection, [WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_shift_press_on_an_entity_that_is_not_selected_adds_it_and_moves_nothing() {
    let (mut session, engine) = selecting();
    // The wall is loose here, so that with Shift a move would go by the cursor.
    session.doc.as_mut().unwrap().entities.get_mut(&WALL).unwrap().anchored = false;
    session.selection = vec![WALL];
    session.held.shift = true;
    // Pressed on the chair and dragged three quarters of a cell across and a quarter up.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    assert_eq!(session.selection, [WALL, CHAIR]);
    // The press began no drag: neither a move nor a box.
    assert!(matches!(session.drag, Drag::None));
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    assert!(session.outlines().is_empty());
    session.pointer_up(at(&session, (2.25, 1.75)), VIEW);

    assert!(asked(&engine).is_empty());
    assert_eq!(session.selection, [WALL, CHAIR]);
}

#[test]
fn a_plain_click_on_one_of_several_selected_leaves_the_selection_as_it_was() {
    let (mut session, engine) = selecting();
    session.selection = vec![WALL, CHAIR];
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL, CHAIR]);
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.selection, [WALL, CHAIR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_box_being_dragged_is_the_only_outline_of_the_select_tool() {
    let (mut session, _engine) = selecting();
    session.pointer_move(at(&session, (0.5, 0.5)), VIEW);
    assert!(session.outlines().is_empty());

    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(outline(&session), (OutlineStyle::Drag, [(0.5, 0.5), (4.5, 0.5), (4.5, 2.5), (0.5, 2.5)]));
}

#[test]
fn choosing_an_entity_ends_a_drag_where_it_takes_the_place_tool_in_hand() {
    let (mut session, engine) = selecting();
    session.set_tool(Tool::Area);
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    choose(&mut session, "SnapgridCenter");
    assert_eq!(session.tool(), Tool::Place);
    // The box is gone. What is drawn is the place tool's outline of the cell under the cursor.
    assert_eq!(outline(&session), (OutlineStyle::Hover, [(4.0, 2.0), (5.0, 2.0), (5.0, 3.0), (4.0, 3.0)]));

    // The button is still down, and what follows is a bare hover and a release that comes late.
    session.pointer_move(at(&session, (4.5, 3.5)), VIEW);
    session.pointer_up(at(&session, (4.5, 3.5)), VIEW);
    assert!(session.region.is_none());
    assert!(asked(&engine).is_empty());

    // The select tool stays in hand, and its box goes on.
    let (mut session, _engine) = selecting();
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    choose(&mut session, "SnapgridCenter");
    assert_eq!(session.tool(), Tool::Select);
    assert_eq!(outline(&session), (OutlineStyle::Drag, [(0.5, 0.5), (4.5, 0.5), (4.5, 2.5), (0.5, 2.5)]));
}

#[test]
fn a_box_dragged_over_entities_selects_them() {
    let (mut session, engine) = selecting();
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR, WALL]);

    // A box over the chair alone takes its place, and with shift adds to it.
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (2.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR]);
    session.held.shift = true;
    session.pointer_down(at(&session, (2.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR, WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_thing_on_a_hidden_layer_cannot_be_clicked() {
    let (mut session, engine) = selecting();
    // The chair, and over it in the same place the wall, drawn at a depth the game gives doors.
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (1.5, 1.5), 9)]);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);

    // With the layer of doors hidden the click goes through to the chair under it.
    session.layers.doors = false;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);
    // With the chair's layer hidden as well there is nothing there to click.
    session.layers.objects = false;
    click(&mut session, (1.5, 1.5));
    assert!(session.selection.is_empty());
    session.layers.doors = true;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);

    // What the engine says lives under the floor is on the SubFloor layer whatever its depth.
    session.layers = Layers::default();
    draw_kinds(&mut session, &[(CHAIR, (1.5, 1.5), 0, KIND_ENTITY), (WALL, (1.5, 1.5), 9, KIND_SUBFLOOR)]);
    session.layers.doors = false;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    session.layers.subfloor = false;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn plumbing_is_a_layer_of_its_own() {
    // A duct under the floor, which the engine says is plumbing, over a cable it does not.
    let (mut session, engine) = selecting();
    draw_kinds(&mut session, &[(CHAIR, (1.5, 1.5), -14, KIND_SUBFLOOR), (WALL, (1.5, 1.5), -13, KIND_PLUMBING)]);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    let both = label_at(&mut session, (1.5, 1.5)).expect("a label");
    assert!(both.ends_with("(+1 more)"), "{both}");

    // The SubFloor switch leaves it, as it has a switch of its own.
    session.layers.subfloor = false;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    session.layers.subfloor = true;

    // Off, it is hidden, and clicks and the mouse go through it to what is under it.
    session.layers.plumbing = false;
    assert_eq!(session.layers.hidden_mask(), 1 << group::PLUMBING);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);
    let alone = label_at(&mut session, (1.5, 1.5)).expect("a label");
    assert!(!alone.contains("more"), "{alone}");

    // On again, it is there to click.
    session.layers.plumbing = true;
    assert_eq!(session.layers.hidden_mask(), 0);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn disposals_is_a_layer_of_its_own() {
    // A disposal tube under the floor, over a cable.
    let (mut session, engine) = selecting();
    draw_kinds(&mut session, &[(CHAIR, (1.5, 1.5), -14, KIND_SUBFLOOR), (WALL, (1.5, 1.5), -13, KIND_DISPOSALS)]);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);

    // The SubFloor and Plumbing switches leave it.
    session.layers.subfloor = false;
    session.layers.plumbing = false;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    session.layers.subfloor = true;
    session.layers.plumbing = true;

    // Off, it is hidden, and clicks go through it.
    session.layers.disposals = false;
    assert_eq!(session.layers.hidden_mask(), 1 << group::DISPOSALS);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);
    let alone = label_at(&mut session, (1.5, 1.5)).expect("a label");
    assert!(!alone.contains("more"), "{alone}");
    assert!(asked(&engine).is_empty());
}

#[test]
fn show_entities_off_hides_every_entity_and_not_the_decals() {
    let (mut session, engine) = selecting();
    draw_kinds(&mut session, &[(CHAIR, (1.5, 1.5), 0, KIND_ENTITY), (WALL, (1.5, 1.5), 9, KIND_SUBFLOOR)]);
    session.layers.entities = false;
    let entity_groups = [group::SUBFLOOR, group::FLOOR_OBJECTS, group::STRUCTURES, group::OBJECTS, group::DOORS, group::MARKERS, group::PLUMBING, group::DISPOSALS];
    assert_eq!(session.layers.hidden_mask(), entity_groups.iter().fold(0, |mask, group| mask | 1 << group));
    // Nothing is there to click, whatever the layers' own switches say.
    click(&mut session, (1.5, 1.5));
    assert!(session.selection.is_empty());

    session.layers.entities = true;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    assert_eq!(session.layers.hidden_mask(), 0);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_box_leaves_out_what_is_on_a_hidden_layer() {
    let (mut session, engine) = selecting();
    // The wall where it stands, drawn at a depth the game gives structures.
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), -2)]);
    session.layers.structures = false;
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR]);

    session.layers.structures = true;
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR, WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_ctrl_click_takes_an_entity_out_of_the_selection_and_begins_no_move() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.held.ctrl = true;
    // Pressed on the chair and dragged away from it, which without Ctrl would move both.
    session.pointer_down(at(&session, (1.25, 1.25)), VIEW, Button::Primary);
    assert_eq!(session.selection, [WALL]);
    session.pointer_move(at(&session, (0.25, 3.25)), VIEW);
    assert_eq!(outline(&session).0, OutlineStyle::DragTakeAway);
    session.pointer_up(at(&session, (0.25, 3.25)), VIEW);
    assert_eq!(session.selection, [WALL]);

    // The chair is not selected now, so a click on it has nothing to take away.
    click(&mut session, (1.25, 1.25));
    assert_eq!(session.selection, [WALL]);
    // Nor has a click on empty floor, where a plain click would select nothing.
    click(&mut session, (0.5, 4.5));
    assert_eq!(session.selection, [WALL]);
    click(&mut session, (3.25, 1.25));
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_ctrl_box_takes_its_entities_out_and_leaves_the_rest() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.held.ctrl = true;
    // A box over the wall alone.
    session.pointer_down(at(&session, (2.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [CHAIR]);

    // A box over both, of which only the chair is selected. Ctrl is let go before the mouse
    // is, and the box still does what it was begun for.
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
    session.held.ctrl = false;
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_box_of_no_size_finds_nothing() {
    let (mut session, engine) = selecting();
    // The chair, and over it the wall, with their pictures on the same middle.
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (1.5, 1.5), 9)]);
    session.selection = vec![CHAIR, WALL];
    // A Ctrl+click exactly on that middle takes out the wall, which is the one under the
    // press. The box it begins has no size, and takes out nothing more.
    session.held.ctrl = true;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR]);

    // A click on nothing is a box of no size as well. With Shift it adds nothing, and a
    // plain one still selects nothing.
    session.held = Held { shift: true, ..Held::default() };
    click(&mut session, (0.5, 4.5));
    assert_eq!(session.selection, [CHAIR]);
    session.held = Held::default();
    click(&mut session, (0.5, 4.5));
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_ctrl_box_leaves_in_what_is_on_a_hidden_layer() {
    let (mut session, engine) = selecting();
    // The wall where it stands, drawn at a depth the game gives structures.
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), -2)]);
    session.selection = vec![CHAIR, WALL];
    session.layers.structures = false;
    session.held.ctrl = true;
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert_eq!(session.selection, [WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn ctrl_with_shift_takes_away() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.held = Held { shift: true, ctrl: true, ..Held::default() };
    // A click on the wall, which Shift alone would add.
    click(&mut session, (3.25, 1.25));
    assert_eq!(session.selection, [CHAIR]);
    // A click on the chair takes it out, and one more does not put it back.
    click(&mut session, (1.25, 1.25));
    assert!(session.selection.is_empty());
    click(&mut session, (1.25, 1.25));
    assert!(session.selection.is_empty());

    // A box over both, which Shift alone would add the chair with.
    session.selection = vec![WALL];
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
    session.pointer_up(at(&session, (4.5, 2.5)), VIEW);
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_box_says_whether_it_adds_or_takes_away() {
    let shift = Held { shift: true, ..Held::default() };
    let ctrl = Held { ctrl: true, ..Held::default() };
    let both = Held { shift: true, ctrl: true, ..Held::default() };
    for (held, style) in [(Held::default(), OutlineStyle::Drag), (shift, OutlineStyle::DragAdd), (ctrl, OutlineStyle::DragTakeAway), (both, OutlineStyle::DragTakeAway)] {
        let (mut session, _engine) = selecting();
        session.held = held;
        session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Primary);
        session.pointer_move(at(&session, (4.5, 2.5)), VIEW);
        assert_eq!(outline(&session), (style, [(0.5, 0.5), (4.5, 0.5), (4.5, 2.5), (0.5, 2.5)]), "{held:?}");

        // It is what it was begun as, whatever is held after the press.
        session.held = if held == both { Held::default() } else { both };
        session.pointer_move(at(&session, (4.5, 3.5)), VIEW);
        assert_eq!(outline(&session), (style, [(0.5, 0.5), (4.5, 0.5), (4.5, 3.5), (0.5, 3.5)]), "{held:?}");
    }
}

#[test]
fn a_right_click_with_the_select_tool_clears_the_selection() {
    // Pressed on the wall, which is selected, on the chair, which is not, and on empty floor.
    for pressed in [(3.5, 1.5), (1.5, 1.5), (0.5, 4.5)] {
        let (mut session, engine) = selecting();
        session.selection = vec![WALL];
        session.pointer_down(at(&session, pressed), VIEW, Button::Secondary);
        assert!(session.selection.is_empty(), "{pressed:?}");

        // It begins no drag: the mouse moved with the button down draws no box.
        session.pointer_move(at(&session, (pressed.0 + 1.0, pressed.1 + 1.0)), VIEW);
        assert!(matches!(session.drag, Drag::None), "{pressed:?}");
        assert!(session.outlines().is_empty(), "{pressed:?}");
        session.pointer_up(at(&session, (pressed.0 + 1.0, pressed.1 + 1.0)), VIEW);
        assert!(session.selection.is_empty(), "{pressed:?}");
        assert!(asked(&engine).is_empty(), "{pressed:?}");
    }
}

#[test]
fn a_right_click_with_no_document_does_nothing() {
    let (mut session, engine) = selecting();
    // A selection with no document to be of, which no session comes to by itself.
    session.selection = vec![CHAIR];
    session.doc = None;
    session.pointer_down(at(&session, (0.5, 4.5)), VIEW, Button::Secondary);
    session.pointer_up(at(&session, (0.5, 4.5)), VIEW);
    assert_eq!(session.selection, [CHAIR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn r_with_the_wheel_turns_the_selection_by_five_degrees_a_notch() {
    let (mut session, engine) = selecting();
    // The chair faces a quarter turn round, and the wall stands at 0.
    session.doc.as_mut().unwrap().entities.get_mut(&CHAIR).unwrap().rotation = std::f64::consts::FRAC_PI_2;
    session.selection = vec![CHAIR, WALL];
    session.key_down('R', false);

    // A notch towards the user is clockwise, which takes five degrees off each one's angle.
    wheel(&mut session, -1.0);
    let towards = edits(&engine);
    assert_eq!(labels(&towards), ["Rotate"]);
    assert_eq!(turns(&towards[0].2), [(CHAIR, 85.0), (WALL, 355.0)]);
    // Two notches away from the user are ten degrees the other way, in one edit.
    wheel(&mut session, 2.0);
    let away = edits(&engine);
    assert_eq!(labels(&away), ["Rotate"]);
    assert_eq!(turns(&away[0].2), [(CHAIR, 95.0), (WALL, 5.0)]);
}

#[test]
fn a_notch_that_comes_in_parts_is_one_step() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    // The turn is asked for once, when more than half of the notch is here.
    wheel(&mut session, -0.3);
    assert!(asked(&engine).is_empty());
    wheel(&mut session, -0.3);
    wheel(&mut session, -0.4);
    assert_eq!(asked(&engine).iter().map(|ops| turns(ops)).collect::<Vec<_>>(), [[(CHAIR, 355.0)]]);

    // A second notch in other parts is one more turn of five degrees, and never a part of it.
    for part in [-0.5, -0.25, -0.25] {
        wheel(&mut session, part);
    }
    assert_eq!(asked(&engine).iter().map(|ops| turns(ops)).collect::<Vec<_>>(), [[(CHAIR, 350.0)]]);
}

#[test]
fn the_turns_of_one_hold_of_r_are_one_step_to_undo() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    for _ in 0..3 {
        wheel(&mut session, -1.0);
    }
    session.key_up('R');
    // Each is of one stroke, and may be passed over for a later one of it.
    let first = strokes(&engine);
    assert_eq!(first.len(), 3);
    assert!(first[0].0.is_some() && first.iter().all(|&edit| edit == (first[0].0, true)), "{first:?}");

    // Another hold is another step.
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    session.key_up('R');
    let second = strokes(&engine);
    assert_eq!(second.len(), 1);
    assert!(second[0].0.is_some() && second[0].0 != first[0].0 && second[0].1, "{second:?} after {first:?}");
}

#[test]
fn turns_are_stated_from_where_each_entity_stood_when_the_hold_began() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    for _ in 0..3 {
        wheel(&mut session, -1.0);
    }
    // Nothing answers here, so the chair still stands at 0 for every notch. Each edit states
    // the whole turn from there all the same.
    let asked: Vec<_> = asked(&engine).iter().map(|ops| turns(ops)).collect();
    assert_eq!(asked, [[(CHAIR, 355.0)], [(CHAIR, 350.0)], [(CHAIR, 345.0)]]);
}

#[test]
fn another_selection_in_the_same_hold_is_turned_from_where_it_stands_as_another_step() {
    let (mut session, engine) = selecting();
    session.selection = vec![CHAIR];
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    wheel(&mut session, -1.0);
    let first = edits(&engine);
    assert_eq!(turns(&first[1].2), [(CHAIR, 350.0)]);

    // The wall is selected in the chair's place with R still down. The next notch turns the
    // wall by five degrees, and asks nothing more of the chair.
    session.selection = vec![WALL];
    wheel(&mut session, -1.0);
    wheel(&mut session, -1.0);
    let second = edits(&engine);
    assert_eq!([turns(&second[0].2), turns(&second[1].2)], [[(WALL, 355.0)], [(WALL, 350.0)]]);
    assert!(one_stroke(&first) && one_stroke(&second) && first[0].1 != second[0].1);
}

#[test]
fn r_let_go_after_the_wheel_turns_no_quarter() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    assert_eq!(asked(&engine).len(), 1);
    session.key_up('R');
    assert!(asked(&engine).is_empty());

    // Nor after a part of a notch, which turned nothing.
    session.key_down('R', false);
    wheel(&mut session, -0.25);
    session.key_up('R');
    assert!(asked(&engine).is_empty());
}

#[test]
fn r_alone_turns_a_quarter_when_it_comes_up() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    assert!(asked(&engine).is_empty());
    session.key_up('R');

    // One edit of its own. A quarter with the clock from 0 is minus a quarter, as a map file
    // writes that angle, and not three quarters.
    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Rotate", None));
    assert_eq!(edits[0].2, [json!({ "kind": "rotate", "uid": CHAIR, "rotation": -std::f64::consts::FRAC_PI_2 })]);
    // R that comes up again without having gone down turns nothing.
    session.key_up('R');
    assert!(asked(&engine).is_empty());
}

#[test]
fn holding_r_does_not_repeat() {
    // The interface hands R on once as it goes down. Were the keyboard's repeats to reach the
    // session all the same, they would turn nothing and begin no hold of their own.
    let (mut session, engine) = holding(Tool::Select);
    for _ in 0..3 {
        assert!(session.key_down('R', false));
    }
    assert!(asked(&engine).is_empty());
    session.key_up('R');
    assert_eq!(asked(&engine).len(), 1);

    // A repeat after the wheel leaves the hold as it was: R let go turns no quarter, and the
    // repeat's Shift is not the hold's.
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    session.key_down('R', true);
    wheel(&mut session, -1.0);
    session.key_up('R');
    let edits = edits(&engine);
    assert_eq!([turns(&edits[0].2), turns(&edits[1].2)], [[(CHAIR, 355.0)], [(CHAIR, 350.0)]]);
    assert!(edits.len() == 2 && one_stroke(&edits));
}

#[test]
fn shift_r_turns_the_other_way() {
    let (mut session, engine) = holding(Tool::Select);
    // Shift is read as R goes down. It is let go before R is, and the turn is still Shift's.
    session.key_down('R', true);
    session.held.shift = false;
    session.key_up('R');
    // And R that went down without it is plain, whatever is held as it comes up.
    session.key_down('R', false);
    session.held.shift = true;
    session.key_up('R');
    let quarters: Vec<_> = asked(&engine).iter().map(|ops| turns(ops)).collect();
    assert_eq!(quarters, [[(CHAIR, 90.0)], [(CHAIR, -90.0)]]);

    // The wheel has its own two ways, and Shift changes neither.
    session.key_down('R', true);
    wheel(&mut session, -1.0);
    session.key_up('R');
    assert_eq!(asked(&engine).iter().map(|ops| turns(ops)).collect::<Vec<_>>(), [[(CHAIR, 355.0)]]);
}

#[test]
fn the_wheel_does_not_zoom_while_r_is_held_with_an_entity_tool() {
    for tool in [Tool::Select, Tool::Place] {
        // With the chair selected, and with nothing selected.
        for selected in [true, false] {
            let (mut session, _engine) = holding(tool);
            if !selected {
                session.selection.clear();
            }
            let view = (session.camera.x, session.camera.y, session.camera.zoom);
            assert!(!session.wheel_turns() &&session.key_down('R', false) && session.wheel_turns(), "{tool:?} {selected}");
            session.wheel(1.0, (480.0, 200.0), VIEW);
            session.wheel(-0.25, (480.0, 200.0), VIEW);
            assert_eq!(glide_out(&mut session), 0, "{tool:?} {selected}");
            assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), view, "{tool:?} {selected}");

            // With R let go it zooms again.
            session.key_up('R');
            assert!(!session.wheel_turns(), "{tool:?} {selected}");
            session.wheel(1.0, (480.0, 200.0), VIEW);
            glide_out(&mut session);
            assert!(session.camera.zoom > view.2, "{tool:?} {selected}");
        }
    }
}

#[test]
fn r_held_with_a_tile_tool_leaves_the_wheel_to_zoom() {
    // The area tool keeps R for itself, and is tried with its own tests.
    for tool in Tool::EVERY.into_iter().filter(|&tool| !tool.is_entity_tool() && tool != Tool::Area) {
        let (mut session, engine) = holding(tool);
        // R is the rectangle tool there, as it goes down.
        assert!(session.key_down('R', false), "{tool:?}");
        assert_eq!((session.tool(), session.wheel_turns()), (Tool::Rectangle, false), "{tool:?}");
        let zoom = session.camera.zoom;
        wheel(&mut session, 1.0);
        assert!(session.camera.zoom > zoom, "{tool:?}");
        session.key_up('R');
        assert!(asked(&engine).is_empty(), "{tool:?}");
        assert_eq!((session.tool(), &session.selection[..]), (Tool::Rectangle, &[CHAIR][..]), "{tool:?}");
    }
}

#[test]
fn changing_the_tool_ends_a_hold_of_r() {
    // To a tile tool by its key, to the other entity tool, and to the tool that is in hand.
    for next in [Tool::Paint, Tool::Place, Tool::Select] {
        let (mut session, engine) = holding(Tool::Select);
        session.key_down('R', false);
        if next == Tool::Paint {
            session.key_down('B', false);
        } else {
            session.set_tool(next);
        }
        assert_eq!((session.tool(), session.wheel_turns()), (next, false), "{next:?}");

        // R is still down, and the wheel zooms. R let go turns nothing, neither the chair
        // nor what the place tool places next.
        let zoom = session.camera.zoom;
        wheel(&mut session, 1.0);
        assert!(session.camera.zoom > zoom, "{next:?}");
        session.key_up('R');
        assert!(asked(&engine).is_empty(), "{next:?}");
        session.set_tool(Tool::Place);
        choose(&mut session, "SnapgridCenter");
        click(&mut session, (2.5, 3.5));
        assert_eq!(asked(&engine)[0][0]["rotation"], json!(0.0), "{next:?}");
    }
}

#[test]
fn a_key_that_is_lost_ends_the_hold_with_nothing_asked() {
    let (mut session, engine) = holding(Tool::Select);
    // Another key that is not down says nothing about R.
    session.key_down('R', false);
    session.key_lost('B');
    assert!(session.wheel_turns());
    // R is lost, as when a dialog opens over the map. No quarter turn comes of it, then or
    // when R is seen to come up after all.
    session.key_lost('R');
    assert!(!session.wheel_turns());
    session.key_up('R');
    assert!(asked(&engine).is_empty());

    // R that is not held is not down in any frame, and is said to be lost in each.
    session.key_lost('R');
    tap_r(&mut session, false);
    assert_eq!(asked(&engine).len(), 1);
}

#[test]
fn the_pointer_leaving_the_map_does_not_end_a_hold_of_r() {
    let (mut session, engine) = holding(Tool::Select);
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    session.pointer_left();
    session.pointer_move(at(&session, (1.5, 1.5)), VIEW);
    wheel(&mut session, -1.0);
    let edits = edits(&engine);
    assert_eq!(turns(&edits[1].2), [(CHAIR, 350.0)]);
    assert!(one_stroke(&edits));

    // R is a key, and turns a quarter wherever the pointer is when it comes up.
    session.key_up('R');
    session.key_down('R', false);
    session.pointer_left();
    session.key_up('R');
    assert_eq!(asked(&engine).iter().map(|ops| turns(ops)).collect::<Vec<_>>(), [[(CHAIR, -90.0)]]);
}

#[test]
fn delete_asks_for_each_selected_entity() {
    let (mut session, engine) = Session::offline(&scene());
    session.selection = vec![CHAIR, WALL];
    session.delete_selection();

    let deleted = [json!({ "kind": "delete", "uid": CHAIR }), json!({ "kind": "delete", "uid": WALL })];
    assert_eq!(asked(&engine), [deleted]);
}

/// Where `stacked` puts its three things: the middle of cell (1, 1).
const HEAP: (f32, f32) = (1.5, 1.5);

/// A session as `selecting` gives it, with the chair, the wall and the crowbar all drawn on
/// one place, in that order, so that the crowbar is on top and the chair at the bottom.
fn stacked() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = selecting();
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 0), (CROWBAR, HEAP, 0)]);
    (session, engine)
}

/// Turns the wheel over a map position, and lets any zoom it owes run out. A notch towards the
/// user is -1.
fn wheel_over(session: &mut Session, world: (f32, f32), notches: f32) {
    session.wheel(notches, at(session, world), VIEW);
    glide_out(session);
}

/// The list the interface would draw: its prototypes, topmost first, and which one it is on.
fn listed(session: &Session) -> Option<(Vec<String>, usize)> {
    session.stack().map(|stack| (stack.entries, stack.current))
}

/// Where the view is and how far it is zoomed.
fn view_of(session: &Session) -> (f32, f32, f32) {
    (session.camera.x, session.camera.y, session.camera.zoom)
}

#[test]
fn the_wheel_over_a_selected_stack_steps_through_it() {
    let (mut session, engine) = stacked();
    click(&mut session, HEAP);
    assert_eq!(session.selection, [CROWBAR]);
    assert_eq!(session.stack(), None);
    let view = view_of(&session);

    // A notch towards the user goes one down the list, and one away from the user one up.
    let names = ["Crowbar", "WallSolid", "Chair"].map(String::from).to_vec();
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[WALL][..], Some((names.clone(), 1))));
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[CHAIR][..], Some((names.clone(), 2))));
    wheel_over(&mut session, HEAP, 1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[WALL][..], Some((names.clone(), 1))));
    // The list is drawn beside the cell the cursor is over.
    assert_eq!(session.stack().unwrap().cell, [(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]);

    // A notch that comes in parts is one step, taken when more than half of it is here.
    wheel_over(&mut session, HEAP, -0.3);
    assert_eq!(session.selection, [WALL]);
    wheel_over(&mut session, HEAP, -0.3);
    assert_eq!(session.selection, [CHAIR]);
    wheel_over(&mut session, HEAP, -0.4);
    assert_eq!(session.selection, [CHAIR]);

    // None of it zoomed, and none of it asked anything of the engine.
    assert_eq!(view_of(&session), view);
    assert!(asked(&engine).is_empty());

    // The wheel steps from the selected one, wherever in the list it is.
    let (mut session, _engine) = stacked();
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 0), (CROWBAR, (4.5, 4.5), 0)]);
    session.selection = vec![CHAIR];
    wheel_over(&mut session, HEAP, 1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[WALL][..], Some((vec!["WallSolid".to_owned(), "Chair".to_owned()], 0))));
}

#[test]
fn the_wheel_zooms_when_several_are_selected() {
    // Two of the stack selected, or all three, as a box over a room selects them.
    for selection in [vec![CROWBAR, CHAIR], vec![CHAIR, WALL, CROWBAR]] {
        let (mut session, engine) = stacked();
        session.selection = selection.clone();
        let zoom = session.camera.zoom;
        wheel_over(&mut session, HEAP, -1.0);
        assert!(session.camera.zoom < zoom, "{selection:?}");
        assert_eq!((session.selection.clone(), session.stack()), (selection.clone(), None), "{selection:?}");
        assert!(asked(&engine).is_empty(), "{selection:?}");
    }
}

#[test]
fn the_wheel_zooms_where_nothing_selected_stands() {
    // Nothing is selected. The crowbar is, and stands elsewhere. The wall is, and the wheel
    // is over empty floor. The place tool is in hand over a selected stack.
    for case in 0..4 {
        let (mut session, engine) = stacked();
        let mut over = HEAP;
        match case {
            0 => {}
            1 => {
                draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 0), (CROWBAR, (4.5, 4.5), 0)]);
                session.selection = vec![CROWBAR];
            }
            2 => {
                session.selection = vec![WALL];
                over = (0.5, 4.5);
            }
            _ => {
                session.selection = vec![WALL];
                session.set_tool(Tool::Place);
            }
        }
        let (selection, zoom) = (session.selection.clone(), session.camera.zoom);
        wheel_over(&mut session, over, 1.0);
        assert!(session.camera.zoom > zoom, "case {case}");
        assert_eq!((session.selection.clone(), session.stack()), (selection, None), "case {case}");
        assert!(asked(&engine).is_empty(), "case {case}");
    }

    // One selected thing with nothing else under the cursor is no list either.
    let (mut session, _engine) = selecting();
    session.selection = vec![CHAIR];
    let zoom = session.camera.zoom;
    wheel_over(&mut session, (1.5, 1.5), -1.0);
    assert!(session.camera.zoom < zoom);
    assert_eq!((&session.selection[..], session.stack()), (&[CHAIR][..], None));
}

#[test]
fn the_stack_wraps_at_both_ends() {
    let (mut session, _engine) = stacked();
    session.selection = vec![CROWBAR];
    // Up from the top is the bottom, and down from the bottom is the top.
    wheel_over(&mut session, HEAP, 1.0);
    assert_eq!((&session.selection[..], listed(&session).map(|list| list.1)), (&[CHAIR][..], Some(2)));
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!((&session.selection[..], listed(&session).map(|list| list.1)), (&[CROWBAR][..], Some(0)));

    // Several notches at once are as many steps, round the ends as well.
    wheel_over(&mut session, HEAP, -4.0);
    assert_eq!(session.selection, [WALL]);
    wheel_over(&mut session, HEAP, 5.0);
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn leaving_the_cell_closes_the_stack() {
    let (mut session, _engine) = stacked();
    session.selection = vec![CROWBAR];
    wheel_over(&mut session, HEAP, -1.0);
    // The cursor moved within the cell leaves the list open.
    session.pointer_move(at(&session, (1.9, 1.1)), VIEW);
    assert_eq!(listed(&session).map(|list| list.1), Some(1));

    // In the next cell it is closed, and the selection stays.
    session.pointer_move(at(&session, (2.1, 1.1)), VIEW);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));

    // A part of a notch is forgotten with the list: two parts with the cursor away between
    // them are no step, where together they would have been one.
    session.pointer_move(at(&session, HEAP), VIEW);
    wheel_over(&mut session, HEAP, -0.4);
    assert_eq!(listed(&session).map(|list| list.1), Some(1));
    session.pointer_move(at(&session, (2.5, 1.5)), VIEW);
    session.pointer_move(at(&session, HEAP), VIEW);
    wheel_over(&mut session, HEAP, -0.4);
    assert_eq!((&session.selection[..], listed(&session).map(|list| list.1)), (&[WALL][..], Some(1)));

    // The cursor leaving the map closes it as well.
    session.pointer_left();
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));
}

#[test]
fn a_press_closes_the_stack_and_drags_the_entity_stepped_to() {
    let (mut session, engine) = stacked();
    click(&mut session, HEAP);
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(session.selection, [WALL]);

    // Pressed where the crowbar is on top, and dragged a cell across. The wall goes, from
    // where the document has it.
    session.pointer_down(at(&session, HEAP), VIEW, Button::Primary);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));
    session.pointer_move(at(&session, (2.5, 1.5)), VIEW);
    // The wheel steps through nothing while a button is down, over the three as they are
    // still drawn.
    let zoom = session.camera.zoom;
    wheel_over(&mut session, HEAP, 1.0);
    assert!(session.camera.zoom > zoom);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));
    session.pointer_up(at(&session, (2.5, 1.5)), VIEW);

    assert_eq!(asked(&engine), [[json!({ "kind": "move", "uid": WALL, "x": 4.5, "y": 1.5 })]]);
    assert_eq!(session.selection, [WALL]);
}

#[test]
fn a_hidden_layer_is_left_out_of_the_stack() {
    let (mut session, _engine) = selecting();
    // The wall is drawn at a depth the game gives doors, over the crowbar and the chair.
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 9), (CROWBAR, HEAP, 0)]);
    session.layers.doors = false;
    session.selection = vec![CROWBAR];
    let names = vec!["Crowbar".to_owned(), "Chair".to_owned()];
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[CHAIR][..], Some((names.clone(), 1))));
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[CROWBAR][..], Some((names, 0))));

    // A selected thing that is hidden does not make a list of the two that show.
    session.pointer_left();
    session.selection = vec![WALL];
    let zoom = session.camera.zoom;
    wheel_over(&mut session, HEAP, 1.0);
    assert!(session.camera.zoom > zoom);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));

    // With only the wall's layer shown there is one thing there, and no list.
    session.layers.doors = true;
    session.layers.objects = false;
    let zoom = session.camera.zoom;
    wheel_over(&mut session, HEAP, 1.0);
    assert!(session.camera.zoom > zoom);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));
}

#[test]
fn a_press_with_no_list_open_takes_the_topmost_entity() {
    // The chair is selected, at the bottom. A click there selects the crowbar on top.
    let (mut session, engine) = stacked();
    session.selection = vec![CHAIR];
    click(&mut session, HEAP);
    assert_eq!(session.selection, [CROWBAR]);
    // A drag from there with the chair selected moves the crowbar, and not the chair.
    session.selection = vec![CHAIR];
    drag(&mut session, (1, 1), (2, 1));
    let moved: Vec<Vec<Value>> = asked(&engine).iter().map(|ops| ops.iter().map(|op| op["uid"].clone()).collect()).collect();
    assert_eq!(moved, [[json!(CROWBAR)]]);
    assert_eq!(session.selection, [CROWBAR]);

    // Ctrl takes out only the crowbar, which is not selected, so nothing goes.
    session.selection = vec![CHAIR, WALL];
    session.held.ctrl = true;
    click(&mut session, HEAP);
    assert_eq!(session.selection, [CHAIR, WALL]);

    // Shift adds the crowbar, and does not take the selected chair out.
    session.selection = vec![CHAIR];
    session.held = Held { shift: true, ..Held::default() };
    click(&mut session, HEAP);
    assert_eq!(session.selection, [CHAIR, CROWBAR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_press_that_closes_the_list_prefers_the_entity_stepped_to() {
    // Ctrl takes out the chair the wheel stepped to, and not the crowbar on top.
    let (mut session, engine) = stacked();
    session.selection = vec![CROWBAR];
    wheel_over(&mut session, HEAP, 1.0);
    assert_eq!(session.selection, [CHAIR]);
    session.held.ctrl = true;
    click(&mut session, HEAP);
    assert!(session.selection.is_empty());

    // Shift takes it out as well, where on the crowbar it would add that.
    session.held = Held::default();
    session.selection = vec![CROWBAR];
    wheel_over(&mut session, HEAP, 1.0);
    session.held.shift = true;
    click(&mut session, HEAP);
    assert!(session.selection.is_empty());

    // A list closed some other way first leaves the press to the topmost.
    session.held = Held::default();
    session.selection = vec![CROWBAR];
    wheel_over(&mut session, HEAP, 1.0);
    session.pointer_left();
    click(&mut session, HEAP);
    assert_eq!(session.selection, [CROWBAR]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_right_click_closes_the_stack() {
    let (mut session, engine) = stacked();
    click(&mut session, HEAP);
    wheel_over(&mut session, HEAP, -1.0);
    assert!(session.stack().is_some());

    // The first closes the list and keeps what the wheel stepped to.
    session.pointer_down(at(&session, HEAP), VIEW, Button::Secondary);
    session.pointer_up(at(&session, HEAP), VIEW);
    assert_eq!((&session.selection[..], session.stack()), (&[WALL][..], None));
    // The next finds no list, and clears the selection as a right-click does.
    session.pointer_down(at(&session, HEAP), VIEW, Button::Secondary);
    session.pointer_up(at(&session, HEAP), VIEW);
    assert!(session.selection.is_empty());
    assert!(asked(&engine).is_empty());
}

#[test]
fn changing_the_tool_closes_the_stack() {
    // To another tool, and to the tool that is in hand.
    for next in [Tool::Place, Tool::Select] {
        let (mut session, _engine) = stacked();
        session.selection = vec![CROWBAR];
        wheel_over(&mut session, HEAP, -0.4);
        assert!(session.stack().is_some(), "{next:?}");
        session.set_tool(next);
        assert_eq!((&session.selection[..], session.stack()), (&[CROWBAR][..], None), "{next:?}");

        // The part of a notch went with the list.
        session.set_tool(Tool::Select);
        wheel_over(&mut session, HEAP, -0.4);
        assert_eq!(session.selection, [CROWBAR], "{next:?}");
    }
}

/// A session as `selecting` gives it, with the chair covering cell (1, 1) and the crowbar
/// drawn over it towards the cell's lower left, so that near the upper right corner only the
/// chair is under the cursor. Both are on the cell.
fn offset_stack() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = selecting();
    draw(&mut session, &[(CHAIR, HEAP, 0), (CROWBAR, (1.3, 1.3), 0)]);
    (session, engine)
}

#[test]
fn the_wheel_steps_from_an_entity_lower_in_the_stack() {
    let (mut session, engine) = offset_stack();
    // A click where only the chair is drawn selects it, though the crowbar is above it.
    click(&mut session, (1.9, 1.9));
    assert_eq!(session.selection, [CHAIR]);
    let view = view_of(&session);

    let names = ["Crowbar", "Chair"].map(String::from).to_vec();
    wheel_over(&mut session, (1.9, 1.9), -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[CROWBAR][..], Some((names.clone(), 0))));
    wheel_over(&mut session, (1.9, 1.9), -1.0);
    assert_eq!((&session.selection[..], listed(&session)), (&[CHAIR][..], Some((names, 1))));
    assert_eq!(view_of(&session), view);
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_wheel_steps_with_the_cursor_anywhere_in_the_cell() {
    // Each corner of the cell, and its middle.
    for over in [(1.05, 1.05), (1.95, 1.05), (1.95, 1.95), (1.05, 1.95), HEAP] {
        for selected in [CHAIR, CROWBAR] {
            let (mut session, _engine) = offset_stack();
            session.selection = vec![selected];
            let zoom = session.camera.zoom;
            wheel_over(&mut session, over, -1.0);
            let other = if selected == CHAIR { CROWBAR } else { CHAIR };
            assert_eq!(session.selection, [other], "{over:?} {selected}");
            assert_eq!(session.camera.zoom, zoom, "{over:?} {selected}");
        }
    }
}

#[test]
fn one_entity_alone_on_its_cell_leaves_the_wheel_to_zoom() {
    // The crowbar's picture reaches into the chair's cell, and its middle is in the next one.
    let (mut session, _engine) = selecting();
    draw(&mut session, &[(CHAIR, HEAP, 0), (CROWBAR, (2.4, 1.5), 0)]);
    session.selection = vec![CHAIR];
    let zoom = session.camera.zoom;
    wheel_over(&mut session, (1.5, 1.5), -1.0);
    assert!(session.camera.zoom < zoom);
    assert_eq!((&session.selection[..], session.stack()), (&[CHAIR][..], None));

    // Where its picture is under the cursor the crowbar is in the stack as well.
    let zoom = session.camera.zoom;
    wheel_over(&mut session, (1.95, 1.5), -1.0);
    assert_eq!((session.camera.zoom, &session.selection[..]), (zoom, &[CROWBAR][..]));
}

#[test]
fn a_press_that_closes_the_list_takes_the_entity_stepped_to_anywhere_in_the_cell() {
    let (mut session, engine) = offset_stack();
    session.selection = vec![CHAIR];
    wheel_over(&mut session, (1.9, 1.9), -1.0);
    assert_eq!(session.selection, [CROWBAR]);
    // The crowbar is not drawn where it is pressed, and is dragged all the same.
    session.pointer_down(at(&session, (1.9, 1.9)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.9, 1.9)), VIEW);
    session.pointer_up(at(&session, (2.9, 1.9)), VIEW);
    let moved: Vec<Vec<Value>> = asked(&engine).iter().map(|ops| ops.iter().map(|op| op["uid"].clone()).collect()).collect();
    assert_eq!(moved, [[json!(CROWBAR)]]);
}

#[test]
fn the_hover_label_counts_the_whole_cell() {
    let (mut session, _engine) = offset_stack();
    session.set_prototypes(vec![named("Crowbar", "crowbar"), named("Chair", "chair")]);
    // Where only the chair is drawn, it is named and the crowbar counted.
    assert_eq!(label_at(&mut session, (1.9, 1.9)).as_deref(), Some("chair (+1 more)"));
    assert_eq!(label_at(&mut session, (1.2, 1.2)).as_deref(), Some("crowbar (+1 more)"));
}

/// A prototype with a name of its own.
fn named(id: &str, name: &str) -> Prototype {
    Prototype { id: id.into(), name: name.into(), ..Default::default() }
}

/// The text of the hover label for the cursor at a map position.
fn label_at(session: &mut Session, world: (f32, f32)) -> Option<String> {
    session.pointer_move(at(session, world), VIEW);
    session.hover_label().map(|(_, text)| text)
}

#[test]
fn the_hover_label_names_the_top_entity_and_counts_the_rest() {
    let (mut session, _engine) = stacked();
    session.set_prototypes(vec![named("Crowbar", "crowbar"), named("Chair", "chair")]);
    // With any tool in hand.
    session.set_tool(Tool::Paint);

    // The crowbar is on top, and is known by its prototype's name.
    session.pointer_move(at(&session, HEAP), VIEW);
    let (cell, text) = session.hover_label().expect("a label");
    assert_eq!((cell, text.as_str()), ([(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)], "crowbar (+2 more)"));

    // An entity's own name comes first.
    session.doc.as_mut().unwrap().entities.get_mut(&CROWBAR).unwrap().name = Some("old crowbar".into());
    assert_eq!(label_at(&mut session, HEAP).as_deref(), Some("old crowbar (+2 more)"));

    // What is on a hidden layer is not counted. Here the wall is drawn as a structure.
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, -2), (CROWBAR, HEAP, 0)]);
    session.layers.structures = false;
    assert_eq!(label_at(&mut session, HEAP).as_deref(), Some("old crowbar (+1 more)"));

    let (mut alone, _engine) = selecting();
    alone.set_prototypes(vec![named("Chair", "chair")]);
    // A prototype the engine does not know is named by its id, and one alone has no count.
    assert_eq!(label_at(&mut alone, (3.5, 1.5)).as_deref(), Some("WallSolid"));
    assert_eq!(label_at(&mut alone, (1.5, 1.5)).as_deref(), Some("chair"));
    alone.layers.objects = false;
    alone.layers.structures = false;
    assert_eq!(label_at(&mut alone, (1.5, 1.5)), None);
    // Nothing is labelled where nothing is.
    alone.layers = Layers::default();
    assert_eq!(label_at(&mut alone, (5.5, 5.5)), None);
}

#[test]
fn no_hover_label_while_a_drag_is_live() {
    let (mut session, _engine) = selecting();
    assert!(label_at(&mut session, (1.5, 1.5)).is_some());
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    assert_eq!(label_at(&mut session, (1.5, 1.5)), None);
    session.pointer_up(at(&session, (1.5, 1.5)), VIEW);
    assert!(label_at(&mut session, (1.5, 1.5)).is_some());

    // A pan is a drag as well.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Middle);
    assert_eq!(session.hover_label(), None);
}

#[test]
fn a_selection_of_several_is_summed_up_by_prototype() {
    let (mut session, _engine) = Session::offline(&scene());
    // Four chairs, three walls, two crowbars, a table and a lamp.
    let doc = session.doc.as_mut().unwrap();
    let chair = doc.entities[&CHAIR].clone();
    for (uid, prototype) in [(20, "Chair"), (21, "Chair"), (22, "Chair"), (23, "WallSolid"), (24, "WallSolid"), (25, "Crowbar"), (26, "Table"), (27, "Lamp")] {
        doc.entities.insert(uid, Entity { uid, prototype: prototype.into(), ..chair.clone() });
    }
    session.selection = vec![CHAIR, WALL, CROWBAR, 20, 21, 22, 23, 24, 25, 26, 27];
    assert_eq!(session.selection_summary(), Some(("11 entities selected".into(), "Chair x4, WallSolid x3, Crowbar x2, +2 more types".into())));

    // As many of each, they go by their ids. Three types or fewer have no more to count.
    session.selection = vec![WALL, CHAIR];
    assert_eq!(session.selection_summary(), Some(("2 entities selected".into(), "Chair x1, WallSolid x1".into())));

    // One entity is not summed up: the panel shows it whole.
    session.selection = vec![CHAIR];
    assert_eq!(session.selection_summary(), None);
}

#[test]
fn shift_with_something_anchored_selected_says_why_nothing_moves_freely() {
    let (mut session, _engine) = selecting();
    session.selection = vec![CHAIR, WALL];
    session.held.shift = true;
    session.set_status("Opened");
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    // Not past four pixels yet.
    session.pointer_move(at(&session, (1.55, 1.5)), VIEW);
    assert_eq!(session.status_line(), "Opened");
    session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
    assert_eq!(session.status_line(), "Shift only moves freely when nothing selected is anchored");
    // Said once, as the drag passes the four pixels.
    session.set_status("Moved");
    session.pointer_move(at(&session, (2.5, 1.75)), VIEW);
    assert_eq!(session.status_line(), "Moved");
    session.pointer_up(at(&session, (2.5, 1.75)), VIEW);

    // Nothing is said with nothing anchored, or without Shift.
    for (selection, shift) in [(vec![CHAIR], true), (vec![CHAIR, WALL], false)] {
        session.selection = selection;
        session.held.shift = shift;
        session.set_status("Opened");
        session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
        session.pointer_move(at(&session, (2.25, 1.75)), VIEW);
        session.pointer_up(at(&session, (2.25, 1.75)), VIEW);
        assert_eq!(session.status_line(), "Opened", "shift {shift}");
    }
}

#[test]
fn a_crowbar_placed_off_its_cell_is_selected_and_moved_by_the_cursor_with_shift() {
    // A crowbar lying loose off the middle of cell (2, 3), on a grid off the map's whole units.
    const LOOSE: i32 = 13;
    let loose = format!(r#"{{"uid":{LOOSE},"yamlUid":0,"prototype":"Crowbar","parent":{GRID},"x":2.3,"y":3.7,"rotation":0,"anchored":false,"contained":false,"name":null}}],"#);
    let scene = super::place::scene_off_whole_units().replacen("}],\n            \"history\"", &format!("}},{loose}\n            \"history\""), 1);
    let (mut session, engine) = Session::offline(&scene);
    session.set_prototypes(vec![super::place::real_crowbar()]);
    let origin = (-1.8541667_f32, -0.40625_f32);
    let on_map = |local: (f32, f32)| (origin.0 + local.0, origin.1 + local.1);
    draw(&mut session, &[(LOOSE, on_map((2.3, 3.7)), 0)]);
    session.set_tool(Tool::Select);

    // A click selects it, and a drag with Shift takes it by the cursor exactly.
    click(&mut session, on_map((2.3, 3.7)));
    assert_eq!(session.selection, [LOOSE]);
    session.held.shift = true;
    session.pointer_down(at(&session, on_map((2.3, 3.7))), VIEW, Button::Primary);
    session.pointer_move(at(&session, on_map((3.45, 3.95))), VIEW);
    session.pointer_up(at(&session, on_map((3.45, 3.95))), VIEW);
    let asked = asked(&engine);
    assert_eq!(asked.len(), 1, "{asked:?}");
    let (x, y) = (asked[0][0]["x"].as_f64().unwrap(), asked[0][0]["y"].as_f64().unwrap());
    assert!((x - 3.45).abs() < 1e-4 && (y - 3.95).abs() < 1e-4, "moved to ({x}, {y})");
}
