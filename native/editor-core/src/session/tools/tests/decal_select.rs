//! Decals in Entity Select and Pick: selecting, moving, turning, deleting and picking them.

use super::decals::decals_on_map;
use super::*;

/// The ops of every edit, each with the decal it changes and its corner and angle in degrees.
fn updates(ops: &[Value]) -> Vec<(i64, f64, f64, f64)> {
    assert!(ops.iter().all(|op| op["kind"] == "decal.update" && op["grid"] == GRID), "{ops:?}");
    ops.iter().map(|op| (op["uid"].as_i64().unwrap(), op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap(), angle(op))).collect()
}

/// A session with the select tool and three decals on the floor: two arrows stacked on cell
/// (1, 4), the later one on top, and grass placed freely across cells (3, 4) and (4, 4).
fn decal_selecting() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Arrows", (1.0, 4.0)), (3, "Grass", (3.5, 4.0))]);
    session.set_tool(Tool::Select);
    (session, engine)
}

#[test]
fn a_click_where_no_entity_is_selects_the_topmost_decal_alone() {
    let (mut session, engine) = decal_selecting();
    session.selection = vec![CHAIR];
    click(&mut session, (1.5, 4.5));

    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[][..], &[(GRID, 2)][..]));
    assert!(asked(&engine).is_empty());
    // It is outlined where it lies.
    assert_eq!(outline(&session), (OutlineStyle::Decal, [(1.0, 4.0), (2.0, 4.0), (2.0, 5.0), (1.0, 5.0)]));

    // A decal placed freely is found by its own square, across two cells.
    click(&mut session, (4.25, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 3)]);
    // The higher z-index is on top, whatever the ids.
    let (mut session, _engine) = decals_on_map(&[(5, "Arrows", (1.0, 4.0)), (4, "Grass", (1.0, 4.0))]);
    session.set_tool(Tool::Select);
    session.doc.as_mut().unwrap().grids[0].decals.get_mut(&4).unwrap().z_index = 1;
    click(&mut session, (1.5, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 4)]);
}

#[test]
fn an_entity_under_the_cursor_wins_over_a_decal_and_selecting_it_drops_the_decals() {
    let (mut session, _engine) = decals_on_map(&[(1, "Arrows", (1.0, 1.0))]);
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0)]);
    session.set_tool(Tool::Select);
    session.selected_decals = vec![(GRID, 1)];
    click(&mut session, (1.5, 1.5));

    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR][..], &[][..]));
}

#[test]
fn shift_adds_a_decal_or_takes_a_selected_one_out_and_ctrl_takes_it_out() {
    let (mut session, engine) = decal_selecting();
    session.selection = vec![CHAIR];
    session.held.shift = true;
    click(&mut session, (4.25, 4.5));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR][..], &[(GRID, 3)][..]));
    click(&mut session, (1.5, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 3), (GRID, 2)]);
    // A click with Shift on one that is selected takes it out, and moves nothing.
    click(&mut session, (4.25, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 2)]);

    session.held = Held { ctrl: true, ..Held::default() };
    click(&mut session, (1.5, 4.5));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR][..], &[][..]));
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_drag_moves_the_selected_decals_by_whole_cells_with_the_entities_as_one_step() {
    let (mut session, engine) = decal_selecting();
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0)]);
    session.selection = vec![CHAIR];
    session.selected_decals = vec![(GRID, 3)];
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.2, 1.6)), VIEW);
    session.pointer_move(at(&session, (3.6, 2.4)), VIEW);
    session.pointer_up(at(&session, (3.6, 2.4)), VIEW);

    let edits = edits(&engine);
    assert!(one_stroke(&edits) && labels(&edits).iter().all(|label| *label == "Move"), "{edits:?}");
    let last = &edits.last().unwrap().2;
    assert_eq!((last[0]["kind"].clone(), last[0]["x"].clone(), last[0]["y"].clone()), (json!("move"), json!(3.5), json!(2.5)));
    assert_eq!(updates(&last[1..]), [(3, 5.5, 5.0, 0.0)]);
    // Every field of the decal is stated.
    assert_eq!((last[1]["prototype"].clone(), last[1]["color"].clone(), last[1]["zIndex"].clone(), last[1]["cleanable"].clone()), (json!("Grass"), Value::Null, json!(0), json!(false)));

    // Pressed on a decal alone, it moves alone. With Shift it goes by the cursor exactly.
    let (mut session, engine) = decal_selecting();
    session.held.shift = true;
    session.selected_decals = vec![(GRID, 2)];
    session.pointer_down(at(&session, (1.5, 4.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.75, 4.0)), VIEW);
    session.pointer_up(at(&session, (2.75, 4.0)), VIEW);
    assert_eq!(updates(&asked(&engine)[0]), [(2, 2.25, 3.5, 0.0)]);
    assert_eq!(session.selected_decals, [(GRID, 2)]);
}

