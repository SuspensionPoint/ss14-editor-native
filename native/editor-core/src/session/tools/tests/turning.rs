//! R turning the entity in hand for every tool that puts one down: the shapes and Fill as
//! well as the place tool.

use super::*;

/// The angle of each entity an edit spawns, in degrees.
fn spawned(ops: &[Value]) -> Vec<f64> {
    ops.iter().filter(|op| op["kind"] == "spawn").map(angle).collect()
}

/// A session with a table chosen and a tool in hand.
fn table_in(tool: Tool) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_tool(tool);
    (session, engine)
}

#[test]
fn r_turns_the_entity_a_shape_puts_down_and_keeps_the_shape_in_hand() {
    for tool in [Tool::Line, Tool::Rectangle, Tool::Circle] {
        let (mut session, engine) = table_in(tool);
        tap_r(&mut session, false);
        assert_eq!(session.tool(), tool);
        drag(&mut session, (0, 4), (1, 4));
        let all: Vec<f64> = asked(&engine).iter().flat_map(|ops| spawned(ops)).collect();
        assert!(!all.is_empty() && all.iter().all(|&angle| angle == 270.0), "{tool:?} {all:?}");

        // Shift+R turns it back, and an unturned one is sent as 0.
        tap_r(&mut session, true);
        drag(&mut session, (4, 4), (5, 4));
        let all: Vec<f64> = asked(&engine).iter().flat_map(|ops| spawned(ops)).collect();
        assert!(!all.is_empty() && all.iter().all(|&angle| angle == 0.0), "{tool:?} {all:?}");
    }
}

#[test]
fn the_turn_is_shared_with_the_place_tool_and_fill() {
    let (mut session, engine) = table_in(Tool::Place);
    tap_r(&mut session, true);
    session.set_tool(Tool::Fill);
    click(&mut session, (2.5, 2.5));
    let all: Vec<f64> = asked(&engine).iter().flat_map(|ops| spawned(ops)).collect();
    assert_eq!(all.len(), 36);
    assert!(all.iter().all(|&angle| angle == 90.0), "{all:?}");
    // R with Fill turns it too, and does not take the rectangle.
    tap_r(&mut session, false);
    assert_eq!(session.tool(), Tool::Fill);
    session.set_tool(Tool::Place);
    click(&mut session, (0.5, 0.5));
    assert_eq!(asked(&engine).iter().flat_map(|ops| spawned(ops)).collect::<Vec<_>>(), [0.0]);
}

#[test]
fn r_held_turns_the_entity_of_a_shape_by_the_wheel() {
    let (mut session, engine) = table_in(Tool::Circle);
    // Off the hold, the wheel zooms.
    assert!(!session.wheel_turns());
    session.key_down('R', false);
    assert!(session.wheel_turns());
    wheel(&mut session, -2.0);
    assert!(session.wheel_turns());
    session.key_up('R');
    // The wheel turned it, so R let go is no quarter turn.
    drag(&mut session, (2, 2), (2, 2));
    assert_eq!(asked(&engine).iter().flat_map(|ops| spawned(ops)).collect::<Vec<_>>(), [350.0]);
    assert!(!session.wheel_turns());
}

#[test]
fn with_a_tile_in_hand_r_is_still_the_rectangle() {
    let (mut session, _engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Line);
    tap_r(&mut session, false);
    assert_eq!(session.tool(), Tool::Rectangle);
}
