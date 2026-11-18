//! Laying a tile under new walls.

use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::forks::settings::Settings;

/// A prototype with these tags and components, anchored by default or not.
pub(super) fn made(id: &str, tags: &[&str], components: &[&str], anchored: bool) -> Prototype {
    Prototype {
        id: id.into(),
        name: id.to_lowercase(),
        placement_mode: "SnapgridCenter".into(),
        tags: tags.iter().map(|&tag| Arc::from(tag)).collect(),
        components: components.iter().map(|&component| Arc::from(component)).collect(),
        anchored,
        ..Default::default()
    }
}

/// The scene's session, knowing a wall, a diagonal wall, a window, a grille, an airlock, a
/// table and a window pane that lies loose, in that order.
pub(super) fn knowing_walls() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.set_prototypes(vec![
        made("WallSolid", &["Wall"], &["Tag"], true),
        made("WallSolidDiagonal", &["Wall", "Diagonal"], &["Tag"], true),
        made("Window", &["Window"], &["Tag"], true),
        made("Grille", &[], &["CanBuildWindowOnTop"], true),
        made("Airlock", &[], &["Door"], true),
        made("Table", &[], &[], true),
        made("SheetGlass", &["Window"], &["Item"], false),
    ]);
    (session, engine)
}

const WALL_SOLID: usize = 0;
const TABLE: usize = 5;

/// What each op of an edit asks for: a tile and its cell, or a spawn and its cell.
fn kinds(ops: &[Value]) -> Vec<(String, (i64, i64))> {
    ops.iter()
        .map(|op| match op["kind"].as_str().unwrap() {
            "tile" => (format!("tile {}", op["tile"].as_str().unwrap()), (op["x"].as_i64().unwrap(), op["y"].as_i64().unwrap())),
            kind => (format!("{kind} {}", op["prototype"].as_str().unwrap()), (op["x"].as_f64().unwrap().floor() as i64, op["y"].as_f64().unwrap().floor() as i64)),
        })
        .collect()
}

fn tile(id: &str, cell: (i64, i64)) -> (String, (i64, i64)) {
    (format!("tile {id}"), cell)
}

fn spawn(id: &str, cell: (i64, i64)) -> (String, (i64, i64)) {
    (format!("spawn {id}"), cell)
}

#[test]
fn walls_windows_grilles_and_doors_need_a_floor() {
    let (session, _engine) = knowing_walls();
    let needs: Vec<(&str, bool)> = session.prototypes().iter().map(|prototype| (prototype.id.as_str(), needs_floor(prototype))).collect();
    assert_eq!(
        needs,
        [("WallSolid", true), ("WallSolidDiagonal", true), ("Window", true), ("Grille", true), ("Airlock", true), ("Table", false), ("SheetGlass", false)]
    );
    // A wall is one whatever its anchoring. A door that lies loose is not.
    assert!(needs_floor(&made("WallLoose", &["Wall"], &[], false)));
    assert!(!needs_floor(&made("DoorKit", &[], &["Door"], false)));
    // The room is built with straight walls only.
    let rooms: Vec<bool> = session.prototypes().iter().map(is_room_wall).collect();
    assert_eq!(rooms, [true, false, false, false, false, false, false]);
}

#[test]
fn a_rectangle_of_walls_lays_plating_where_there_is_no_tile_in_the_same_edit() {
    let (mut session, engine) = knowing_walls();
    session.set_tool(Tool::Rectangle);
    session.choose_entity(WALL_SOLID);
    // From the floor out past the grid's edge at x = 6.
    drag(&mut session, (4, 2), (7, 2));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Rectangle of entities"]);
    assert_eq!(edits[0].1, None);
    // The tiles first, only where there was none, and then the walls on every cell.
    assert_eq!(
        kinds(&edits[0].2),
        [tile("Plating", (6, 2)), tile("Plating", (7, 2)), spawn("WallSolid", (4, 2)), spawn("WallSolid", (5, 2)), spawn("WallSolid", (6, 2)), spawn("WallSolid", (7, 2))]
    );
    assert!(edits[0].2.iter().all(|op| op["kind"] != "tile" || op["grid"] == GRID));
}

#[test]
fn a_stroke_lays_the_tile_in_the_same_step_and_the_place_tool_too() {
    let (mut session, engine) = knowing_walls();
    session.set_tool(Tool::Paint);
    session.choose_entity(WALL_SOLID);
    drag(&mut session, (5, 4), (7, 4));
    let stroke = edits(&engine);
    assert!(one_stroke(&stroke));
    let ops: Vec<Value> = stroke.into_iter().flat_map(|edit| edit.2).collect();
    // The press is on the floor. The move crosses two cells with no tile, as one edit.
    assert_eq!(kinds(&ops), [spawn("WallSolid", (5, 4)), tile("Plating", (6, 4)), tile("Plating", (7, 4)), spawn("WallSolid", (6, 4)), spawn("WallSolid", (7, 4))]);

    // The place tool puts it on the grid, with the tile under it, where no grid has a tile.
    let (mut session, engine) = knowing_walls();
    session.set_tool(Tool::Place);
    session.choose_entity(WALL_SOLID);
    click(&mut session, (8.25, 3.75));
    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!(kinds(&edits[0].2), [tile("Plating", (8, 3)), spawn("WallSolid", (8, 3))]);
    assert_eq!((edits[0].2[0]["grid"].clone(), edits[0].2[1]["parent"].clone()), (json!(GRID), json!(GRID)));
    // On the floor it lays nothing.
    click(&mut session, (2.5, 2.5));
    assert_eq!(kinds(&asked(&engine)[0]), [spawn("WallSolid", (2, 2))]);
}

