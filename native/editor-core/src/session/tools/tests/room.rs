//! The Room tool.

use super::floors::{knowing_walls, made};
use super::*;

/// A session that knows the walls, with FloorWood in hand and the Room tool.
fn rooming() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = knowing_walls();
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Room);
    (session, engine)
}

/// The tiles an edit lays, by tile, and the cells it puts walls on.
fn built(ops: &[Value]) -> (Vec<(String, (i64, i64))>, Vec<(i64, i64)>) {
    let tiles = ops.iter().filter(|op| op["kind"] == "tile").map(|op| (op["tile"].as_str().unwrap().to_owned(), (op["x"].as_i64().unwrap(), op["y"].as_i64().unwrap()))).collect();
    let walls = ops
        .iter()
        .filter(|op| op["kind"] == "spawn")
        .map(|op| {
            assert_eq!((op["prototype"].clone(), op["parent"].clone(), op["rotation"].clone()), (json!("WallSolid"), json!(GRID), json!(0.0)));
            ((op["x"].as_f64().unwrap() - 0.5) as i64, (op["y"].as_f64().unwrap() - 0.5) as i64)
        })
        .collect();
    (tiles, walls)
}

#[test]
fn a_room_is_floor_inside_and_plating_and_walls_on_its_edge_in_one_edit() {
    let (mut session, engine) = rooming();
    // From (1, 1) to (4, 3). The scene's wall stands on (3, 1), on the edge.
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (4, 3)), VIEW);
    // Nothing is asked while it is dragged. Its size is said over it, and its tiles are drawn faint.
    assert!(asked(&engine).is_empty());
    assert_eq!(session.drag_label(), Some(((3.0, 4.0), "4x3".to_owned())));
    assert_eq!(session.overlay_quads().0.len(), 12 * FLOATS_PER_QUAD);
    assert_eq!(outline(&session), (OutlineStyle::Drag, [(1.0, 1.0), (5.0, 1.0), (5.0, 4.0), (1.0, 4.0)]));
    session.pointer_up(over(&session, (4, 3)), VIEW);

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Build room"]);
    assert_eq!(edits[0].1, None);
    let ops = &edits[0].2;
    // The tiles come before the walls.
    let first_wall = ops.iter().position(|op| op["kind"] == "spawn").unwrap();
    assert!(ops[first_wall..].iter().all(|op| op["kind"] == "spawn"));
    let (tiles, walls) = built(ops);
    let edge = [(1, 1), (1, 2), (1, 3), (2, 1), (2, 3), (3, 3), (4, 1), (4, 2), (4, 3)];
    let mut wanted: Vec<(String, (i64, i64))> = vec![("FloorWood".into(), (2, 2)), ("FloorWood".into(), (3, 2))];
    wanted.extend(edge.iter().map(|&cell| ("Plating".to_owned(), cell)));
    assert_eq!(tiles, wanted);
    // The cell that holds the wall already is skipped whole.
    assert_eq!(walls, edge);
}

#[test]
fn a_room_reaches_into_space_and_lays_only_what_differs() {
    let (mut session, engine) = rooming();
    session.set_floor_tile("Lattice");
    // From (4, 4) to (6, 6): the cells right of x = 5 and above y = 5 have no tile.
    drag(&mut session, (4, 4), (6, 6));
    let (tiles, walls) = built(&asked(&engine)[0]);
    let edge = [(4, 4), (4, 5), (4, 6), (5, 4), (5, 6), (6, 4), (6, 5), (6, 6)];
    let mut wanted: Vec<(String, (i64, i64))> = vec![("FloorWood".into(), (5, 5))];
    wanted.extend(edge.iter().map(|&cell| ("Lattice".to_owned(), cell)));
    assert_eq!(tiles, wanted);
    assert_eq!(walls, edge);

    // The same room with that floor already, and plating on its edge, lays no tile again.
    let (mut session, engine) = rooming();
    let doc = session.doc.as_mut().unwrap();
    doc.tile_names.push("Plating".into());
    doc.tile_names.push("FloorWood".into());
    let (plating, wood) = ((doc.tile_names.len() - 2) as u32, (doc.tile_names.len() - 1) as u32);
    let grid = &mut doc.grids[0];
    for cell in [(0, 0), (0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1), (2, 2)] {
        grid.tiles.get_mut(&cell).unwrap().id = plating;
    }
    grid.tiles.get_mut(&(1, 1)).unwrap().id = wood;
    drag(&mut session, (0, 0), (2, 2));
    let (tiles, walls) = built(&asked(&engine)[0]);
    assert!(tiles.is_empty(), "{tiles:?}");
    assert_eq!(walls.len(), 8);
}

