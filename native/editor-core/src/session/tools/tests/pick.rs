//! The pick tool.

use super::*;

/// Where `picking` heaps three things up: the middle of cell (1, 1).
const HEAP: (f32, f32) = (1.5, 1.5);

/// A session with the pick tool in hand and wood chosen, which is on no cell, whose engine
/// knows the three prototypes of the scene. The chair, the wall and the crowbar are drawn on
/// one place, in that order, so that the crowbar is on top and the chair at the bottom.
fn picking() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    let known = |id: &str| Prototype { id: id.into(), name: id.to_lowercase(), placement_mode: "SnapgridCenter".into(), ..Default::default() };
    session.set_prototypes(vec![known("Chair"), known("Crowbar"), known("WallSolid")]);
    session.choose_tile("FloorWood");
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 0), (CROWBAR, HEAP, 0)]);
    session.set_tool(Tool::Pick);
    (session, engine)
}

/// Turns the wheel over a map position, and lets any zoom it owes run out. A notch towards the
/// user is -1.
fn wheel_over(session: &mut Session, world: (f32, f32), notches: f32) {
    session.wheel(notches, at(session, world), VIEW);
    glide_out(session);
}

/// The list the interface would draw, and the row it is on.
fn listed(session: &Session) -> Option<(Vec<Pickable>, usize)> {
    session.pick_list().map(|list| (list.entries, list.current))
}

/// What the heap holds, top first, with the floor under it last.
fn heap() -> Vec<Pickable> {
    let entity = |id: &str| Pickable::Entity(id.into());
    vec![entity("Crowbar"), entity("WallSolid"), entity("Chair"), Pickable::Tile("FloorSteel".into())]
}

/// The id of the entity chosen, if one is.
fn chosen(session: &Session) -> Option<String> {
    session.chosen_prototype().map(|prototype| prototype.id.clone())
}

#[test]
fn pick_takes_the_top_entity_and_goes_to_place() {
    let (mut session, engine) = picking();
    click(&mut session, HEAP);
    assert_eq!((chosen(&session), session.tool()), (Some("Crowbar".to_owned()), Tool::Place));
    assert_eq!(session.status_line(), "Picked Crowbar");
    assert!(asked(&engine).is_empty());

    // A prototype with no place in the palette is picked as well, and placed.
    let (mut session, _engine) = picking();
    let mut hidden = session.prototypes().to_vec();
    hidden[1].is_abstract = true;
    hidden[1].hide_spawn_menu = true;
    session.set_prototypes(hidden);
    assert!(!session.prototypes()[1].placeable());
    click(&mut session, HEAP);
    assert_eq!((chosen(&session), session.tool()), (Some("Crowbar".to_owned()), Tool::Place));
}

#[test]
fn pick_takes_the_tile_where_nothing_stands() {
    let (mut session, _engine) = picking();
    click(&mut session, (0.5, 4.5));
    assert_eq!((session.chosen_tile(), session.tool()), (Some("FloorSteel"), Tool::Paint));
    assert_eq!(session.status_line(), "Picked FloorSteel");

    // Off the grid there is nothing to pick, and nothing changes.
    let (mut session, engine) = picking();
    click(&mut session, (8.5, 8.5));
    assert_eq!((session.chosen_tile(), chosen(&session), session.tool()), (Some("FloorWood"), None, Tool::Pick));
    assert!(asked(&engine).is_empty());
}

#[test]
fn pick_leaves_out_a_hidden_layer() {
    let (mut session, _engine) = picking();
    // The chair, and over it in the same place the wall, drawn at a depth the game gives doors.
    draw(&mut session, &[(CHAIR, HEAP, 0), (WALL, HEAP, 9)]);
    session.layers.doors = false;
    click(&mut session, HEAP);
    assert_eq!(chosen(&session), Some("Chair".to_owned()));

    // With the chair's layer hidden as well the tile is what is there.
    session.set_tool(Tool::Pick);
    session.layers.objects = false;
    click(&mut session, HEAP);
    assert_eq!((session.chosen_tile(), session.tool()), (Some("FloorSteel"), Tool::Paint));

    // The wheel's list leaves them out too, and with the tile alone it is no list.
    session.set_tool(Tool::Pick);
    let zoom = session.camera.zoom;
    wheel_over(&mut session, HEAP, -1.0);
    assert!(session.camera.zoom < zoom);
    assert_eq!(listed(&session), None);
}

