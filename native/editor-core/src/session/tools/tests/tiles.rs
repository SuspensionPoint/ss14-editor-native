//! The tile tools.

use super::*;

#[test]
fn a_stroke_paints_every_cell_it_crosses() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Paint);
    drag(&mut session, (0, 0), (3, 0));

    assert_eq!(cells(asked(&engine).iter().flatten()), [(0, 0), (1, 0), (2, 0), (3, 0)]);
}

#[test]
fn a_stroke_is_one_step_to_undo_and_the_next_stroke_is_another() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Paint);
    drag(&mut session, (0, 0), (3, 0));
    drag(&mut session, (0, 1), (1, 1));

    // Each stroke asks twice: for the cell pressed on, and for the cells moved over.
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Paint tiles"; 4]);
    assert!(one_stroke(&edits[..2]) && one_stroke(&edits[2..]));
    assert_ne!(edits[0].1, edits[2].1);
}

#[test]
fn a_stroke_goes_on_from_the_cell_it_last_reached() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Paint);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 0)), VIEW);
    session.pointer_move(over(&session, (3, 0)), VIEW);
    session.pointer_up(over(&session, (3, 0)), VIEW);

    let edits = edits(&engine);
    assert_eq!(cells(edits.iter().flat_map(|edit| &edit.2)), [(0, 0), (1, 0), (2, 0), (3, 0)]);
    assert!(one_stroke(&edits));
}

#[test]
fn a_stroke_goes_on_after_a_move_inside_the_cell_it_is_in() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Paint);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (0.75, 0.5)), VIEW);
    session.pointer_move(over(&session, (1, 0)), VIEW);
    session.pointer_up(over(&session, (1, 0)), VIEW);
    assert_eq!(cells(asked(&engine).iter().flatten()), [(0, 0), (1, 0)]);
}

#[test]
fn painting_the_tile_already_there_asks_for_nothing() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorSteel");
    session.set_tool(Tool::Paint);
    drag(&mut session, (0, 0), (3, 0));

    assert!(asked(&engine).is_empty());
}

#[test]
fn the_eraser_asks_for_space_whatever_tile_is_chosen() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Erase);
    drag(&mut session, (0, 0), (1, 0));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Erase tiles"; 2]);
    assert!(one_stroke(&edits));
    let ops: Vec<&Value> = edits.iter().flat_map(|edit| &edit.2).collect();
    assert_eq!(cells(ops.iter().copied()), [(0, 0), (1, 0)]);
    assert!(ops.iter().all(|op| op["tile"] == "Space"));
}

#[test]
fn a_rectangle_fills_from_corner_to_corner() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Rectangle);
    drag(&mut session, (1, 1), (2, 3));

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Rectangle of tiles", None));
    let mut filled = cells(&edits[0].2);
    filled.sort_unstable();
    assert_eq!(filled, [(1, 1), (1, 2), (1, 3), (2, 1), (2, 2), (2, 3)]);
}

#[test]
fn a_rectangle_being_dragged_is_the_only_outline() {
    let (mut session, _engine) = Session::offline(&scene());
    session.set_tool(Tool::Rectangle);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    assert_eq!(outline(&session), (OutlineStyle::Hover, [(2.0, 3.0), (3.0, 3.0), (3.0, 4.0), (2.0, 4.0)]));

    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    assert_eq!(outline(&session), (OutlineStyle::Drag, [(1.0, 1.0), (3.0, 1.0), (3.0, 4.0), (1.0, 4.0)]));
}

#[test]
fn a_fill_covers_the_whole_floor_in_one_edit() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Fill);
    click(&mut session, (2.5, 2.5));

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Fill tiles", None));
    let mut filled = cells(&edits[0].2);
    filled.sort_unstable();
    let floor: Vec<(i64, i64)> = (0..6).flat_map(|x| (0..6).map(move |y| (x, y))).collect();
    assert_eq!(filled, floor);
    assert!(edits[0].2.iter().all(|op| op["tile"] == "FloorWood"));
}

