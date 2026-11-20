//! The decal palette and the placement settings.

use super::*;

/// A decal prototype with these tags.
fn decal(id: &str, tags: &[&str]) -> DecalPrototype {
    DecalPrototype { id: id.into(), tags: tags.iter().map(|tag| (*tag).to_owned()).collect(), ..Default::default() }
}

/// A few decal prototypes, in the order of their ids as the engine lists them. Arrows has a
/// picture.
fn decal_prototypes() -> Vec<DecalPrototype> {
    let arrows = DecalPrototype { sprite_path: Some("/Textures/Decals/markings.rsi".into()), sprite_state: Some("arrows".into()), ..decal("Arrows", &["station", "markings"]) };
    vec![arrows, decal("Bushes", &["flora"]), decal("Grass", &["flora"]), decal("Plain", &[]), decal("Rune", &["crayon"]), decal("WarnLineN", &["markings"])]
}

/// A session with the decal prototypes of `decal_prototypes`.
pub(super) fn with_decals() -> (Session, Receiver<ToEngine>) {
    decals_on_map(&[])
}

/// The scene of `scene`, with decals on its grid: an id, a prototype and a lower-left corner each.
pub(super) fn scene_with_decals(decals: &[(i32, &str, (f32, f32))]) -> String {
    let decals: Vec<String> = decals
        .iter()
        .map(|(id, prototype, (x, y))| format!(r#"{{"id":{id},"prototype":"{prototype}","x":{x},"y":{y},"color":null,"angle":0,"zIndex":0,"cleanable":false}}"#))
        .collect();
    scene().replacen("]}],", &format!(r#"],"decals":[{}]}}],"#, decals.join(",")), 1)
}

/// A session over `scene_with_decals`, with the decal prototypes of `decal_prototypes`.
pub(super) fn decals_on_map(decals: &[(i32, &str, (f32, f32))]) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene_with_decals(decals));
    session.decal_prototypes = decal_prototypes();
    (session, engine)
}

/// The kind of each op and the decal's lower-left corner it asks for.
fn corners(ops: &[Value]) -> Vec<(&str, f64, f64)> {
    ops.iter().map(|op| (op["kind"].as_str().unwrap(), op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect()
}

#[test]
fn r_turns_the_decal_placement_angle_while_a_brush_places_a_decal() {
    let (mut session, engine) = with_decals();
    session.choose_decal("Arrows");
    let degrees = |session: &Session| (session.decal_settings().angle.to_degrees() * 1e6).round() / 1e6;
    // Clockwise is a quarter less, within a whole turn, and Shift turns it back.
    tap_r(&mut session, false);
    assert_eq!((degrees(&session), session.tool()), (270.0, Tool::Paint));
    tap_r(&mut session, true);
    tap_r(&mut session, true);
    assert_eq!(degrees(&session), 90.0);
    session.set_tool(Tool::Line);
    tap_r(&mut session, false);
    assert_eq!((degrees(&session), session.tool()), (0.0, Tool::Line));
    assert!(asked(&engine).is_empty());

    // With a tile in hand R is the rectangle tool, and the angle stays.
    session.choose_tile("FloorSteel");
    session.set_tool(Tool::Paint);
    tap_r(&mut session, false);
    assert_eq!((degrees(&session), session.tool()), (0.0, Tool::Rectangle));
}

#[test]
fn the_wheel_with_r_held_turns_the_decal_placement_angle_five_degrees_a_notch_and_does_not_zoom() {
    let (mut session, engine) = with_decals();
    session.choose_decal("Arrows");
    let degrees = |session: &Session| (session.decal_settings().angle.to_degrees() * 1e6).round() / 1e6;
    let point = over(&session, (1, 1));
    // R alone: the wheel zooms.
    session.wheel(1.0, point, VIEW);
    assert!(session.zooming() && !session.wheel_turns());
    session.stop_zoom();

    // R goes down and turns a quarter. Held, a notch away adds five degrees, as the web editor
    // has it, and one towards the user takes them off. A part of a notch turns its part.
    assert!(session.key_down('R', false));
    assert!(session.wheel_turns());
    session.wheel(1.0, point, VIEW);
    assert_eq!(degrees(&session), 275.0);
    session.wheel(-2.0, point, VIEW);
    session.wheel(0.5, point, VIEW);
    assert_eq!(degrees(&session), 267.5);
    assert!(!session.zooming());
    assert_eq!(session.tool(), Tool::Paint);

    // R up: the wheel zooms again and the angle stays.
    session.key_up('R');
    session.wheel(1.0, point, VIEW);
    assert!(session.zooming());
    assert_eq!(degrees(&session), 267.5);
    session.stop_zoom();

    // R lost to a field ends the hold too, and so does a change of tool, to the eraser too,
    // which keeps the decal chosen and places none.
    session.key_down('R', false);
    session.key_lost('R');
    assert!(!session.wheel_turns());
    for tool in [Tool::Circle, Tool::Erase] {
        session.set_tool(Tool::Paint);
        session.key_down('R', false);
        session.set_tool(tool);
        assert!(!session.wheel_turns(), "{tool:?}");
    }
    assert_eq!(session.chosen_decal(), Some("Arrows"));

    // With a tile chosen R is the rectangle tool, and the wheel zooms with it held.
    session.choose_tile("FloorSteel");
    session.set_tool(Tool::Paint);
    session.key_down('R', false);
    session.wheel(1.0, point, VIEW);
    assert!(session.zooming() && session.tool() == Tool::Rectangle);
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_scene_with_decals_puts_them_in_the_mirror() {
    let (session, _engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Grass", (2.5, 4.25))]);
    let grid = session.doc.as_ref().unwrap().grid(GRID).unwrap();
    assert_eq!((grid.decals.len(), grid.decals[&2].x, grid.tiles.len()), (2, 2.5, 36));
}

#[test]
fn choosing_a_decal_keeps_a_brush_and_takes_paint_from_any_other_tool() {
    for tool in Tool::EVERY {
        let (mut session, _engine) = with_decals();
        session.set_tool(tool);
        session.choose_decal("Arrows");

        assert_eq!((session.palette(), session.chosen_decal()), (&Palette::Decal("Arrows".into()), Some("Arrows")), "{tool:?}");
        let kept = matches!(tool, Tool::Paint | Tool::Erase | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Lasso | Tool::Polygon);
        assert_eq!(session.tool(), if kept { tool } else { Tool::Paint }, "{tool:?}");
        // Nothing else is chosen any more.
        assert_eq!((session.chosen_tile(), session.chosen_entity()), (None, None), "{tool:?}");
    }
}

#[test]
fn a_decal_chosen_gives_way_to_a_tile_or_an_entity_chosen_after_it() {
    let (mut session, _engine) = with_decals();
    session.choose_decal("Grass");
    session.choose_tile("FloorWood");
    assert_eq!(session.chosen_decal(), None);

    session.choose_decal("Grass");
    choose(&mut session, "SnapgridCenter");
    assert_eq!((session.chosen_decal(), session.chosen_entity()), (None, Some(0)));
}

#[test]
fn a_choice_of_a_decal_answers_a_hint() {
    let (mut session, _engine) = with_decals();
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Room);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    assert_eq!(session.status_line(), "Choose a tile to paint");

    session.choose_decal("Arrows");
    assert_ne!(session.status_line(), "Choose a tile to paint");
}

#[test]
fn the_decal_palette_groups_by_the_first_tag_with_the_web_editor_s_first() {
    let (session, _engine) = with_decals();
    let groups: Vec<(String, Vec<&str>)> =
        session.decal_groups().into_iter().map(|group| (group.tag, group.decals.iter().map(|&index| session.decal_prototypes[index].id.as_str()).collect())).collect();

    assert_eq!(
        groups,
        [
            ("station".to_owned(), vec!["Arrows"]),
            ("markings".to_owned(), vec!["WarnLineN"]),
            ("flora".to_owned(), vec!["Bushes", "Grass"]),
            // The rest by name, and the untagged among them as "other".
            ("crayon".to_owned(), vec!["Rune"]),
            ("other".to_owned(), vec!["Plain"]),
        ]
    );
}

#[test]
fn the_decal_search_matches_every_word_in_the_id_in_either_case() {
    let (session, _engine) = with_decals();

    assert_eq!(session.decals_matching(&[]), [0, 1, 2, 3, 4, 5]);
    assert_eq!(session.decals_matching(&["r"]), [0, 2, 4, 5]);
    assert_eq!(session.decals_matching(&["WARN", "line"]), [5]);
    assert_eq!(session.decals_matching(&["warn", "grass"]), Vec::<usize>::new());
    // The tags are not searched.
    assert_eq!(session.decals_matching(&["flora"]), Vec::<usize>::new());
}

#[test]
fn placement_settings_start_as_the_web_editor_s_and_keep_the_angle_within_a_turn() {
    let (mut session, _engine) = with_decals();
    assert_eq!(session.decal_settings(), &DecalSettings { color: None, angle: 0.0, z_index: 0, cleanable: false });
    // Decals go on cells until Free is chosen, as entities do.
    assert!(!session.place_free());

    session.set_decal_settings(DecalSettings { angle: -std::f64::consts::FRAC_PI_2, z_index: 3, ..Default::default() });
    let settings = session.decal_settings();
    assert!((settings.angle - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{}", settings.angle);
    assert_eq!(settings.z_index, 3);
}

#[test]
fn a_decal_colour_reads_and_writes_as_the_engine_has_it() {
    assert_eq!(decal_colour_bytes("#FF8000CC"), Some([255, 128, 0, 204]));
    assert_eq!(decal_colour_bytes("#ff8000"), Some([255, 128, 0, 255]));
    assert_eq!(decal_colour_bytes("FF8000CC"), None);
    assert_eq!(decal_colour_bytes("#FF80"), None);
    assert_eq!(decal_colour_bytes("#GG8000CC"), None);
    assert_eq!(decal_colour_text([255, 128, 0, 204]), "#FF8000CC");
    assert_eq!(decal_colour_text([1, 2, 3, 4]), "#01020304");
}

// ---- placing ----

#[test]
fn a_stroke_of_paint_puts_a_decal_on_the_corner_of_each_cell_as_one_step() {
    let (mut session, engine) = with_decals();
    let settings = DecalSettings { color: Some("#FF000080".into()), angle: 0.5, z_index: 2, cleanable: true };
    session.set_decal_settings(settings);
    session.choose_decal("Arrows");
    drag(&mut session, (0, 4), (2, 4));

    let edits = edits(&engine);
    assert!(one_stroke(&edits), "{edits:?}");
    assert!(labels(&edits).iter().all(|label| *label == "Paint decals"));
    let ops: Vec<Value> = edits.into_iter().flat_map(|edit| edit.2).collect();
    assert_eq!(corners(&ops), [("decal", 0.0, 4.0), ("decal", 1.0, 4.0), ("decal", 2.0, 4.0)]);
    // Each with the settings, on the grid.
    let op = &ops[0];
    assert_eq!((op["grid"].clone(), op["prototype"].clone(), op["color"].clone()), (json!(GRID), json!("Arrows"), json!("#FF000080")));
    assert_eq!((op["rotation"].clone(), op["zIndex"].clone(), op["cleanable"].clone()), (json!(0.5), json!(2), json!(true)));
}

#[test]
fn a_cell_crossed_twice_in_one_stroke_gets_one_decal() {
    let (mut session, engine) = with_decals();
    session.choose_decal("Arrows");
    session.pointer_down(over(&session, (0, 4)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (1, 4)), VIEW);
    session.pointer_move(over(&session, (0, 4)), VIEW);
    session.pointer_up(over(&session, (0, 4)), VIEW);

    let ops: Vec<Value> = asked(&engine).into_iter().flatten().collect();
    assert_eq!(corners(&ops), [("decal", 0.0, 4.0), ("decal", 1.0, 4.0)]);
}

#[test]
fn paint_with_free_or_shift_puts_one_decal_with_its_middle_on_the_cursor() {
    for (snap, shift) in [(false, false), (true, true)] {
        let (mut session, engine) = with_decals();
        session.set_place_free(!snap);
        session.choose_decal("Grass");
        session.held.shift = shift;
        session.pointer_down(at(&session, (2.25, 3.75)), VIEW, Button::Primary);
        // No stroke is begun, so the drag adds nothing.
        session.pointer_move(at(&session, (4.5, 3.5)), VIEW);
        session.pointer_up(at(&session, (4.5, 3.5)), VIEW);

        let edits = edits(&engine);
        assert_eq!(labels(&edits), ["Place decal"], "snap {snap} shift {shift}");
        assert_eq!(edits[0].1, None);
        assert_eq!(corners(&edits[0].2), [("decal", 1.75, 3.25)], "snap {snap} shift {shift}");
    }
}

#[test]
fn a_free_decal_whose_corner_has_no_tile_is_not_asked_for() {
    let (mut session, engine) = with_decals();
    session.set_place_free(true);
    session.choose_decal("Grass");
    // The corner is half a cell below and left of the cursor, off the floor.
    click(&mut session, (0.25, 2.5));

    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not placed: a decal needs a tile under its lower-left corner");
}

#[test]
fn the_shapes_put_decals_one_to_a_cell_with_a_tile_as_one_edit() {
    for (tool, label, to, count) in [(Tool::Rectangle, "Rectangle of decals", (7, 5), 4), (Tool::Line, "Line of decals", (7, 4), 2), (Tool::Circle, "Circle of decals", (5, 4), 5)] {
        let (mut session, engine) = with_decals();
        session.set_tool(tool);
        session.choose_decal("Grass");
        assert_eq!(session.tool(), tool);
        // From cell (4, 4) towards space beyond the floor's edge at 5, and the shapes keep to
        // cells whatever the settings say.
        session.set_place_free(true);
        drag(&mut session, (4, 4), to);

        let edits = edits(&engine);
        assert_eq!(labels(&edits), [label], "{tool:?}");
        assert_eq!(edits[0].1, None, "{tool:?}");
        let ops = &edits[0].2;
        assert_eq!(ops.len(), count, "{tool:?}: {ops:?}");
        assert!(ops.iter().all(|op| op["kind"] == "decal" && op["x"].as_f64().unwrap() <= 5.0 && op["y"].as_f64().unwrap() <= 5.0), "{tool:?}: {ops:?}");
    }
}

#[test]
fn the_eraser_with_only_on_removes_only_that_prototype_s_decals_on_the_cells_it_crosses() {
    let (mut session, engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Grass", (1.0, 4.0)), (3, "Arrows", (2.5, 4.25)), (4, "Arrows", (5.0, 5.0))]);
    session.set_erase_only(true);
    session.set_tool(Tool::Erase);
    session.choose_decal("Arrows");
    assert_eq!(session.tool(), Tool::Erase);
    drag(&mut session, (0, 4), (3, 4));

    let edits = edits(&engine);
    assert!(one_stroke(&edits) && labels(&edits).iter().all(|label| *label == "Erase decals"), "{edits:?}");
    let removed: Vec<(i64, i64)> = edits.iter().flat_map(|edit| &edit.2).map(|op| (op["grid"].as_i64().unwrap(), op["uid"].as_i64().unwrap())).collect();
    assert!(edits.iter().flat_map(|edit| &edit.2).all(|op| op["kind"] == "decal.remove"));
    assert_eq!(removed, [(GRID as i64, 1), (GRID as i64, 3)]);
}

#[test]
fn the_ghost_of_the_chosen_decal_is_where_a_press_would_put_it() {
    let (mut session, _engine) = with_decals();
    session.choose_decal("Arrows");
    session.set_decal_settings(DecalSettings { color: Some("#00FF0080".into()), ..Default::default() });
    session.pointer_move(at(&session, (2.6, 3.3)), VIEW);

    let (_, over) = session.overlay_quads();
    assert_eq!(over.len(), FLOATS_PER_QUAD);
    let corners: Vec<(f32, f32)> = over[..8].chunks_exact(2).map(|corner| (corner[0], corner[1])).collect();
    assert_eq!(corners, [(2.0, 3.0), (3.0, 3.0), (3.0, 4.0), (2.0, 4.0)]);
    // Tinted with the colour, and half seen through.
    assert_eq!(&over[8..12], &[0.0, 1.0, 0.0, 128.0 / 255.0 * 0.5]);

    // Turned a quarter, about its middle.
    session.set_decal_settings(DecalSettings { angle: std::f64::consts::FRAC_PI_2, ..Default::default() });
    let (_, over) = session.overlay_quads();
    let first = (over[0], over[1]);
    assert!((first.0 - 3.0).abs() < 1e-5 && (first.1 - 3.0).abs() < 1e-5, "{first:?}");

    // Placed freely, its middle is on the cursor.
    session.set_decal_settings(DecalSettings::default());
    session.set_place_free(true);
    let (_, over) = session.overlay_quads();
    assert!((over[0] - 2.1).abs() < 1e-5 && (over[1] - 2.8).abs() < 1e-5, "{:?}", &over[..2]);
}

#[test]
fn only_the_tools_that_put_a_decal_down_show_its_ghost() {
    for tool in Tool::EVERY {
        let (mut session, _engine) = with_decals();
        session.choose_decal("Arrows");
        session.set_tool(tool);
        session.pointer_move(at(&session, (2.5, 3.5)), VIEW);
        let shows = matches!(tool, Tool::Paint | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Lasso | Tool::Polygon);
        assert_eq!(session.overlay_quads().1.len(), if shows { FLOATS_PER_QUAD } else { 0 }, "{tool:?}");
    }

    // A decal with no picture has no ghost.
    let (mut session, _engine) = with_decals();
    session.choose_decal("Grass");
    session.pointer_move(at(&session, (2.5, 3.5)), VIEW);
    assert!(session.overlay_quads().1.is_empty());
}

#[test]
fn a_stroke_or_a_shape_of_decals_skips_cells_that_hold_one_already_and_a_click_stacks() {
    let (mut session, engine) = decals_on_map(&[(1, "Grass", (2.0, 4.0))]);
    session.choose_decal("Grass");
    assert_eq!(session.tool(), Tool::Paint);
    // From (1, 4) over (2, 4), which holds grass, to (3, 4).
    drag(&mut session, (1, 4), (3, 4));
    assert_eq!(corners(&edits(&engine).iter().flat_map(|edit| edit.2.clone()).collect::<Vec<_>>()), [("decal", 1.0, 4.0), ("decal", 3.0, 4.0)]);

    // A click on it puts another on top.
    click(&mut session, (2.5, 4.5));
    assert_eq!(corners(&asked(&engine).concat()), [("decal", 2.0, 4.0)]);

    // So does a press that begins a stroke, but a rectangle skips it.
    session.set_tool(Tool::Rectangle);
    drag(&mut session, (1, 4), (2, 4));
    assert_eq!(corners(&asked(&engine).concat()), [("decal", 1.0, 4.0)]);
}

#[test]
fn fill_with_a_decal_puts_one_on_each_cell_of_the_floor() {
    let (mut session, engine) = decals_on_map(&[(1, "Grass", (2.0, 4.0))]);
    session.set_tool(Tool::Fill);
    session.choose_decal("Grass");
    assert_eq!(session.tool(), Tool::Fill);
    click(&mut session, (0.5, 0.5));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Fill decals"]);
    assert_eq!(edits[0].2.len(), 35);
}
