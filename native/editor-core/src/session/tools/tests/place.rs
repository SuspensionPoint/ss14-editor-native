//! The place tool.

use super::*;

/// A session with the place tool in hand. Choosing an entity keeps it there, where the paint
/// tool a session begins with would stay.
fn placing() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Place);
    (session, engine)
}

#[test]
fn a_click_places_the_chosen_prototype_on_the_middle_of_the_cell() {
    let (mut session, engine) = placing();
    choose(&mut session, "SnapgridCenter");
    click(&mut session, (2.25, 3.75));

    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Table", "parent": GRID, "x": 2.5, "y": 3.5, "rotation": 0.0 })]]);
}

#[test]
fn a_drag_places_one_in_each_cell_it_stops_in() {
    let (mut session, engine) = placing();
    choose(&mut session, "SnapgridCenter");
    session.pointer_down(over(&session, (1, 4)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (1.75, 4.25)), VIEW);
    session.pointer_move(over(&session, (2, 4)), VIEW);
    session.pointer_move(over(&session, (3, 4)), VIEW);
    session.pointer_up(over(&session, (3, 4)), VIEW);

    let edits = edits(&engine);
    let places: Vec<(f64, f64)> = edits.iter().flat_map(|edit| &edit.2).map(|op| (op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect();
    assert_eq!(places, [(1.5, 4.5), (2.5, 4.5), (3.5, 4.5)]);
    assert_eq!(labels(&edits), ["Place entity"; 3]);
    assert!(one_stroke(&edits));
}

#[test]
fn a_place_drag_remembers_every_cell_it_has_placed_in() {
    let (mut session, engine) = placing();
    choose(&mut session, "SnapgridCenter");
    session.pointer_down(over(&session, (1, 4)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 4)), VIEW);
    session.pointer_move(at(&session, (2.75, 4.25)), VIEW);
    session.pointer_move(over(&session, (1, 4)), VIEW);
    session.pointer_up(over(&session, (1, 4)), VIEW);

    assert_eq!(asked(&engine).len(), 2);
}

#[test]
fn a_drag_places_what_does_not_sit_on_cells_only_where_it_was_pressed() {
    let (mut session, engine) = placing();
    choose(&mut session, "PlaceFree");
    session.pointer_down(at(&session, (1.25, 4.25)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 4)), VIEW);
    session.pointer_move(over(&session, (3, 4)), VIEW);
    session.pointer_up(over(&session, (3, 4)), VIEW);

    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Table", "parent": GRID, "x": 1.25, "y": 4.25, "rotation": 0.0 })]]);
}

#[test]
fn the_place_tool_outlines_where_it_would_place() {
    let (mut session, _engine) = placing();
    // On a grid it is the cell, for what sits on cells and for what does not.
    for placement_mode in ["SnapgridCenter", "PlaceFree"] {
        choose(&mut session, placement_mode);
        session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
        assert_eq!(outline(&session), (OutlineStyle::Hover, [(2.0, 3.0), (3.0, 3.0), (3.0, 4.0), (2.0, 4.0)]));
    }
    // Off the grids it is a square around the place itself, which here is the cursor.
    session.pointer_move(at(&session, (8.25, 8.25)), VIEW);
    assert_eq!(outline(&session), (OutlineStyle::Hover, [(7.75, 7.75), (8.75, 7.75), (8.75, 8.75), (7.75, 8.75)]));
}

#[test]
fn shift_places_at_the_cursor_exactly() {
    let (mut session, engine) = placing();
    // A prototype that sits on the middle of a cell, put down off it.
    choose(&mut session, "SnapgridCenter");
    session.held.shift = true;
    click(&mut session, (2.25, 3.75));

    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Table", "parent": GRID, "x": 2.25, "y": 3.75, "rotation": 0.0 })]]);
}

/// Chooses a prototype that sits on cells, anchored by default or not.
fn choose_anchored(session: &mut Session, anchored: bool) {
    session.set_prototypes(vec![Prototype { id: "Wall".into(), placement_mode: "SnapgridCenter".into(), anchored, ..Default::default() }]);
    session.choose_entity(0);
}