#[test]
fn a_rectangle_being_dragged_says_its_size() {
    let (mut session, _engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Rectangle);
    session.pointer_move(over(&session, (1, 1)), VIEW);
    assert_eq!(session.drag_label(), None);

    // Across by three cells and up by two, said over the middle of its top edge.
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 2)), VIEW);
    assert_eq!(session.drag_label(), Some(((2.5, 3.0), "3x2".to_owned())));
    // Back past where it began.
    session.pointer_move(over(&session, (0, 1)), VIEW);
    assert_eq!(session.drag_label(), Some(((1.0, 2.0), "2x1".to_owned())));
    session.pointer_up(over(&session, (0, 1)), VIEW);
    assert_eq!(session.drag_label(), None);
}

/// A map with one grid of `count` cells of steel and nothing on them, laid in rows of 250
/// from the origin, the last row as long as is left.
fn floor_of(count: usize) -> String {
    let tiles: Vec<String> = (0..count).map(|i| format!("{},{},1,0,0", i % 250, i / 250)).collect();
    let tiles = tiles.join(",");
    format!(
        r#"{{"document":{{"id":1,"source":"floor.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[{tiles}]}}],
            "entities":[],
            "history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#
    )
}

#[test]
fn a_fill_of_fifty_thousand_is_done_and_one_more_is_refused() {
    let (mut session, engine) = Session::offline(&floor_of(50_000));
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Fill);
    click(&mut session, (0.5, 0.5));
    let filled = edits(&engine);
    assert_eq!((filled.len(), filled[0].2.len()), (1, 50_000));

    let (mut session, engine) = Session::offline(&floor_of(50_001));
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Fill);
    click(&mut session, (0.5, 0.5));
    assert!(edits(&engine).is_empty());
    assert_eq!(session.status_line(), "Not filled: the area has more than 50,000 tiles");
}

#[test]
fn picking_takes_the_tile_under_the_click_and_goes_back_to_painting() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Pick);
    // Off the floor there is nothing to pick.
    click(&mut session, (-0.5, 0.5));
    assert_eq!((session.chosen_tile(), session.tool()), (Some("FloorWood"), Tool::Pick));

    click(&mut session, (2.5, 2.5));
    assert_eq!((session.chosen_tile(), session.tool()), (Some("FloorSteel"), Tool::Paint));
    assert!(asked(&engine).is_empty());
}

#[test]
fn picking_a_tile_ends_a_drag_and_goes_to_the_paint_tool() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Pick);
    // An interface hands on one button at a time. Here the middle button is down and panning
    // when the primary button picks. A drag can be live under a pick in this way, or as a pan
    // begun with Space held and the primary button.
    session.pointer_down((320.0, 320.0), VIEW, Button::Middle);
    session.pointer_move((352.0, 320.0), VIEW);
    let view = (session.camera.x, session.camera.y);
    session.pointer_down(over(&session, (2, 2)), VIEW, Button::Primary);
    assert_eq!((session.chosen_tile(), session.tool()), (Some("FloorSteel"), Tool::Paint));
    assert_eq!(session.status_line(), "Picked FloorSteel");

    // The pan ended with the pick. A move after it moves nothing and paints nothing.
    session.pointer_move((384.0, 352.0), VIEW);
    session.pointer_up((384.0, 352.0), VIEW);
    assert_eq!((session.camera.x, session.camera.y), view);
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_room_tool_with_an_entity_chosen_hints_for_a_tile() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    assert_eq!((session.chosen_tile(), session.chosen_entity()), (None, Some(0)));
    session.set_tool(Tool::Room);
    session.set_status("Opened");
    click(&mut session, (0.5, 0.5));
    // The press says why nothing comes of it.
    assert_eq!(session.status_line(), "Choose a tile to paint");
    assert!(asked(&engine).is_empty());

    // Choosing a tile takes the hint away, which leaves what the line said before it. Any
    // other status is left as it is.
    session.choose_tile("FloorWood");
    assert_eq!(session.status_line(), "Opened");
    session.set_status("Saved");
    session.choose_tile("FloorSteel");
    assert_eq!(session.status_line(), "Saved");

    // An entity chosen where a tile was asked for answers the hint as well.
    session.hint = Some(Hint::Tile);
    assert_eq!(session.status_line(), "Choose a tile to paint");
    session.choose_entity(0);
    assert_eq!(session.status_line(), "Saved");
}