#[test]
fn other_entities_still_skip_cells_with_no_tile() {
    let (mut session, engine) = knowing_walls();
    session.set_tool(Tool::Rectangle);
    session.choose_entity(TABLE);
    drag(&mut session, (4, 2), (7, 2));
    assert_eq!(kinds(&asked(&engine)[0]), [spawn("Table", (4, 2)), spawn("Table", (5, 2))]);

    // The place tool puts it on the map off the grids, as before.
    session.set_tool(Tool::Place);
    click(&mut session, (8.25, 3.75));
    let ops = asked(&engine);
    assert_eq!(ops[0].len(), 1);
    assert_eq!(ops[0][0]["parent"], json!(1));
}

#[test]
fn nothing_is_laid_with_the_switch_off() {
    let (mut session, engine) = knowing_walls();
    assert!(session.floor_under_walls());
    session.set_floor_under_walls(false);
    session.set_tool(Tool::Line);
    session.choose_entity(WALL_SOLID);
    drag(&mut session, (4, 2), (7, 2));
    assert_eq!(kinds(&asked(&engine)[0]), [spawn("WallSolid", (4, 2)), spawn("WallSolid", (5, 2))]);
    assert_eq!(session.floor_note(), None);
}

#[test]
fn the_tile_picked_is_the_one_laid() {
    let (mut session, engine) = knowing_walls();
    session.set_floor_tile("Lattice");
    session.set_tool(Tool::Circle);
    session.choose_entity(4);
    // A radius of 1 about (6, 0): the cells right of x = 5 and below y = 0 have no tile.
    drag(&mut session, (6, 0), (7, 0));
    assert_eq!(
        kinds(&asked(&engine)[0]),
        [
            tile("Lattice", (6, -1)),
            tile("Lattice", (6, 0)),
            tile("Lattice", (7, 0)),
            tile("Lattice", (6, 1)),
            spawn("Airlock", (6, -1)),
            spawn("Airlock", (5, 0)),
            spawn("Airlock", (6, 0)),
            spawn("Airlock", (7, 0)),
            spawn("Airlock", (6, 1))
        ]
    );
}

#[test]
fn the_ghost_and_the_status_bar_show_where_a_tile_will_be_laid() {
    let (mut session, _engine) = knowing_walls();
    session.set_tool(Tool::Paint);
    session.choose_entity(WALL_SOLID);
    assert_eq!(session.floor_note().as_deref(), Some("Lays Plating where there's no tile"));
    // Over the floor nothing is laid, and off it one tile is.
    session.pointer_move(over(&session, (2, 2)), VIEW);
    assert!(session.overlay_quads().0.is_empty());
    session.pointer_move(over(&session, (7, 2)), VIEW);
    assert_eq!(session.overlay_quads().0.len(), FLOATS_PER_QUAD);
    // A rectangle shows each of its cells with no tile while it is dragged.
    session.set_tool(Tool::Rectangle);
    session.pointer_down(over(&session, (4, 2)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (8, 3)), VIEW);
    assert_eq!(session.overlay_quads().0.len(), 6 * FLOATS_PER_QUAD);
    session.pointer_up(over(&session, (8, 3)), VIEW);

    // A table lays nothing, so there is no ghost and no note.
    session.choose_entity(TABLE);
    session.pointer_move(over(&session, (7, 2)), VIEW);
    assert!(session.overlay_quads().0.is_empty());
    assert_eq!(session.floor_note(), None);
}

#[test]
fn the_switch_the_tile_and_the_room_wall_are_kept_per_fork() {
    let folder = std::env::temp_dir().join(format!("editor-core-floors-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let file = folder.join("settings.json");
    // What else the file holds stays.
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), Some("p".into()), None);
    settings.save(&file).unwrap();

    let (mut session, _engine) = knowing_walls();
    session.flooring = Flooring::load(file.clone(), "C:/forks/starlight");
    assert_eq!((session.floor_under_walls(), session.floor_tile()), (true, "Plating"));
    session.set_floor_under_walls(false);
    session.set_floor_tile("Lattice");
    session.set_prototypes(vec![made("WallSolid", &["Wall"], &[], true), made("WallReinforced", &["Wall"], &[], true)]);
    session.set_room_wall(1);

    let read = Settings::load(&file);
    assert_eq!(read.forks[0].prototypes.as_deref(), Some("p"));
    let kept = read.placing("C:/forks/starlight");
    assert_eq!((kept.floor_under_walls, kept.floor_tile.as_str(), kept.room_wall.as_deref()), (false, "Lattice", Some("WallReinforced")));

    // A session of this fork reads them back, and another fork has the defaults.
    let (mut again, _again_engine) = knowing_walls();
    again.set_prototypes(vec![made("WallSolid", &["Wall"], &[], true), made("WallReinforced", &["Wall"], &[], true)]);
    again.flooring = Flooring::load(file.clone(), "C:\\forks\\starlight\\");
    assert_eq!((again.floor_under_walls(), again.floor_tile(), again.room_wall()), (false, "Lattice", Some(1)));
    let (mut other, _other_engine) = knowing_walls();
    other.flooring = Flooring::load(file, "C:/checkouts/wizden");
    assert_eq!((other.floor_under_walls(), other.floor_tile(), other.room_wall()), (true, "Plating", Some(WALL_SOLID)));
    let _ = std::fs::remove_dir_all(&folder);
}
