//! The Room tool.

use std::collections::BTreeSet;
use std::path::Path;

use super::floors::{knowing_walls, made};
use super::*;
use crate::forks::settings::Settings;
use crate::session::tools::room::room_cells;

/// A session that knows the walls, with FloorWood in hand and the Room tool.
fn rooming() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = knowing_walls();
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Room);
    (session, engine)
}

/// The tiles an edit lays, by tile, and the cells it puts WallSolid on.
fn built(ops: &[Value]) -> (Vec<(String, (i64, i64))>, Vec<(i64, i64)>) {
    built_with(ops, "WallSolid")
}

/// The tiles an edit lays, by tile, and the cells it puts walls of this prototype on.
fn built_with(ops: &[Value], wall: &str) -> (Vec<(String, (i64, i64))>, Vec<(i64, i64)>) {
    let tiles = ops.iter().filter(|op| op["kind"] == "tile").map(|op| (op["tile"].as_str().unwrap().to_owned(), (op["x"].as_i64().unwrap(), op["y"].as_i64().unwrap()))).collect();
    let walls = ops
        .iter()
        .filter(|op| op["kind"] == "spawn")
        .map(|op| {
            assert_eq!((op["prototype"].clone(), op["parent"].clone(), op["rotation"].clone()), (json!(wall), json!(GRID), json!(0.0)));
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
    // The cell that holds a wall already is not drawn, since nothing is built there.
    assert_eq!(session.overlay_quads().0.len(), 11 * FLOATS_PER_QUAD);
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

/// The prototypes the rooms below are built from: those of `knowing_walls`, then
/// WallReinforced.
fn room_prototypes() -> Vec<Prototype> {
    vec![
        made("WallSolid", &["Wall"], &["Tag"], true),
        made("WallSolidDiagonal", &["Wall", "Diagonal"], &["Tag"], true),
        made("Window", &["Window"], &["Tag"], true),
        made("Grille", &[], &["CanBuildWindowOnTop"], true),
        made("Airlock", &[], &["Door"], true),
        made("Table", &[], &[], true),
        made("SheetGlass", &["Window"], &["Item"], false),
        made("WallReinforced", &["Wall"], &["Tag"], true),
    ]
}

/// A session on a grid that holds these rooms, each from one corner to the other with
/// FloorSteel inside and WallSolid on Plating on its edge, and space round them. It knows the
/// walls, has FloorWood in hand and the Room tool. The walls' uids count up from 100.
fn rooms(drawn: &[((i32, i32), (i32, i32))]) -> (Session, Receiver<ToEngine>) {
    let mut tiles = Vec::new();
    let mut walls = Vec::new();
    for &(from, to) in drawn {
        let (inside, edge) = room_cells(from, to);
        tiles.extend(inside.iter().map(|&(x, y)| format!("{x},{y},1,0,0")));
        for (x, y) in edge {
            tiles.push(format!("{x},{y},2,0,0"));
            let uid = 100 + walls.len();
            walls.push(format!(
                r#"{{"uid":{uid},"yamlUid":{uid},"prototype":"WallSolid","parent":{GRID},"x":{x}.5,"y":{y}.5,"rotation":0,"anchored":true,"contained":false,"name":null}}"#
            ));
        }
    }
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"test.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel","Plating"],"tiles":[{}]}}],
            "entities":[{}],
            "history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#,
        tiles.join(","),
        walls.join(",")
    );
    let (mut session, engine) = Session::offline(&scene);
    session.set_prototypes(room_prototypes());
    session.choose_tile("FloorWood");
    session.set_tool(Tool::Room);
    (session, engine)
}

/// The cells of the walls an edit takes away.
fn removed(session: &Session, ops: &[Value]) -> Vec<(i64, i64)> {
    let doc = session.doc.as_ref().unwrap();
    ops.iter()
        .filter(|op| op["kind"] == "delete")
        .map(|op| {
            let entity = &doc.entities[&(op["uid"].as_i64().unwrap() as i32)];
            (entity.x.floor() as i64, entity.y.floor() as i64)
        })
        .collect()
}