#[test]
fn shift_puts_what_is_anchored_by_default_on_the_middle_of_its_cell() {
    let (mut session, engine) = placing();
    choose_anchored(&mut session, true);
    session.held.shift = true;
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);

    // The ghost, its label and the outline tell where it will land: on the cell, not free.
    let preview = session.place_preview(1.0).expect("a preview");
    assert_eq!((preview.at, preview.label.as_deref(), preview.square, preview.colour), ((2.5, 3.5), Some("Wall"), Some(CELL_2_3), [0, 255, 100, 204]));
    assert_eq!(outline(&session), (OutlineStyle::Hover, CELL_2_3));

    // A press sends the cell's middle and says why.
    session.set_status("Opened");
    click(&mut session, (2.25, 3.75));
    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Wall", "parent": GRID, "x": 2.5, "y": 3.5, "rotation": 0.0 })]]);
    assert_eq!(session.status_line(), "Wall is anchored, so it goes in the middle of its cell");
    // Said once a press, and not again as the cursor moves.
    session.set_status("Moved");
    session.pointer_move(at(&session, (4.25, 3.75)), VIEW);
    assert_eq!(session.status_line(), "Moved");
}

#[test]
fn shift_puts_what_is_loose_at_the_cursor() {
    let (mut session, engine) = placing();
    choose_anchored(&mut session, false);
    session.held.shift = true;
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);

    let preview = session.place_preview(1.0).expect("a preview");
    assert_eq!((preview.at, preview.label.as_deref(), preview.colour), ((2.25, 3.75), Some("Wall (free)"), [100, 200, 255, 204]));
    session.set_status("Opened");
    click(&mut session, (2.25, 3.75));
    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Wall", "parent": GRID, "x": 2.25, "y": 3.75, "rotation": 0.0 })]]);
    assert_eq!(session.status_line(), "Opened");
}

/// The crowbar as the engine tells it at start: the game's default placement, and not anchored.
pub(super) fn real_crowbar() -> Prototype {
    Prototype { id: "Crowbar".into(), name: "crowbar".into(), placement_mode: "PlaceFree".into(), anchored: false, components: vec!["Item".into(), "Sprite".into()], ..Default::default() }
}

/// The test scene with its grid where the pursuit shuttle's is, off the whole units of the map.
pub(super) fn scene_off_whole_units() -> String {
    scene().replace(r#""x":0,"y":0,"rotation":0,"tileIds""#, r#""x":-1.8541667,"y":-0.40625,"rotation":0,"tileIds""#)
}

/// The x and y of the one op asked for.
#[track_caller]
fn only_place(engine: &Receiver<ToEngine>) -> (String, f64, f64) {
    let asked = asked(engine);
    assert_eq!((asked.len(), asked[0].len()), (1, 1), "{asked:?}");
    let op = &asked[0][0];
    (op["kind"].as_str().unwrap().to_owned(), op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())
}

#[test]
fn a_crowbar_goes_where_the_cursor_is_with_shift_and_without() {
    let (mut session, engine) = Session::offline(&scene_off_whole_units());
    session.set_tool(Tool::Place);
    session.set_prototypes(vec![real_crowbar()]);
    session.choose_entity(0);
    let origin = (-1.8541667_f32, -0.40625_f32);
    for shift in [true, false] {
        session.held.shift = shift;
        // The cell (2, 3) of the grid, three tenths across and seven up.
        let world = (origin.0 + 2.3, origin.1 + 3.7);
        session.pointer_move(at(&session, world), VIEW);
        let ghost = session.place_preview(1.0).expect("a preview").at;
        assert!((ghost.0 - world.0).abs() < 1e-4 && (ghost.1 - world.1).abs() < 1e-4, "shift {shift}: the ghost is at {ghost:?}");
        click(&mut session, world);
        let (kind, x, y) = only_place(&engine);
        assert_eq!(kind, "spawn");
        assert!((x - 2.3).abs() < 1e-4 && (y - 3.7).abs() < 1e-4, "shift {shift}: placed at ({x}, {y})");
        // The performance display says what was sent and what was held at the press.
        let held = if shift { "Shift" } else { "nothing" };
        assert_eq!(session.last_put.as_deref(), Some(format!("placed Crowbar at (2.300, 3.700) of {GRID}, {held} held").as_str()));
    }
}

#[test]
fn a_shift_drag_places_only_where_it_was_pressed() {
    let (mut session, engine) = placing();
    choose(&mut session, "SnapgridCenter");
    session.held.shift = true;
    session.pointer_down(at(&session, (1.25, 4.25)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 4)), VIEW);
    // Shift let go during the drag lays no row either.
    session.held.shift = false;
    session.pointer_move(over(&session, (3, 4)), VIEW);
    session.pointer_up(over(&session, (3, 4)), VIEW);

    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Table", "parent": GRID, "x": 1.25, "y": 4.25, "rotation": 0.0 })]]);
}