#[test]
fn a_box_selects_the_decals_whose_middle_is_in_it() {
    let (mut session, _engine) = decal_selecting();
    // Over the middles of the arrows, (1.5, 4.5), and short of the grass's, (4, 4.5).
    drag(&mut session, (0, 3), (3, 4));
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 2)]);

    // With Shift a box adds, and with Ctrl it takes away.
    session.held.shift = true;
    drag(&mut session, (3, 3), (5, 5));
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 2), (GRID, 3)]);
    session.held = Held { ctrl: true, ..Held::default() };
    drag(&mut session, (0, 3), (2, 5));
    assert_eq!(session.selected_decals, [(GRID, 3)]);
}

#[test]
fn decals_on_a_hidden_layer_are_not_selected() {
    let (mut session, _engine) = decal_selecting();
    session.layers.decals = false;
    click(&mut session, (1.5, 4.5));
    drag(&mut session, (0, 3), (5, 5));

    assert!(session.selected_decals.is_empty());
    assert!(session.hover_label().is_none());
}

#[test]
fn delete_removes_the_selected_decals_with_the_entities_as_one_edit() {
    let (mut session, engine) = decal_selecting();
    session.selection = vec![CHAIR];
    session.selected_decals = vec![(GRID, 3), (GRID, 1)];
    assert!(session.can_delete());
    session.delete_key();

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Delete"]);
    let ops: Vec<(String, i64)> = edits[0].2.iter().map(|op| (op["kind"].as_str().unwrap().to_owned(), op["uid"].as_i64().unwrap())).collect();
    assert_eq!(ops, [("delete".to_owned(), CHAIR as i64), ("decal.remove".to_owned(), 3), ("decal.remove".to_owned(), 1)]);
}

#[test]
fn r_turns_the_selected_decals_a_quarter_about_their_middles() {
    let (mut session, engine) = decal_selecting();
    session.selected_decals = vec![(GRID, 1)];
    tap_r(&mut session, false);
    tap_r(&mut session, true);

    let turns = asked(&engine);
    assert_eq!(updates(&turns[0]), [(1, 1.0, 4.0, -90.0)]);
    assert_eq!(updates(&turns[1]), [(1, 1.0, 4.0, 90.0)]);
}

#[test]
fn r_held_with_the_wheel_turns_the_selected_decals_five_degrees_a_notch_as_one_step() {
    let (mut session, engine) = decal_selecting();
    session.selected_decals = vec![(GRID, 3)];
    session.key_down('R', false);
    wheel(&mut session, 1.0);
    wheel(&mut session, 1.0);
    session.key_up('R');

    let edits = edits(&engine);
    assert!(one_stroke(&edits), "{edits:?}");
    let last: Vec<(i64, f64, f64, f64)> = updates(&edits.last().unwrap().2);
    assert_eq!(last, [(3, 3.5, 4.0, 10.0)]);
}