/// The cells that hold a wall once an edit is done: those that held one, less those taken
/// away, with those built.
fn walls_after(session: &Session, ops: &[Value]) -> BTreeSet<(i64, i64)> {
    let doc = session.doc.as_ref().unwrap();
    let mut walls: BTreeSet<(i64, i64)> = doc.entities.values().filter(|entity| entity.prototype.starts_with("Wall")).map(|entity| (entity.x.floor() as i64, entity.y.floor() as i64)).collect();
    for cell in removed(session, ops) {
        walls.remove(&cell);
    }
    walls.extend(built(ops).1);
    walls
}

/// The cells reached from one by steps across cells with no wall. It gives up, with None,
/// once it is out past (-3, -3) or (15, 15), which is how a room that is not closed shows.
fn reached(walls: &BTreeSet<(i64, i64)>, start: (i64, i64)) -> Option<BTreeSet<(i64, i64)>> {
    let mut seen = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some((x, y)) = pending.pop() {
        if !(-3..=15).contains(&x) || !(-3..=15).contains(&y) {
            return None;
        }
        for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if !walls.contains(&next) && seen.insert(next) {
                pending.push(next);
            }
        }
    }
    Some(seen)
}

/// Every cell of a rectangle from one corner to the other.
fn block(from: (i64, i64), to: (i64, i64)) -> BTreeSet<(i64, i64)> {
    (from.0..=to.0).flat_map(|x| (from.1..=to.1).map(move |y| (x, y))).collect()
}

fn sorted<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    items.sort();
    items
}

#[test]
fn separate_rooms_never_stack_a_wall_or_lay_floor_under_one() {
    // A room of WallReinforced with the scene's WallSolid on (3, 1), on its edge.
    let (mut session, engine) = rooming();
    session.set_prototypes(room_prototypes());
    session.set_room_wall(7);
    assert!(!session.room_intersection());
    drag(&mut session, (1, 1), (4, 3));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Build room"]);
    let (tiles, walls) = built_with(&edits[0].2, "WallReinforced");
    assert!(!walls.contains(&(3, 1)), "{walls:?}");
    assert!(!tiles.iter().any(|(_, cell)| *cell == (3, 1)), "{tiles:?}");
    assert_eq!(walls.len(), 9);
    assert!(removed(&session, &edits[0].2).is_empty());

    // The same wall inside a room keeps its wall and its tile.
    let (mut session, engine) = rooming();
    drag(&mut session, (2, 0), (4, 2));
    let ops = &asked(&engine)[0];
    let (tiles, walls) = built(ops);
    assert!(!tiles.iter().any(|(_, cell)| *cell == (3, 1)), "{tiles:?}");
    assert!(!walls.contains(&(3, 1)));
    assert!(removed(&session, ops).is_empty());

    // A hallway drawn into a room keeps both rooms' walls, and walls its own edge inside the room.
    let (mut session, engine) = rooms(&[((0, 0), (6, 6))]);
    drag(&mut session, (5, 2), (12, 4));
    let ops = &asked(&engine)[0];
    assert!(removed(&session, ops).is_empty());
    let (tiles, walls) = built(ops);
    assert!(!tiles.iter().any(|(_, cell)| [(6, 2), (6, 3), (6, 4)].contains(cell)), "{tiles:?}");
    assert_eq!(sorted(walls.iter().copied().filter(|cell| cell.0 <= 6)), [(5, 2), (5, 3), (5, 4)]);
}