/// Puts one prototype of this id in the palette and chooses it.
fn choose_id(session: &mut Session, id: &str) {
    session.set_prototypes(vec![Prototype { id: id.into(), name: id.into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
}

/// Where spawn ops put their entity, in the order of the ops. Each puts down the chosen
/// prototype on the grid, unturned.
#[track_caller]
fn spawned<'a>(ops: impl IntoIterator<Item = &'a Value>, prototype: &str) -> Vec<(f64, f64)> {
    ops.into_iter()
        .map(|op| {
            assert_eq!((op["kind"].as_str(), op["prototype"].as_str(), op["parent"].as_i64(), op["rotation"].as_f64()), (Some("spawn"), Some(prototype), Some(GRID as i64), Some(0.0)), "{op}");
            (op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())
        })
        .collect()
}

#[test]
fn paint_with_an_entity_chosen_puts_one_on_each_cell_crossed() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    // Choosing an entity with Paint at a point takes the place tool, which is Paint at a point
    // on entities. The paint tool's brush still works with one.
    assert_eq!(session.tool(), Tool::Place);
    session.set_tool(Tool::Paint);
    drag(&mut session, (0, 0), (3, 0));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Paint entities"; 2]);
    assert_eq!(spawned(edits.iter().flat_map(|edit| &edit.2), "Table"), [(0.5, 0.5), (1.5, 0.5), (2.5, 0.5), (3.5, 0.5)]);
    // No tile is asked for, and none is drawn ahead.
    assert!(session.overlay_quads().0.is_empty());
}

#[test]
fn a_cell_that_holds_the_same_prototype_is_skipped() {
    let (mut session, engine) = Session::offline(&scene());
    choose_id(&mut session, "Chair");
    drag(&mut session, (0, 1), (2, 1));
    assert_eq!(spawned(asked(&engine).iter().flatten(), "Chair"), [(0.5, 1.5), (2.5, 1.5)]);

    // The crowbar in the wall is inside something, and does not hold its cell.
    choose_id(&mut session, "Crowbar");
    click(&mut session, (3.5, 1.5));
    assert_eq!(spawned(asked(&engine).iter().flatten(), "Crowbar"), [(3.5, 1.5)]);
}

#[test]
fn a_stroke_does_not_put_two_on_a_cell_it_crosses_twice() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Paint);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 0)), VIEW);
    session.pointer_move(over(&session, (0, 0)), VIEW);
    session.pointer_up(over(&session, (0, 0)), VIEW);

    assert_eq!(spawned(asked(&engine).iter().flatten(), "Table"), [(0.5, 0.5), (1.5, 0.5), (2.5, 0.5)]);
}

#[test]
fn a_cell_with_no_tile_gets_no_entity() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Paint);
    // From the floor out over space.
    drag(&mut session, (1, 0), (-2, 0));
    assert_eq!(spawned(asked(&engine).iter().flatten(), "Table"), [(1.5, 0.5), (0.5, 0.5)]);
}

#[test]
fn a_stroke_of_entities_is_one_stroke() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Paint);
    drag(&mut session, (0, 0), (3, 0));
    drag(&mut session, (0, 4), (1, 4));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Paint entities"; 4]);
    assert!(one_stroke(&edits[..2]) && one_stroke(&edits[2..]));
    assert_ne!(edits[0].1, edits[2].1);
}

