//! What holds whatever tool is in hand.

use super::*;

#[test]
fn a_secondary_press_does_nothing_with_any_tool_but_select() {
    // The select tool clears the selection with it, which its own tests cover. The area tool
    // clears its region, and has none here. The cable and pipe tools remove their own kind
    // along the drag, and there is none here either.
    for tool in Tool::EVERY.into_iter().filter(|&tool| tool != Tool::Select) {
        let (mut session, engine) = holding(tool);
        let palette = session.palette().clone();
        let view = (session.camera.x, session.camera.y, session.camera.zoom);
        // Pressed on the chair, and dragged across the floor to past the wall.
        session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Secondary);
        session.pointer_move(over(&session, (3, 3)), VIEW);
        session.pointer_move(over(&session, (4, 4)), VIEW);
        assert!(session.outlines().iter().all(|outline| outline.style == OutlineStyle::Hover), "{tool:?}");
        session.pointer_up(over(&session, (4, 4)), VIEW);

        assert!(asked(&engine).is_empty(), "{tool:?}");
        assert_eq!((session.tool(), &session.selection[..], session.palette()), (tool, &[CHAIR][..], &palette), "{tool:?}");
        assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), view, "{tool:?}");
    }
}

#[test]
fn a_drag_ends_with_nothing_more_asked_when_the_pointer_is_lost() {
    // Pressed on the chair, and on empty floor where the select tool begins a box.
    for pressed in [(1.5, 1.5), (0.5, 4.5)] {
        for tool in Tool::EVERY {
            let (mut session, engine) = holding(tool);
            session.pointer_down(at(&session, pressed), VIEW, Button::Primary);
            session.pointer_move(at(&session, (pressed.0 + 1.0, pressed.1)), VIEW);
            asked(&engine);
            let before = (session.selection.clone(), (session.camera.x, session.camera.y, session.camera.zoom));

            // The button is still down as far as the session was told, and what follows is a
            // bare hover and a release that comes late.
            session.pointer_left();
            session.pointer_move(at(&session, (pressed.0 + 2.0, pressed.1)), VIEW);
            session.pointer_move(at(&session, (pressed.0 + 3.0, pressed.1 + 1.0)), VIEW);
            assert!(session.outlines().iter().all(|outline| outline.style == OutlineStyle::Hover), "{tool:?} at {pressed:?}");
            session.pointer_up(at(&session, (pressed.0 + 3.0, pressed.1 + 1.0)), VIEW);

            assert!(asked(&engine).is_empty(), "{tool:?} at {pressed:?}");
            assert!(session.outlines().iter().all(|outline| outline.style == OutlineStyle::Hover), "{tool:?} at {pressed:?}");
            assert_eq!((session.selection.clone(), (session.camera.x, session.camera.y, session.camera.zoom)), before, "{tool:?} at {pressed:?}");
        }
    }
}

#[test]
fn changing_the_tool_ends_a_drag() {
    // Pressed on the chair, and on empty floor where the select tool begins a box.
    for pressed in [(1.5, 1.5), (0.5, 4.5)] {
        for tool in Tool::EVERY {
            let (mut session, engine) = holding(tool);
            session.pointer_down(at(&session, pressed), VIEW, Button::Primary);
            session.pointer_move(at(&session, (pressed.0 + 1.0, pressed.1)), VIEW);
            // Every tool has begun a drag but the pick and the fill, which are done with the
            // press, and the link tool, which drags nothing.
            let begins = !matches!(tool, Tool::Pick | Tool::Fill | Tool::Link);
            assert_eq!(!matches!(session.drag, Drag::None), begins, "{tool:?} at {pressed:?}");
            asked(&engine);

            // Another tool is taken in hand with the button still down.
            let next = if tool == Tool::Fill { Tool::Pick } else { Tool::Fill };
            session.set_tool(next);
            assert!(matches!(session.drag, Drag::None), "{tool:?} at {pressed:?}");
            let before = (session.selection.clone(), (session.camera.x, session.camera.y, session.camera.zoom));

            // What follows is a bare hover and a release that comes late.
            session.pointer_move(at(&session, (pressed.0 + 2.0, pressed.1)), VIEW);
            session.pointer_move(at(&session, (pressed.0 + 3.0, pressed.1 + 1.0)), VIEW);
            assert!(session.outlines().iter().all(|outline| outline.style == OutlineStyle::Hover), "{tool:?} at {pressed:?}");
            session.pointer_up(at(&session, (pressed.0 + 3.0, pressed.1 + 1.0)), VIEW);

            assert!(asked(&engine).is_empty(), "{tool:?} at {pressed:?}");
            assert_eq!(session.tool(), next, "{tool:?} at {pressed:?}");
            assert_eq!((session.selection.clone(), (session.camera.x, session.camera.y, session.camera.zoom)), before, "{tool:?} at {pressed:?}");
        }
    }
}

