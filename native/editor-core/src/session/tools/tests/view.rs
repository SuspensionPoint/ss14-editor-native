//! The view: panning and zooming.

use super::*;

#[test]
fn a_drag_with_the_pan_tool_or_the_middle_button_moves_the_view() {
    let (mut session, engine) = Session::offline(&scene());
    // A tile to the right and a tile up on the screen: the map follows the cursor.
    session.set_tool(Tool::Pan);
    session.pointer_down((320.0, 320.0), VIEW, Button::Primary);
    session.pointer_move((352.0, 288.0), VIEW);
    session.pointer_up((352.0, 288.0), VIEW);
    assert_eq!((session.camera.x, session.camera.y), (-1.0, -1.0));

    // The middle button does the same with any tool, and the tool does nothing.
    session.set_tool(Tool::Paint);
    session.choose_tile("FloorWood");
    session.pointer_down((320.0, 320.0), VIEW, Button::Middle);
    session.pointer_move((320.0, 352.0), VIEW);
    session.pointer_up((320.0, 352.0), VIEW);
    assert_eq!((session.camera.x, session.camera.y), (-1.0, 0.0));
    assert!(asked(&engine).is_empty());
}

#[test]
fn space_held_pans_with_the_left_button_whatever_the_tool() {
    for tool in Tool::EVERY {
        let (mut session, engine) = holding(tool);
        session.held.space = true;
        let before = (session.camera.x, session.camera.y);
        // Two tiles to the right on the screen, in two moves. Space is let go between them
        // and the pan goes on.
        session.pointer_down((320.0, 320.0), VIEW, Button::Primary);
        session.pointer_move((352.0, 320.0), VIEW);
        session.held.space = false;
        session.pointer_move((384.0, 320.0), VIEW);
        session.pointer_up((384.0, 320.0), VIEW);

        assert_eq!((session.camera.x, session.camera.y), (before.0 - 2.0, before.1), "{tool:?}");
        assert!(asked(&engine).is_empty(), "{tool:?}");
        assert_eq!((session.tool(), &session.selection[..]), (tool, &[CHAIR][..]), "{tool:?}");
    }
}

#[test]
fn the_wheel_zooms_when_no_tool_takes_it() {
    let (mut session, _engine) = Session::offline(&scene());
    let point = (480.0, 200.0);
    let (zoom, under) = (session.camera.zoom, session.world_at(point, VIEW));
    session.wheel(1.0, point, VIEW);
    glide_out(&mut session);

    assert!(session.camera.zoom > zoom);
    // What was under the cursor still is.
    let now = session.world_at(point, VIEW);
    assert!((now.0 - under.0).abs() < 1e-4 && (now.1 - under.1).abs() < 1e-4, "{under:?} became {now:?}");
}

#[test]
fn the_wheel_zooms_with_any_tool_in_hand_and_during_any_drag() {
    // Pressed on the chair, and on empty floor where the select tool begins a box.
    for pressed in [(1.5, 1.5), (0.5, 4.5)] {
        for tool in Tool::EVERY {
            let (mut session, engine) = holding(tool);
            let zoom = session.camera.zoom;
            session.wheel(1.0, at(&session, pressed), VIEW);
            glide_out(&mut session);
            assert!(session.camera.zoom > zoom, "{tool:?} at {pressed:?}");

            // With the button down, which is a drag with every tool but the pick and the fill.
            session.pointer_down(at(&session, pressed), VIEW, Button::Primary);
            session.pointer_move(at(&session, (pressed.0 + 1.0, pressed.1)), VIEW);
            asked(&engine);
            let zoom = session.camera.zoom;
            session.wheel(1.0, at(&session, pressed), VIEW);
            glide_out(&mut session);
            assert!(session.camera.zoom > zoom, "{tool:?} at {pressed:?}");
            assert!(asked(&engine).is_empty(), "{tool:?} at {pressed:?}");
        }
    }
}

/// How far a view zoomed from `from`, in steps of 1.15 times.
fn steps_from(session: &Session, from: f32) -> f32 {
    (session.camera.zoom / from).ln() / 1.15f32.ln()
}