#[test]
fn erase_with_an_entity_chosen_deletes_that_prototype_and_leaves_the_tile() {
    let (mut session, engine) = Session::offline(&scene());
    choose_id(&mut session, "Chair");
    session.set_erase_only(true);
    session.set_tool(Tool::Erase);
    assert_eq!(session.chosen_entity(), Some(0));
    // Over the chair, on and back. The mirror still has it, and it is asked for once.
    session.pointer_down(over(&session, (0, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 1)), VIEW);
    session.pointer_move(over(&session, (0, 1)), VIEW);
    session.pointer_up(over(&session, (0, 1)), VIEW);

    let erased = edits(&engine);
    assert_eq!(erased.len(), 1);
    assert_eq!((erased[0].0, &erased[0].2), ("Erase entities", &vec![json!({ "kind": "delete", "uid": CHAIR })]));
    assert!(erased[0].1.is_some());

    // Erase with a tile chosen takes tiles away: none with Only on, since the cell is steel,
    // and the steel with it off.
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Erase);
    click(&mut session, (0.5, 0.5));
    assert!(edits(&engine).iter().all(|edit| edit.2.is_empty()));
    session.set_erase_only(false);
    click(&mut session, (0.5, 0.5));
    assert_eq!(labels(&edits(&engine)), ["Erase tiles"]);
}

#[test]
fn erase_with_only_on_leaves_other_prototypes_and_contained_entities() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_erase_only(true);
    // The crowbar is inside the wall.
    choose_id(&mut session, "Crowbar");
    session.set_tool(Tool::Erase);
    drag(&mut session, (0, 1), (5, 1));
    // The chair and the wall are of other prototypes.
    choose_id(&mut session, "Table");
    drag(&mut session, (0, 1), (5, 1));

    assert!(asked(&engine).is_empty());
}

#[test]
fn a_rectangle_of_entities_is_one_edit() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Rectangle);
    drag(&mut session, (1, 1), (2, 3));

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Rectangle of entities", None));
    let mut put = spawned(&edits[0].2, "Table");
    put.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(put, [(1.5, 1.5), (1.5, 2.5), (1.5, 3.5), (2.5, 1.5), (2.5, 2.5), (2.5, 3.5)]);
}

#[test]
fn a_rectangle_keeps_the_brush_it_was_pressed_with() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Rectangle);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (1, 0)), VIEW);
    session.choose_tile("FloorWood");
    session.pointer_up(over(&session, (1, 0)), VIEW);
    assert_eq!(labels(&edits(&engine)), ["Rectangle of entities"]);

    // And the other way about.
    session.pointer_down(over(&session, (0, 4)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (1, 4)), VIEW);
    session.choose_entity(0);
    assert_eq!(session.tool(), Tool::Rectangle);
    session.pointer_up(over(&session, (1, 4)), VIEW);
    assert_eq!(labels(&edits(&engine)), ["Rectangle of tiles"]);
}

#[test]
fn a_rectangle_of_more_than_ten_thousand_entities_is_refused() {
    // Forty rows of 250 cells.
    let (mut session, engine) = Session::offline(&floor_of(10_000));
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Rectangle);
    drag(&mut session, (0, 0), (249, 39));
    let put = edits(&engine);
    assert_eq!((put.len(), put[0].2.len()), (1, 10_000));

    // One row more is refused, though the row has no tiles.
    drag(&mut session, (0, 0), (249, 40));
    assert!(edits(&engine).is_empty());
    assert_eq!(session.status_line(), "Not placed: the rectangle has more than 10,000 cells");

    // Tiles have no such limit.
    session.choose_tile("FloorWood");
    drag(&mut session, (0, 0), (249, 40));
    assert_eq!(labels(&edits(&engine)), ["Rectangle of tiles"]);
}

#[test]
fn choosing_an_entity_keeps_a_brush_or_fill_in_hand_and_leaves_the_rest_for_place() {
    let (mut session, _engine) = Session::offline(&scene());
    for tool in [Tool::Erase, Tool::Fill, Tool::Rectangle, Tool::Line, Tool::Circle, Tool::Select] {
        session.set_tool(tool);
        choose(&mut session, "SnapgridCenter");
        assert_eq!(session.tool(), tool);
    }
    // Paint's shape is a point after this, so Paint on entities is the place tool. Paint at a
    // point with an entity chosen is the place tool too.
    session.set_tool(Tool::Paint);
    for tool in [Tool::Pan, Tool::Paint, Tool::Pick, Tool::Area, Tool::Place] {
        session.set_tool(tool);
        choose(&mut session, "SnapgridCenter");
        assert_eq!(session.tool(), Tool::Place, "{tool:?}");
    }
}