#[test]
fn r_turns_with_an_entity_tool_and_chooses_the_rectangle_otherwise() {
    for tool in Tool::EVERY {
        let (mut session, engine) = holding(tool);
        // The key is taken either way as it goes down, and nothing is asked for yet. Only a
        // tool that does not turn things gives way to the rectangle tool. The link tool is an
        // entity tool that turns nothing.
        assert!(session.key_down('R', false), "{tool:?}");
        assert!(asked(&engine).is_empty(), "{tool:?}");
        let turns = (tool.is_entity_tool() && tool != Tool::Link) || tool == Tool::Area;
        assert_eq!(session.tool(), if turns { tool } else { Tool::Rectangle }, "{tool:?}");
        session.key_up('R');
        let asked_for = labels(&edits(&engine));
        match tool {
            // The area tool stays in hand. It has no region here, and nothing is turned.
            Tool::Area => assert_eq!((session.tool(), asked_for), (Tool::Area, Vec::new())),
            // The select tool stays in hand, and the chair it has selected is turned.
            Tool::Select => assert_eq!((session.tool(), asked_for), (Tool::Select, vec!["Rotate"])),
            // The place tool stays in hand and asks for nothing. What it places next is turned.
            Tool::Place => {
                assert_eq!((session.tool(), asked_for), (Tool::Place, Vec::new()));
                click(&mut session, (2.5, 3.5));
                assert_ne!(asked(&engine)[0][0]["rotation"], json!(0.0));
            }
            Tool::Pan | Tool::Paint | Tool::Erase | Tool::Pick | Tool::Fill | Tool::Rectangle | Tool::Line | Tool::Circle | Tool::Room | Tool::Link | Tool::Cable | Tool::Pipe => {
                assert_eq!((session.tool(), asked_for), (Tool::Rectangle, Vec::new()), "{tool:?}");
            }
        }
    }
}

#[test]
fn r_turns_clockwise_and_shift_r_the_other_way() {
    // The angle an op asks for, in whole degrees.
    let degrees = |op: &Value| op["rotation"].as_f64().unwrap().to_degrees().round();

    // The selection. R is clockwise, which takes a quarter turn off the angle: the chair stands
    // at 0 and is asked to minus 90 degrees, as a map file writes that angle. Shift+R adds a
    // quarter turn. Nothing answers here, so the chair still stands at 0 for the second key,
    // and is asked to 90.
    let (mut session, engine) = holding(Tool::Select);
    tap_r(&mut session, false);
    tap_r(&mut session, true);
    let turns = asked(&engine);
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().all(|ops| ops.len() == 1 && ops[0]["kind"] == "rotate" && ops[0]["uid"] == CHAIR));
    assert_eq!([degrees(&turns[0][0]), degrees(&turns[1][0])], [-90.0, 90.0]);

    // What the place tool places next. It begins at 0 and turns the same way: R makes it 270
    // degrees. Shift+R twice from there passes 0 and makes it 90. A turn asks for nothing by
    // itself, and leaves the chair that is selected alone.
    let (mut session, engine) = holding(Tool::Place);
    tap_r(&mut session, false);
    assert!(asked(&engine).is_empty());
    click(&mut session, (2.5, 3.5));
    tap_r(&mut session, true);
    tap_r(&mut session, true);
    click(&mut session, (4.5, 3.5));
    let placed = asked(&engine);
    assert_eq!(placed.len(), 2);
    assert!(placed.iter().all(|ops| ops.len() == 1 && ops[0]["kind"] == "spawn"));
    assert_eq!([degrees(&placed[0][0]), degrees(&placed[1][0])], [270.0, 90.0]);
}

#[test]
fn a_hint_stands_over_the_status_until_it_is_answered_or_a_message_takes_its_place() {
    let (mut session, _engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(Tool::Fill);
    session.set_status("Saved");

    // A press with no tile chosen asks for one, over what the line said.
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    assert_eq!(session.status_line(), "Choose a tile to paint");
    // A choice answers the hint, and the line says again what it said before.
    session.choose_tile("FloorWood");
    assert_eq!(session.status_line(), "Saved");

    // The place tool asks for an entity, now that the palette holds a tile.
    session.set_tool(Tool::Place);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    assert_eq!(session.status_line(), "Choose an entity to place");
    // A message that comes while the hint stands takes its place.
    session.set_status("Opening map.yml");
    assert_eq!(session.status_line(), "Opening map.yml");
    // The hint is gone for good, and a choice after it changes nothing on the line.
    session.choose_entity(0);
    assert_eq!(session.status_line(), "Opening map.yml");
}

#[test]
fn the_controls_list_names_every_tool_key() {
    // A toolbar tool's row is under its key, and begins with its name.
    for tool in crate::toolbox::BarTool::ALL {
        let key = tool.key().to_string();
        let does = CONTROLS.iter().find(|(keys, _)| *keys == key).map(|(_, does)| *does).unwrap_or_else(|| panic!("no row for {key}"));
        assert!(does.starts_with(tool.name()), "{key}: {does}");
    }
    // Every other tool key has a row of its own too.
    for key in crate::toolbox::TOOL_KEYS {
        assert!(CONTROLS.iter().any(|(keys, _)| *keys == key.to_string()), "no row for {key}");
    }
    // Each row is there once.
    let mut keys: Vec<&str> = CONTROLS.iter().map(|(keys, _)| *keys).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), CONTROLS.len());
}

#[test]
fn a_move_is_coalesced_and_a_stroke_is_not() {
    // A move asks for every place the cursor reaches. Of those waiting only the last is worth
    // the engine's time, so each may be passed over for a later one.
    let (mut session, engine) = selecting();
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (2.25, 2.0)), VIEW);
    session.pointer_move(at(&session, (2.5, 2.0)), VIEW);
    session.pointer_up(at(&session, (2.5, 2.0)), VIEW);
    assert_eq!(coalesced(&engine), [("Move", true); 2]);

    // Each edit of a stroke of the brush or the eraser, or of a row of the place tool, sets
    // something of its own. None may be passed over.
    // The place tool puts one on the cell between as well, which the cursor jumped over.
    for (tool, label, count) in [(Tool::Paint, "Paint tiles", 2), (Tool::Erase, "Erase tiles", 2), (Tool::Place, "Place entity", 3)] {
        let (mut session, engine) = holding(tool);
        drag(&mut session, (0, 4), (2, 4));
        assert_eq!(coalesced(&engine), vec![(label, false); count], "{tool:?}");
    }
}