/// Gives the session the engine's previews of the chosen prototype (index 0) from all four
/// sides: one rectangle, its corners `base` for the side it faces unturned and `base` turned
/// by a quarter for each side after.
fn give_previews(session: &mut Session, base: [(f32, f32); 4]) {
    let items: Vec<Value> = (0..4).map(|turns| json!([turns, 0, 0, 0.0, 1, KIND_ENTITY])).collect();
    let header = json!({ "doc": PREVIEWS, "full": true, "items": items }).to_string();
    let mut message = b"DRW1".to_vec();
    message.extend((header.len() as i32).to_le_bytes());
    message.extend(header.as_bytes());
    for turns in 0..4 {
        let mut quad = [0.0f32; FLOATS_PER_QUAD];
        for (index, corner) in base.iter().enumerate() {
            let turned = turn(*corner, turns as f32 * std::f32::consts::FRAC_PI_2);
            quad[index * 2] = turned.0;
            quad[index * 2 + 1] = turned.1;
        }
        quad[8..12].copy_from_slice(&[1.0; 4]);
        message.extend(quad.iter().flat_map(|float| float.to_le_bytes()));
    }
    session.lists.apply(&message).expect("draw list");
    session.placing.arrived(&[0], true);
}

/// A point turned about the origin, anticlockwise by `angle` radians.
fn turn(point: (f32, f32), angle: f32) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    (point.0 * cos - point.1 * sin, point.0 * sin + point.1 * cos)
}

/// The corners of the ghost's rectangles, to the thousandth.
fn ghost(session: &mut Session) -> Vec<(f32, f32)> {
    let (_, over) = session.overlay_quads();
    let round = |value: f32| (value * 1000.0).round() / 1000.0;
    over.chunks_exact(FLOATS_PER_QUAD).flat_map(|quad| quad[..8].chunks_exact(2).map(|corner| (round(corner[0]), round(corner[1]))).collect::<Vec<_>>()).collect()
}

#[test]
fn the_place_outline_follows_the_cursor_with_shift_held() {
    let (mut session, _engine) = placing();
    choose(&mut session, "SnapgridCenter");
    give_previews(&mut session, [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]);
    session.held.shift = true;
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);

    // The outline and the ghost are round the cursor, and not the cell.
    assert_eq!(outline(&session), (OutlineStyle::Hover, [(1.75, 3.25), (2.75, 3.25), (2.75, 4.25), (1.75, 4.25)]));
    assert_eq!(ghost(&mut session), [(1.75, 3.25), (2.75, 3.25), (2.75, 4.25), (1.75, 4.25)]);
}

#[test]
fn the_preview_names_the_prototype_and_the_way_it_faces() {
    let (mut session, _engine) = placing();
    choose(&mut session, "SnapgridCenter");
    // Nothing is shown with the cursor off the map.
    assert!(session.place_preview(1.0).is_none());

    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    let preview = session.place_preview(1.0).expect("a preview");
    // Not turned from the way it faces by default, so with no arrow.
    assert_eq!((preview.id.as_str(), preview.at, preview.facing), ("Table", (2.5, 3.5), None));

    // A quarter turn clockwise from facing down is facing left.
    tap_r(&mut session, false);
    let facing = session.place_preview(1.0).and_then(|preview| preview.facing).expect("an arrow");
    assert_eq!((facing.0.round(), facing.1.round()), (-1.0, 0.0));
    assert!((facing.0.hypot(facing.1) - 1.0).abs() < 1e-5);

    // Nothing with another tool in hand.
    session.set_tool(Tool::Select);
    assert!(session.place_preview(1.0).is_none());
}