#[test]
fn a_secondary_press_and_escape_clear_the_selected_decals() {
    let (mut session, _engine) = decal_selecting();
    session.selected_decals = vec![(GRID, 1)];
    session.pointer_down(at(&session, (0.5, 0.5)), VIEW, Button::Secondary);
    assert!(session.selected_decals.is_empty());

    session.selected_decals = vec![(GRID, 1)];
    session.escape();
    assert!(session.selected_decals.is_empty());
}

#[test]
fn the_hover_label_names_the_topmost_decal_where_no_entity_is() {
    let (mut session, _engine) = decal_selecting();
    session.pointer_move(at(&session, (1.5, 4.5)), VIEW);
    let (corners, text) = session.hover_label().unwrap();
    assert_eq!((corners, text.as_str()), ([(1.0, 4.0), (2.0, 4.0), (2.0, 5.0), (1.0, 5.0)], "Arrows (+1 more)"));
}

#[test]
fn pick_takes_a_decal_with_its_settings_before_the_tile() {
    let (mut session, _engine) = decal_selecting();
    {
        let decal = session.doc.as_mut().unwrap().grids[0].decals.get_mut(&2).unwrap();
        (decal.color, decal.angle, decal.z_index, decal.cleanable) = (Some("#11223344".into()), 1.0, 4, true);
    }
    session.set_place_free(true);
    session.set_tool(Tool::Pick);
    click(&mut session, (1.5, 4.5));

    assert_eq!((session.chosen_decal(), session.tool(), session.status_line()), (Some("Arrows"), Tool::Paint, "Picked Arrows"));
    // The decal's colour and the rest, and free placing as it was.
    assert_eq!(session.decal_settings(), &DecalSettings { color: Some("#11223344".into()), angle: 1.0, z_index: 4, cleanable: true });
    assert!(session.place_free());

    // The wheel's list has both arrows and then the tile.
    session.set_tool(Tool::Pick);
    wheel_at(&mut session, (1.5, 4.5));
    let ids: Vec<String> = session.pick_list().unwrap().entries.iter().map(|entry| entry.id().to_owned()).collect();
    assert_eq!(ids, ["Arrows", "Arrows", "FloorSteel"]);
}

// ---- the decal panel ----

/// Gives a decal of the grid a colour of its own.
fn colour(session: &mut Session, id: i32, color: &str) {
    session.doc.as_mut().unwrap().grids[0].decals.get_mut(&id).unwrap().color = Some(color.into());
}

#[test]
fn the_summary_says_what_the_selected_decals_share() {
    let (mut session, _engine) = decal_selecting();
    assert_eq!(session.decal_summary(), None);

    session.selected_decals = vec![(GRID, 1), (GRID, 2), (GRID, 3)];
    let summary = session.decal_summary().unwrap();
    assert_eq!((summary.count, summary.breakdown.as_str(), summary.prototype), (3, "Arrows x2, Grass x1", None));
    // None has a colour of its own, so they share having none, as do the three on the grid.
    assert_eq!((summary.color, summary.angle, summary.z_index, summary.cleanable, summary.same_colour), (Some(None), Some(0.0), Some(0), Some(false), 3));

    colour(&mut session, 1, "#FF0000FF");
    session.selected_decals = vec![(GRID, 1), (GRID, 2)];
    let summary = session.decal_summary().unwrap();
    assert_eq!((summary.prototype.as_deref(), summary.color, summary.same_colour), (Some("Arrows"), None, 0));
}