/// Whether the map position is still under the point of the view.
#[track_caller]
fn still_under(session: &Session, point: (f32, f32), world: (f32, f32)) {
    let now = session.world_at(point, VIEW);
    assert!((now.0 - world.0).abs() < 1e-3 && (now.1 - world.1).abs() < 1e-3, "{world:?} became {now:?}");
}

#[test]
fn a_notch_zooms_as_before_spread_over_frames_about_the_same_map_point() {
    let (mut session, _engine) = Session::offline(&scene());
    let point = (480.0, 200.0);
    let (zoom, under) = (session.camera.zoom, session.world_at(point, VIEW));
    session.wheel(1.0, point, VIEW);
    // Nothing yet. The frames give it.
    assert_eq!(session.camera.zoom, zoom);
    assert!(session.zooming());

    // The first frame after the editor stood idle says it took seconds. It gives a part all
    // the same, as a sixtieth of a second would.
    assert!(session.advance_zoom(5.0));
    let first = steps_from(&session, zoom);
    assert!(first > 0.1 && first < 0.4, "{first}");
    still_under(&session, point, under);

    // Each frame after gives a part of what is left, about the same map point, until it runs
    // out. Then no more frames are asked for.
    let mut frames = 1;
    let mut before = session.camera.zoom;
    while session.advance_zoom(1.0 / 60.0) {
        frames += 1;
        assert!(session.camera.zoom > before);
        before = session.camera.zoom;
        still_under(&session, point, under);
    }
    still_under(&session, point, under);
    assert!((5..=20).contains(&frames), "{frames}");
    assert!(!session.zooming() && !session.advance_zoom(1.0 / 60.0));
    // The whole of it is the notch's 0.8 of a step, as when it came at once.
    assert!((steps_from(&session, zoom) - 0.8).abs() < 1e-3, "{}", steps_from(&session, zoom));

    // Towards the user it zooms out by as much.
    session.wheel(-1.0, point, VIEW);
    glide_out(&mut session);
    assert!((session.camera.zoom - zoom).abs() < 1e-4);
}

#[test]
fn a_second_notch_during_a_glide_adds_to_it() {
    let (mut session, _engine) = Session::offline(&scene());
    let point = (480.0, 200.0);
    let zoom = session.camera.zoom;
    session.wheel(1.0, point, VIEW);
    session.advance_zoom(1.0 / 60.0);
    session.advance_zoom(1.0 / 60.0);
    session.wheel(1.0, point, VIEW);
    glide_out(&mut session);
    assert!((steps_from(&session, zoom) - 1.6).abs() < 1e-3, "{}", steps_from(&session, zoom));

    // A notch elsewhere: what is left goes about the newer point.
    let (mut session, _engine) = Session::offline(&scene());
    session.wheel(1.0, point, VIEW);
    session.advance_zoom(1.0 / 60.0);
    let other = (100.0, 500.0);
    let under = session.world_at(other, VIEW);
    session.wheel(1.0, other, VIEW);
    while session.advance_zoom(1.0 / 60.0) {
        still_under(&session, other, under);
    }
    still_under(&session, other, under);
    assert!((steps_from(&session, zoom) - 1.6).abs() < 1e-3, "{}", steps_from(&session, zoom));
}

#[test]
fn what_takes_the_wheel_takes_whole_notches_at_once_and_owes_no_zoom() {
    // R held with the select tool turns the chair five degrees, at once.
    let (mut session, engine) = holding(Tool::Select);
    let view = (session.camera.x, session.camera.y, session.camera.zoom);
    session.key_down('R', false);
    session.wheel(-1.0, (480.0, 200.0), VIEW);
    assert!(!session.zooming() && !session.advance_zoom(1.0 / 60.0));
    session.key_up('R');
    assert_eq!(asked(&engine).iter().map(|ops| turns(ops)).collect::<Vec<_>>(), [[(CHAIR, 355.0)]]);

    // The select tool's list steps a whole notch at once.
    let (mut session, _engine) = selecting();
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (1.5, 1.5), 0)]);
    session.selection = vec![WALL];
    session.wheel(-1.0, at(&session, (1.5, 1.5)), VIEW);
    assert_eq!(session.selection, [CHAIR]);
    assert!(!session.zooming() && !session.advance_zoom(1.0 / 60.0));
    assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), view);
}