#[test]
fn intersection_builds_a_hallway_into_a_room() {
    // Room A from (0, 0) to (6, 6). A hallway three cells wide from (5, 2) to (12, 4) reaches
    // one cell into it, past its right wall at x = 6.
    let (mut session, engine) = rooms(&[((0, 0), (6, 6))]);
    session.set_room_intersection(true);
    session.pointer_down(over(&session, (5, 2)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (12, 4)), VIEW);
    // The ghost: 6 cells of floor, 13 walled and 3 left open, and a red mark on the wall that goes.
    let (under, over_map) = session.overlay_quads();
    assert_eq!(under.len(), 22 * FLOATS_PER_QUAD);
    let marks = over_map.chunks_exact(FLOATS_PER_QUAD).filter(|quad| quad[8..12] == [1.0, 0.25, 0.2, 0.45]).count();
    assert_eq!(marks, 1);
    session.pointer_up(over(&session, (12, 4)), VIEW);

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Build room"]);
    assert_eq!(edits[0].1, None);
    let ops = &edits[0].2;
    // Room A's wall on the hallway's inside is taken away, first.
    assert_eq!(removed(&session, ops), [(6, 3)]);
    assert_eq!(ops[0]["kind"], "delete");
    let (tiles, walls) = built(ops);
    // The hallway's floor goes on its inside, the opened cell too.
    let floor: Vec<(i64, i64)> = tiles.iter().filter(|(tile, _)| tile == "FloorWood").map(|(_, cell)| *cell).collect();
    assert_eq!(floor, [(6, 3), (7, 3), (8, 3), (9, 3), (10, 3), (11, 3)]);
    // Its edge inside room A, (5, 2) to (5, 4), is left open with its floor, and room A's
    // walls on (6, 2) and (6, 4) stay as they are.
    let wanted: Vec<(i64, i64)> = sorted(block((7, 2), (12, 4)).into_iter().filter(|&(x, y)| y != 3 || x == 12));
    assert_eq!(sorted(walls.clone()), wanted);
    assert!(!tiles.iter().any(|(_, cell)| cell.0 <= 5), "{tiles:?}");

    // The two make one closed room: room A's inside, the opened cell and the hallway's inside.
    let mut inside = block((1, 1), (5, 5));
    inside.extend(block((6, 3), (11, 3)));
    assert_eq!(reached(&walls_after(&session, ops), (2, 2)), Some(inside));
}

#[test]
fn intersection_joins_two_rooms_that_overlap() {
    // Room A from (0, 0) to (6, 6), and room B dragged from (4, 4) to (10, 10).
    let (mut session, engine) = rooms(&[((0, 0), (6, 6))]);
    session.set_room_intersection(true);
    drag(&mut session, (4, 4), (10, 10));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Build room"]);
    let ops = &edits[0].2;
    // Room A's walls inside B go.
    assert_eq!(sorted(removed(&session, ops)), [(5, 6), (6, 5), (6, 6)]);
    let (tiles, walls) = built(ops);
    // B's edge inside A is left open, and is not laid. A's walls on B's edge stay.
    for open in [(4, 4), (4, 5), (5, 4)] {
        assert!(!walls.contains(&open) && !tiles.iter().any(|(_, cell)| *cell == open), "{open:?}");
    }
    assert!(!walls.contains(&(4, 6)) && !walls.contains(&(6, 4)));
    assert_eq!(walls.len(), 24 - 5);
    // One closed room: both insides.
    let mut inside = block((1, 1), (5, 5));
    inside.extend(block((5, 5), (9, 9)));
    assert_eq!(reached(&walls_after(&session, ops), (2, 2)), Some(inside));
}