#[test]
fn choosing_a_tile_goes_to_the_paint_tool_unless_a_tool_that_puts_tiles_down_is_in_hand() {
    let (mut session, _engine) = Session::offline(&scene());
    session.set_tool(Tool::Select);
    session.choose_tile("FloorWood");
    assert_eq!((session.palette(), session.tool()), (&Palette::Tile("FloorWood".into()), Tool::Paint));

    for tool in [Tool::Paint, Tool::Fill, Tool::Rectangle, Tool::Line, Tool::Circle] {
        session.set_tool(tool);
        session.choose_tile("FloorSteel");
        assert_eq!((session.palette(), session.tool()), (&Palette::Tile("FloorSteel".into()), tool));
    }
    // Every other tool is put down for the paint tool, at a point since that is its shape now.
    // The eraser is one of them: it takes tiles away.
    session.set_tool(Tool::Paint);
    for tool in [Tool::Pan, Tool::Erase, Tool::Pick, Tool::Select, Tool::Place] {
        session.set_tool(tool);
        session.choose_tile("FloorSteel");
        assert_eq!(session.tool(), Tool::Paint, "{tool:?}");
    }

    // The tile takes a chosen entity's place, and the place tool is put down with it.
    choose(&mut session, "SnapgridCenter");
    session.choose_tile("FloorWood");
    assert_eq!((session.palette(), session.tool()), (&Palette::Tile("FloorWood".into()), Tool::Paint));
}

#[test]
fn a_painted_tile_is_drawn_before_the_engine_answers() {
    // The corners of each rectangle, and the texture it is drawn with.
    let drawn = |quads: &[f32]| -> Vec<([(f32, f32); 4], u32)> {
        quads.chunks_exact(FLOATS_PER_QUAD).map(|quad| ([(quad[0], quad[1]), (quad[2], quad[3]), (quad[4], quad[5]), (quad[6], quad[7])], quad[12] as u32)).collect()
    };
    let cell = |x: f32, y: f32| [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];

    let (mut session, engine) = Session::offline(&scene());
    // The engine being ready says where each tile's picture is. Here it is known for wood alone.
    session.tile_sprites.insert("FloorWood".into(), Some("/Textures/Tiles/wood.png".into()));
    session.choose_tile("FloorWood");
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 0)), VIEW);
    session.pointer_up(over(&session, (2, 0)), VIEW);
    // Off the map, so that no ghost of the tile is drawn with them.
    session.pointer_left();
    let asked_for = numbers(&engine);
    assert_eq!(asked_for.len(), 2);

    // One rectangle a cell with the tile's picture, to go under everything but the tiles.
    let (under, over_all) = session.overlay_quads();
    let wood = session.lists.local_texture("/Textures/Tiles/wood.png", 0, 32, 32);
    assert_eq!(drawn(&under), [(cell(0.0, 0.0), wood), (cell(1.0, 0.0), wood), (cell(2.0, 0.0), wood)]);
    assert!(over_all.is_empty());

    // The engine's answer to the press takes the cell pressed on away, and its answer to the
    // move takes the rest.
    session.answered(asked_for[0]);
    assert_eq!(drawn(&session.overlay_quads().0), [(cell(1.0, 0.0), wood), (cell(2.0, 0.0), wood)]);
    session.answered(asked_for[1]);
    assert!(session.overlay_quads().0.is_empty());

    // An erased cell is drawn ahead as the empty canvas.
    session.set_tool(Tool::Erase);
    click(&mut session, (4.5, 4.5));
    assert_eq!(drawn(&session.overlay_quads().0), [(cell(4.0, 4.0), WHITE_TEXTURE)]);
    session.answered(numbers(&engine)[0]);

    // A tile whose picture is not known is asked for and not drawn ahead.
    session.choose_tile("FloorDark");
    click(&mut session, (4.5, 4.5));
    assert_eq!(numbers(&engine).len(), 1);
    assert!(session.overlay_quads().0.is_empty());
}