#[test]
fn the_wheel_steps_through_what_is_there_and_the_tile_comes_last() {
    let (mut session, engine) = picking();
    let view = (session.camera.x, session.camera.y, session.camera.zoom);

    // A notch towards the user goes one down the list, and one away from the user one up.
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(listed(&session), Some((heap(), 1)));
    wheel_over(&mut session, HEAP, -1.0);
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(listed(&session), Some((heap(), 3)));
    // Round both ends.
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(listed(&session), Some((heap(), 0)));
    wheel_over(&mut session, HEAP, 1.0);
    assert_eq!(listed(&session), Some((heap(), 3)));
    // A notch that comes in parts is one step, taken when more than half of it is here.
    wheel_over(&mut session, HEAP, 0.3);
    assert_eq!(listed(&session), Some((heap(), 3)));
    wheel_over(&mut session, HEAP, 0.3);
    assert_eq!(listed(&session), Some((heap(), 2)));
    // The list is drawn beside the cell the cursor is over.
    assert_eq!(session.pick_list().unwrap().cell, [(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]);

    // None of it zoomed, chose anything or asked anything of the engine.
    assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), view);
    assert_eq!((session.chosen_tile(), chosen(&session), session.tool()), (Some("FloorWood"), None, Tool::Pick));
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_wheel_zooms_where_there_is_one_thing_or_none() {
    // The bare floor is one thing, and off the grid is nothing.
    for over in [(0.5, 4.5), (8.5, 8.5)] {
        let (mut session, _engine) = picking();
        let zoom = session.camera.zoom;
        wheel_over(&mut session, over, 1.0);
        assert!(session.camera.zoom > zoom, "{over:?}");
        assert_eq!(listed(&session), None, "{over:?}");
    }

    // One thing on the floor is two, and a list.
    let (mut session, _engine) = picking();
    draw(&mut session, &[(CHAIR, HEAP, 0)]);
    let zoom = session.camera.zoom;
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(session.camera.zoom, zoom);
    assert_eq!(listed(&session), Some((vec![Pickable::Entity("Chair".into()), Pickable::Tile("FloorSteel".into())], 1)));
}

#[test]
fn a_press_picks_the_row_the_list_is_on() {
    let (mut session, engine) = picking();
    wheel_over(&mut session, HEAP, -2.0);
    click(&mut session, HEAP);
    assert_eq!((chosen(&session), session.tool(), listed(&session)), (Some("Chair".to_owned()), Tool::Place, None));
    assert_eq!(session.status_line(), "Picked Chair");

    // The tile's row picks the tile.
    session.set_tool(Tool::Pick);
    wheel_over(&mut session, HEAP, 1.0);
    click(&mut session, HEAP);
    assert_eq!((session.chosen_tile(), session.tool(), listed(&session)), (Some("FloorSteel"), Tool::Paint, None));
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_right_click_closes_the_list_and_picks_nothing() {
    let (mut session, _engine) = picking();
    wheel_over(&mut session, HEAP, -1.0);
    session.pointer_down(at(&session, HEAP), VIEW, Button::Secondary);
    session.pointer_up(at(&session, HEAP), VIEW);
    assert_eq!((listed(&session), session.chosen_tile(), chosen(&session), session.tool()), (None, Some("FloorWood"), None, Tool::Pick));

    // A press after it takes the top, as with no list.
    click(&mut session, HEAP);
    assert_eq!(chosen(&session), Some("Crowbar".to_owned()));
}

#[test]
fn the_list_closes_when_the_cursor_leaves_its_cell() {
    let (mut session, _engine) = picking();
    wheel_over(&mut session, HEAP, -1.0);
    // The cursor moved within the cell leaves the list open.
    session.pointer_move(at(&session, (1.9, 1.1)), VIEW);
    assert_eq!(listed(&session), Some((heap(), 1)));
    // In the next cell it is closed, and nothing is picked.
    session.pointer_move(at(&session, (2.1, 1.1)), VIEW);
    assert_eq!((listed(&session), session.chosen_tile(), session.tool()), (None, Some("FloorWood"), Tool::Pick));

    // The list opens again on the first, and the cursor leaving the map closes it.
    session.pointer_move(at(&session, HEAP), VIEW);
    wheel_over(&mut session, HEAP, -1.0);
    assert_eq!(listed(&session), Some((heap(), 1)));
    session.pointer_left();
    assert_eq!(listed(&session), None);

    // The tool taken in hand, even the same one, closes it.
    wheel_over(&mut session, HEAP, -1.0);
    session.set_tool(Tool::Pick);
    assert_eq!(listed(&session), None);
}