#[test]
fn a_room_of_more_than_ten_thousand_cells_is_refused() {
    let (mut session, engine) = rooming();
    // 101 by 100 is 10,100 cells.
    drag(&mut session, (0, 0), (100, 99));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not built: the room has more than 10,000 cells");
    // 100 by 100 is not too large.
    drag(&mut session, (0, 0), (99, 99));
    assert_eq!(asked(&engine).len(), 1);
}

#[test]
fn escape_or_the_secondary_button_drops_a_room_being_dragged() {
    for secondary in [false, true] {
        let (mut session, engine) = rooming();
        session.selection = vec![CHAIR];
        session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
        session.pointer_move(over(&session, (4, 3)), VIEW);
        if secondary {
            session.pointer_down(over(&session, (4, 3)), VIEW, Button::Secondary);
        } else {
            session.escape();
            // Escape went to the room alone, and the selection stays.
            assert_eq!(session.selection, [CHAIR]);
        }
        assert!(matches!(session.drag, Drag::None));
        assert_eq!(session.drag_label(), None);
        assert!(session.overlay_quads().1.is_empty());
        session.pointer_move(over(&session, (5, 4)), VIEW);
        session.pointer_up(over(&session, (5, 4)), VIEW);
        assert!(asked(&engine).is_empty(), "secondary {secondary}");
    }
}

#[test]
fn a_room_is_built_on_a_turned_grid_in_the_grid_s_own_cells() {
    let (mut session, engine) = rooming();
    // The grid's origin is elsewhere on the map, and it is turned a quarter against the clock.
    let grid = &mut session.doc.as_mut().unwrap().grids[0];
    (grid.x, grid.y, grid.rotation) = (10.0, -5.0, std::f32::consts::FRAC_PI_2);
    session.camera.x = 8.0;
    session.camera.y = -3.0;
    let middle = |session: &Session, cell: (i32, i32)| {
        let grid = &session.doc.as_ref().unwrap().grids[0];
        at(session, grid.to_world((cell.0 as f32 + 0.5, cell.1 as f32 + 0.5)))
    };
    session.pointer_down(middle(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(middle(&session, (3, 3)), VIEW);
    assert_eq!(session.drag_label().map(|(_, text)| text), Some("3x3".to_owned()));
    session.pointer_up(middle(&session, (3, 3)), VIEW);

    let (tiles, walls) = built(&asked(&engine)[0]);
    // The scene's wall stands on (3, 1), which is skipped.
    let edge = [(1, 1), (1, 2), (1, 3), (2, 1), (2, 3), (3, 2), (3, 3)];
    let mut wanted: Vec<(String, (i64, i64))> = vec![("FloorWood".into(), (2, 2))];
    wanted.extend(edge.iter().map(|&cell| ("Plating".to_owned(), cell)));
    assert_eq!(tiles, wanted);
    assert_eq!(walls, edge);
}

#[test]
fn a_wall_chosen_with_the_room_tool_is_its_wall_and_the_tile_stays_in_hand() {
    let (mut session, engine) = rooming();
    session.set_prototypes(vec![made("WallSolid", &["Wall"], &[], true), made("WallReinforced", &["Wall"], &[], true), made("Table", &[], &[], true)]);
    assert_eq!(session.room_wall(), Some(0));
    session.choose_entity(1);
    assert_eq!((session.tool(), session.chosen_tile(), session.room_wall()), (Tool::Room, Some("FloorWood"), Some(1)));
    assert_eq!(session.status_line(), "Room walls: wallreinforced");
    drag(&mut session, (0, 0), (1, 0));
    assert!(asked(&engine)[0].iter().filter(|op| op["kind"] == "spawn").all(|op| op["prototype"] == "WallReinforced"));

    // Any other entity takes the place tool in hand, as with any tool that does not keep it.
    session.choose_entity(2);
    assert_eq!(session.tool(), Tool::Place);
    // A wall chosen elsewhere is the room's wall too.
    session.choose_entity(0);
    assert_eq!(session.room_wall(), Some(0));
    assert_eq!(session.room_walls(&["reinf"]), [1]);
}

#[test]
fn a_room_with_no_wall_or_no_tile_builds_nothing() {
    let (mut session, engine) = rooming();
    session.set_prototypes(vec![made("Table", &[], &[], true)]);
    drag(&mut session, (0, 0), (2, 2));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not built: choose a wall for the room in the Room panel");

    let (mut session, engine) = rooming();
    session.choose_decal("Arrows");
    session.set_tool(Tool::Room);
    drag(&mut session, (0, 0), (2, 2));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Choose a tile to paint");
}

#[test]
fn o_chooses_the_room_tool() {
    let (mut session, _engine) = Session::offline(&scene());
    assert!(session.key_down('o', false));
    assert_eq!(session.tool(), Tool::Room);
}