#[test]
fn the_chosen_tile_shows_half_seen_through_where_a_press_would_begin() {
    // The rectangles drawn under the entities: their corners, colour and texture.
    let drawn = |quads: &[f32]| -> Vec<([f32; 8], [f32; 4], u32)> {
        quads.chunks_exact(FLOATS_PER_QUAD).map(|quad| (quad[..8].try_into().unwrap(), quad[8..12].try_into().unwrap(), quad[12] as u32)).collect()
    };
    let (mut session, _engine) = Session::offline(&scene());
    session.tile_sprites.insert("FloorWood".into(), Some("/Textures/Tiles/wood.png".into()));
    session.choose_tile("FloorWood");
    let wood = session.lists.local_texture("/Textures/Tiles/wood.png", 0, 32, 32);
    let ghost = vec![([2.0, 3.0, 3.0, 3.0, 3.0, 4.0, 2.0, 4.0], [1.0, 1.0, 1.0, 0.5], wood)];
    for tool in [Tool::Paint, Tool::Fill, Tool::Rectangle, Tool::Line, Tool::Circle] {
        session.set_tool(tool);
        session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
        assert_eq!(drawn(&session.overlay_quads().0), ghost, "{tool:?}");
        // It has no label: that is the entities'.
        assert!(session.place_preview(1.0).is_none(), "{tool:?}");
    }
    // Not with a tool that puts no tile down.
    for tool in [Tool::Erase, Tool::Pick, Tool::Select, Tool::Place, Tool::Area] {
        session.set_tool(tool);
        assert!(session.overlay_quads().0.is_empty(), "{tool:?}");
    }

    // Not while a shape is dragged out.
    session.set_tool(Tool::Rectangle);
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    assert!(session.overlay_quads().0.is_empty());
    session.pointer_left();

    // Nor while Space is held, a dialog is open, or the cursor is off the map.
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    assert_eq!(drawn(&session.overlay_quads().0), ghost);
    session.held.space = true;
    assert!(session.overlay_quads().0.is_empty());
    session.held.space = false;
    session.dialog_open = true;
    assert!(session.overlay_quads().0.is_empty());
    session.dialog_open = false;
    session.pointer_left();
    assert!(session.overlay_quads().0.is_empty());

    // Nor for a tile whose picture is not known.
    session.choose_tile("FloorDark");
    session.pointer_move(at(&session, (2.25, 3.75)), VIEW);
    assert!(session.overlay_quads().0.is_empty());
}

#[test]
fn fill_with_an_entity_puts_one_on_each_cell_of_the_floor_that_lacks_it() {
    let (mut session, engine) = Session::offline(&scene());
    choose_id(&mut session, "Chair");
    session.set_tool(Tool::Fill);
    click(&mut session, (0.5, 0.5));

    let edits = edits(&engine);
    assert_eq!((labels(&edits), edits[0].1), (vec!["Fill entities"], None));
    // Every cell of the six by six floor but the chair's own.
    let put = spawned(&edits[0].2, "Chair");
    assert_eq!(put.len(), 35);
    assert!(!put.contains(&(1.5, 1.5)));
}

#[test]
fn fill_with_an_entity_or_a_decal_refuses_space_and_an_area_past_ten_thousand() {
    let (mut session, engine) = Session::offline(&scene());
    choose_id(&mut session, "Chair");
    session.set_tool(Tool::Fill);
    click(&mut session, (-2.5, 0.5));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not filled: entities and decals fill a floor, not space");

    let (mut session, engine) = Session::offline(&floor_of(10_001));
    choose_id(&mut session, "Chair");
    session.set_tool(Tool::Fill);
    click(&mut session, (0.5, 0.5));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not filled: the area has more than 10,000 cells");
}

#[test]
fn fill_with_nothing_in_hand_for_its_target_asks_for_one() {
    let (mut session, engine) = Session::offline(&scene());
    session.take_bar_tool(crate::toolbox::BarTool::Fill);
    session.set_target(crate::toolbox::Target::Decals);
    click(&mut session, (0.5, 0.5));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Choose a decal to place");
}