/// The cell (2, 3) as a square on the map, and as the ghost of a picture one cell wide.
const CELL_2_3: [(f32, f32); 4] = [(2.0, 3.0), (3.0, 3.0), (3.0, 4.0), (2.0, 4.0)];
const SQUARE: [(f32, f32); 4] = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];

#[test]
fn the_brushes_show_the_chosen_entity_where_a_press_would_put_it() {
    let (mut session, _engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    give_previews(&mut session, SQUARE);
    // Turned with the place tool. The brushes put it down unturned all the same.
    session.set_tool(Tool::Place);
    tap_r(&mut session, false);
    for tool in [Tool::Paint, Tool::Rectangle, Tool::Line, Tool::Circle] {
        session.set_tool(tool);
        // Off the cell's middle, and the ghost is on its middle, unturned and half seen through.
        session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
        assert_eq!(ghost(&mut session), CELL_2_3, "{tool:?}");
        assert_eq!(session.overlay_quads().1[11], 0.5, "{tool:?}");
        let preview = session.place_preview(1.0).expect("a preview");
        assert_eq!((preview.at, preview.label.as_deref(), preview.facing, preview.square), ((2.5, 3.5), Some("Table"), None, None), "{tool:?}");
    }
    // The eraser deletes, and Fill knows tiles alone.
    for tool in [Tool::Erase, Tool::Fill, Tool::Pick, Tool::Select] {
        session.set_tool(tool);
        session.choose_entity(0);
        session.set_tool(tool);
        assert!(ghost(&mut session).is_empty(), "{tool:?}");
        assert!(session.place_preview(1.0).is_none(), "{tool:?}");
    }
}

#[test]
fn a_shape_dragged_out_shows_its_cells_and_no_entity_ghost() {
    let (mut session, _engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    give_previews(&mut session, SQUARE);
    for tool in [Tool::Rectangle, Tool::Line, Tool::Circle] {
        session.set_tool(tool);
        session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
        session.pointer_move(over(&session, (2, 3)), VIEW);
        assert!(session.place_preview(1.0).is_none(), "{tool:?}");
        assert!(session.entity_ghost_quads().is_none(), "{tool:?}");
        session.pointer_up(over(&session, (2, 3)), VIEW);
    }
    // A stroke of the paint brush goes on showing where the next cell would be.
    session.set_tool(Tool::Paint);
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    assert_eq!(ghost(&mut session), CELL_2_3);
}

#[test]
fn the_label_is_the_web_editor_s() {
    let (mut session, _engine) = placing();
    session.set_prototypes(vec![Prototype { id: "TableWood".into(), name: "wooden table".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);

    // The id in green, and the name under it where it is not the id.
    let preview = session.place_preview(1.0).expect("a preview");
    assert_eq!((preview.label.as_deref(), preview.name.as_deref(), preview.colour), (Some("TableWood"), Some("wooden table"), [0, 255, 100, 204]));
    // It hangs under the cell, the lowest the ghost reaches with no picture yet.
    assert_eq!(preview.below, (2.5, 3.0));

    // With Shift it goes where the cursor is, and says so in light blue.
    session.held.shift = true;
    let preview = session.place_preview(1.0).expect("a preview");
    assert_eq!((preview.label.as_deref(), preview.colour), (Some("TableWood (free)"), [100, 200, 255, 204]));
    session.held.shift = false;

    // A tile 16 points wide or less on screen has no label.
    let sixteen = 32.0 * session.camera.zoom / 16.0;
    let preview = session.place_preview(sixteen).expect("a preview");
    assert_eq!((preview.label, preview.name), (None, None));
    assert!(session.place_preview(sixteen * 0.99).expect("a preview").label.is_some());

    // A name that is the id but for its case, or too long for a line, is not written again.
    for name in ["tablewood", "a table of wood with a very long name indeed"] {
        session.set_prototypes(vec![Prototype { id: "TableWood".into(), name: name.into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
        session.choose_entity(0);
        assert_eq!(session.place_preview(1.0).expect("a preview").name, None, "{name}");
    }
}

#[test]
fn a_dashed_square_stands_in_until_the_engine_sends_the_picture() {
    let (mut session, _engine) = placing();
    choose(&mut session, "SnapgridCenter");
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    assert!(ghost(&mut session).is_empty());
    assert_eq!(session.place_preview(1.0).expect("a preview").square, Some(CELL_2_3));
    // With Shift the square is round the cursor.
    session.held.shift = true;
    assert_eq!(session.place_preview(1.0).expect("a preview").square, Some([(1.75, 3.25), (2.75, 3.25), (2.75, 4.25), (1.75, 4.25)]));
    session.held.shift = false;

    // A picture one cell high and two tall reaches below the cell, and the label hangs under it.
    give_previews(&mut session, [(-0.5, -1.0), (0.5, -1.0), (0.5, 1.0), (-0.5, 1.0)]);
    let preview = session.place_preview(1.0).expect("a preview");
    assert_eq!((preview.square, preview.below), (None, (2.5, 2.5)));
}

#[test]
fn no_entity_ghost_while_space_is_held_a_dialog_is_open_or_off_the_map() {
    let (mut session, _engine) = placing();
    choose(&mut session, "SnapgridCenter");
    give_previews(&mut session, SQUARE);
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    assert!(session.place_preview(1.0).is_some());

    session.held.space = true;
    assert!(session.place_preview(1.0).is_none());
    assert!(ghost(&mut session).is_empty());
    session.held.space = false;

    session.dialog_open = true;
    assert!(session.place_preview(1.0).is_none());
    assert!(ghost(&mut session).is_empty());
    session.dialog_open = false;

    session.pointer_left();
    assert!(session.place_preview(1.0).is_none());
    assert!(ghost(&mut session).is_empty());
}

#[test]
fn the_ghost_is_turned_as_what_will_be_placed() {
    let (mut session, _engine) = placing();
    choose(&mut session, "SnapgridCenter");
    // A rectangle longer than it is wide, off the middle, so that every turn looks different.
    let base = [(0.0, -0.25), (1.0, -0.25), (1.0, 0.25), (0.0, 0.25)];
    give_previews(&mut session, base);
    // A quarter turn clockwise and five degrees more: 265 degrees.
    tap_r(&mut session, false);
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    session.key_up('R');
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);

    let round = |value: f32| (value * 1000.0).round() / 1000.0;
    let wanted: Vec<(f32, f32)> = base.iter().map(|&corner| turn(corner, 265f32.to_radians())).map(|(x, y)| (round(2.5 + x), round(3.5 + y))).collect();
    assert_eq!(ghost(&mut session), wanted);
}

#[test]
fn choosing_another_entity_keeps_the_way_the_next_one_faces() {
    let (mut session, engine) = placing();
    let prototypes = ["Table", "Chair"].map(|id| Prototype { id: id.into(), placement_mode: "SnapgridCenter".into(), ..Default::default() });
    session.set_prototypes(Vec::from(prototypes));
    session.choose_entity(0);
    tap_r(&mut session, false);

    // Choosing takes the place tool in hand, which it is already. The turn stays.
    session.choose_entity(1);
    click(&mut session, (2.5, 3.5));
    let placed = asked(&engine);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0][0]["prototype"], "Chair");
    assert_eq!(placed[0][0]["rotation"].as_f64().unwrap().to_degrees().round(), 270.0);
}

#[test]
fn r_with_the_wheel_turns_what_is_about_to_be_placed() {
    let (mut session, engine) = holding(Tool::Place);
    // Two notches towards the user, the second in parts. R is let go after the wheel.
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    for part in [-0.25, -0.5, -0.25] {
        wheel(&mut session, part);
    }
    session.key_up('R');
    // A turn asks for nothing by itself, and leaves the chair that is selected alone.
    assert!(asked(&engine).is_empty());

    // What is placed next is turned ten degrees clockwise, and not a quarter as well.
    click(&mut session, (2.5, 3.5));
    let placed = asked(&engine);
    assert_eq!((placed.len(), &placed[0][0]["kind"], angle(&placed[0][0])), (1, &json!("spawn"), 350.0));

    // Another hold goes on from there. A notch away from the user turns it back by five.
    session.key_down('R', false);
    wheel(&mut session, 1.0);
    session.key_up('R');
    click(&mut session, (4.5, 3.5));
    assert_eq!(angle(&asked(&engine)[0][0]), 355.0);
}

#[test]
fn shift_r_turns_the_other_way() {
    let (mut session, engine) = holding(Tool::Place);
    // Shift is read as R goes down. It is let go before R is, and the turn is still Shift's.
    session.key_down('R', true);
    session.held.shift = false;
    session.key_up('R');
    click(&mut session, (2.5, 3.5));
    // And R that went down without it is plain, whatever is held as it comes up.
    session.key_down('R', false);
    session.held.shift = true;
    session.key_up('R');
    session.held.shift = false;
    click(&mut session, (4.5, 3.5));

    let placed = asked(&engine);
    assert_eq!([angle(&placed[0][0]), angle(&placed[1][0])], [90.0, 0.0]);
}

#[test]
fn choosing_an_entity_ends_a_hold_of_r_and_keeps_the_turn() {
    let (mut session, engine) = holding(Tool::Place);
    session.key_down('R', false);
    wheel(&mut session, -1.0);
    // Choosing takes the place tool in hand, which it is already, and that ends the hold.
    session.choose_entity(0);
    let zoom = session.camera.zoom;
    wheel(&mut session, -1.0);
    assert!(session.camera.zoom < zoom);
    session.key_up('R');

    // The five degrees stay, and nothing more came of the wheel or of R let go.
    click(&mut session, (2.5, 3.5));
    let placed = asked(&engine);
    assert_eq!((placed.len(), angle(&placed[0][0])), (1, 355.0));
}

#[test]
fn choosing_a_tile_in_the_palette_leaves_the_place_tool_with_nothing_to_place() {
    let (mut session, engine) = placing();
    choose(&mut session, "SnapgridCenter");
    // The tile takes the entity's place in the palette and the paint tool is taken in hand.
    // Then the place tool is taken up again, with a tile in the palette.
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Place);
    assert_eq!((session.chosen_tile(), session.chosen_entity(), session.tool()), (Some("FloorWood"), None, Tool::Place));
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    assert!(session.outlines().is_empty());
    session.set_status("Opened");
    session.pointer_down(at(&session, (2.25, 3.75)), VIEW, Button::Primary);
    // The press says why nothing comes of it.
    assert_eq!(session.status_line(), "Choose an entity to place");
    session.pointer_move(over(&session, (3, 3)), VIEW);
    session.pointer_up(over(&session, (3, 3)), VIEW);
    assert!(asked(&engine).is_empty());

    // An entity chosen after the tile is what the place tool places. Choosing it takes the
    // hint away, which leaves what the line said before it. Any other status is left as it is.
    session.choose_entity(0);
    assert_eq!((session.chosen_tile(), session.chosen_entity()), (None, Some(0)));
    assert_eq!(session.status_line(), "Opened");
    session.set_status("Saved");
    session.choose_entity(0);
    assert_eq!(session.status_line(), "Saved");
    click(&mut session, (2.25, 3.75));
    assert_eq!(asked(&engine), [vec![json!({ "kind": "spawn", "prototype": "Table", "parent": GRID, "x": 2.5, "y": 3.5, "rotation": 0.0 })]]);

    // A tile chosen where an entity was asked for answers the hint as well.
    session.hint = Some(Hint::Entity);
    assert_eq!(session.status_line(), "Choose an entity to place");
    session.choose_tile("FloorWood");
    assert_eq!(session.status_line(), "Saved");
}