#[test]
fn intersection_inside_a_room_builds_no_walls_and_opens_nothing() {
    // A room from (2, 2) to (5, 5) drawn wholly inside room A, which has a wall standing on
    // its own at (7, 7), outside the new room.
    let (mut session, engine) = rooms(&[((0, 0), (8, 8)), ((7, 7), (7, 7))]);
    session.set_room_intersection(true);
    drag(&mut session, (2, 2), (5, 5));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Build room"]);
    let ops = &edits[0].2;
    // It joins the room around it: no wall is built or taken away, and only its inside takes
    // the floor in hand. Its edge keeps room A's floor.
    assert!(removed(&session, ops).is_empty());
    let (tiles, walls) = built(ops);
    assert!(walls.is_empty());
    assert_eq!(tiles, [("FloorWood".to_owned(), (3, 3)), ("FloorWood".to_owned(), (3, 4)), ("FloorWood".to_owned(), (4, 3)), ("FloorWood".to_owned(), (4, 4))]);
    assert_eq!(session.status_line(), "Joined the room around it. No walls built.");

    // A wall standing inside the new room is taken away, since the room is one now.
    let (mut session, engine) = rooms(&[((0, 0), (8, 8)), ((3, 3), (3, 3))]);
    session.set_room_intersection(true);
    drag(&mut session, (2, 2), (5, 5));
    let ops = &asked(&engine)[0];
    assert_eq!(removed(&session, ops), [(3, 3)]);
    assert!(built(ops).1.is_empty());

    // With Intersection off the same room is walled inside room A, as a room of its own.
    let (mut session, engine) = rooms(&[((0, 0), (8, 8))]);
    drag(&mut session, (2, 2), (5, 5));
    assert_eq!(built(&asked(&engine)[0]).1.len(), 12);
}

#[test]
fn the_intersection_switch_is_kept_per_fork() {
    let folder = std::env::temp_dir().join(format!("editor-core-room-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let file = folder.join("settings.json");
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), Some("p".into()), None);
    settings.save(&file).unwrap();

    let (mut session, _engine) = knowing_walls();
    session.flooring = Flooring::load(file.clone(), "C:/forks/starlight");
    assert!(!session.room_intersection());
    session.set_floor_tile("Lattice");
    session.set_room_intersection(true);
    let read = Settings::load(&file);
    assert_eq!(read.forks[0].prototypes.as_deref(), Some("p"));
    let kept = read.placing("C:/forks/starlight");
    assert_eq!((kept.room_intersection, kept.floor_tile.as_str()), (true, "Lattice"));

    let (mut again, _again_engine) = knowing_walls();
    again.flooring = Flooring::load(file.clone(), "C:\\forks\\starlight\\");
    assert!(again.room_intersection());
    let (mut other, _other_engine) = knowing_walls();
    other.flooring = Flooring::load(file, "C:/checkouts/wizden");
    assert!(!other.room_intersection());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_wall_list_starts_with_station_walls_and_ends_with_rock() {
    let (mut session, _engine) = rooming();
    session.set_prototypes(vec![
        made("AsteroidRock", &["Wall"], &[], true),
        made("WallRockBasalt", &[], &["Wall"], true),
        made("WallWood", &["Wall"], &[], true),
        made("WallReinforced", &["Wall"], &[], true),
        made("WallSolid", &["Wall"], &[], true),
        made("WallShuttle", &["Wall"], &[], true),
        made("WallClown", &["Wall"], &[], true),
        made("Table", &[], &[], true),
        made("WoodenSupportWall", &["Wall"], &[], true),
    ]);
    let ids = |session: &Session, words: &[&str]| -> Vec<String> { session.room_walls(words).into_iter().map(|index| session.prototypes()[index].id.clone()).collect() };
    // Before the fork's files are read, every other wall is by name.
    assert_eq!(ids(&session, &[]), ["WallSolid", "WallReinforced", "WallClown", "WallShuttle", "WallWood", "AsteroidRock", "WallRockBasalt", "WoodenSupportWall"]);
    // The palette's Structures/Walls come next after the first two.
    let categories = [("WallWood", "Structures/Walls"), ("WallShuttle", "Structures/Walls/Shuttle"), ("WallClown", "Structures/Furniture"), ("AsteroidRock", "Structures/Walls")];
    session.categories_arrived(categories.iter().map(|(id, category)| (id.to_string(), category.to_string())).collect());
    assert_eq!(ids(&session, &[]), ["WallSolid", "WallReinforced", "WallShuttle", "WallWood", "WallClown", "AsteroidRock", "WallRockBasalt", "WoodenSupportWall"]);
    // A search still finds the rock.
    assert_eq!(ids(&session, &["rock"]), ["AsteroidRock", "WallRockBasalt"]);
}