#[test]
fn a_field_changed_in_one_gesture_is_one_step_stated_from_where_it_began() {
    let (mut session, engine) = decal_selecting();
    session.selected_decals = vec![(GRID, 1), (GRID, 3)];
    session.edit_selected_decals(DecalField::Angle(0.5));
    // The mirror moves on in between, and the next change still states the rest as it was.
    session.doc.as_mut().unwrap().grids[0].decals.get_mut(&1).unwrap().z_index = 9;
    session.edit_selected_decals(DecalField::Angle(1.0));

    let edits = edits(&engine);
    assert!(one_stroke(&edits) && labels(&edits) == ["Edit decals", "Edit decals"], "{edits:?}");
    let last = &edits[1].2;
    assert_eq!(updates(last).iter().map(|update| update.0).collect::<Vec<_>>(), [1, 3]);
    assert!(last.iter().all(|op| op["rotation"] == json!(1.0) && op["zIndex"] == json!(0)), "{last:?}");

    // Another field, or the gesture's end, begins a step of its own.
    session.edit_selected_decals(DecalField::Cleanable(true));
    session.end_decal_edit();
    session.edit_selected_decals(DecalField::Cleanable(false));
    let strokes = strokes(&engine);
    assert_eq!(strokes.len(), 2);
    assert!(strokes[0].0 != edits[0].1 && strokes[1].0 != strokes[0].0 && strokes.iter().all(|(_, coalesce)| *coalesce), "{strokes:?}");
}

#[test]
fn a_colour_and_no_colour_are_set_as_the_engine_writes_them() {
    let (mut session, engine) = decal_selecting();
    session.selected_decals = vec![(GRID, 2)];
    session.edit_selected_decals(DecalField::Color(Some("#00FF0080".into())));
    session.end_decal_edit();
    session.edit_selected_decals(DecalField::Color(None));

    let asked = asked(&engine);
    assert_eq!(labels_of(&asked), [json!("#00FF0080"), Value::Null]);
    assert!(asked.iter().all(|ops| ops.len() == 1 && ops[0]["kind"] == "decal.update" && ops[0]["uid"] == 2));
}

/// The colour each edit's first op asks for.
fn labels_of(asked: &[Vec<Value>]) -> Vec<Value> {
    asked.iter().map(|ops| ops[0]["color"].clone()).collect()
}

#[test]
fn select_all_takes_the_decals_of_the_same_prototype_or_the_same_colour() {
    let (mut session, _engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Arrows", (2.0, 2.0)), (3, "Grass", (3.0, 4.0)), (4, "Grass", (0.0, 0.0))]);
    session.selection = vec![CHAIR];
    session.selected_decals = vec![(GRID, 3)];
    session.select_decals_like();
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[][..], &[(GRID, 3), (GRID, 4)][..]));

    colour(&mut session, 1, "#FF0000FF");
    colour(&mut session, 4, "#FF0000FF");
    session.selected_decals = vec![(GRID, 1)];
    session.select_decals_coloured();
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 4)]);

    // Of two prototypes nothing changes.
    session.selected_decals = vec![(GRID, 1), (GRID, 4)];
    session.select_decals_like();
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 4)]);
}

#[test]
fn recolor_all_gives_every_decal_of_the_shared_colour_another_as_one_edit() {
    let (mut session, engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Arrows", (2.0, 2.0)), (3, "Grass", (3.0, 4.0))]);
    colour(&mut session, 1, "#FF0000FF");
    colour(&mut session, 3, "#FF0000FF");
    session.selected_decals = vec![(GRID, 3)];
    session.recolor_decals(Some("#0000FFFF".into()));

    let edits = edits(&engine);
    assert_eq!((labels(&edits), edits[0].1), (vec!["Recolour decals"], None));
    assert_eq!(updates(&edits[0].2).iter().map(|update| update.0).collect::<Vec<_>>(), [1, 3]);
    assert!(edits[0].2.iter().all(|op| op["color"] == json!("#0000FFFF")));

    // Where the selected decals' colours differ nothing is asked.
    session.selected_decals = vec![(GRID, 1), (GRID, 2)];
    session.recolor_decals(None);
    assert!(asked(&engine).is_empty());
}

/// Turns the wheel a notch towards the user over a map position.
fn wheel_at(session: &mut Session, world: (f32, f32)) {
    let point = at(session, world);
    session.wheel(-1.0, point, VIEW);
}
