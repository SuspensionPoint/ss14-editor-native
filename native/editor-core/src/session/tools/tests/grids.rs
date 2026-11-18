//! The grid list: centring on a grid, and adding, renaming and removing grids.

use super::entity::{answer, delta, entity, one_edit, opened, refuse};
use super::*;

/// A grid as the engine describes one, with one tile of steel on cell (0, 0).
fn grid_json(uid: i32, name: &str, x: f32, y: f32) -> String {
    format!(r#"{{"uid":{uid},"name":"{name}","x":{x},"y":{y},"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0],"decals":[]}}"#)
}

/// What the grid list shows: each grid's uid, name and tiles, and which is the tile tools'.
fn rows(session: &Session) -> Vec<(i32, String, usize, bool)> {
    session.grid_rows().into_iter().map(|row| (row.uid, row.name, row.tiles, row.active)).collect()
}

#[test]
fn a_click_on_a_grid_centres_the_view_on_its_tiles() {
    let (mut session, answers, engine) = opened(&scene());
    assert_eq!(rows(&session), [(GRID, "grid".to_owned(), 36, true)]);
    // A second grid far off, with one tile.
    answer(&mut session, &answers, 1, "Add grid", delta(&[], &[], &[grid_json(30, "Shuttle", 100.0, 50.0)], &[30], &[]));

    session.centre_on_grid(30);
    assert_eq!((session.camera.x, session.camera.y), (100.5, 50.5));
    assert_eq!((session.grid.0, session.doc.as_ref().unwrap().active_grid), (30, 30));
    session.centre_on_grid(GRID);
    // The middle of the six by six floor.
    assert_eq!((session.camera.x, session.camera.y, session.grid.0), (3.0, 3.0, GRID));
    assert_eq!(rows(&session)[0].3, true);
    // Centring asks nothing of the engine.
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_grid_is_added_in_the_middle_of_the_view_and_becomes_the_tile_tools_grid() {
    let (mut session, answers, engine) = opened(&scene());
    session.camera.x = 12.25;
    session.camera.y = -3.5;
    session.add_grid("  Shuttle ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Add grid", json!({ "kind": "grid.add", "x": 12.25, "y": -3.5, "name": "Shuttle" })));

    answer(&mut session, &answers, seq, "Add grid", delta(&[], &[], &[grid_json(30, "Shuttle", 12.25, -3.5)], &[30], &[]));
    assert_eq!(rows(&session), [(GRID, "grid".to_owned(), 36, false), (30, "Shuttle".to_owned(), 1, true)]);
    assert_eq!(session.grid, (30, 12.25, -3.5));

    // With no name the engine names it.
    session.add_grid("");
    let (_, _, op) = one_edit(&engine);
    assert!(op.get("name").is_none());
}

#[test]
fn a_refused_grid_leaves_the_list_as_it_was() {
    let (mut session, answers, engine) = opened(&scene());
    session.add_grid("Shuttle");
    let (_, seq, _) = one_edit(&engine);
    refuse(&mut session, &answers, seq, "Add grid", "Something went wrong.");
    assert_eq!(rows(&session), [(GRID, "grid".to_owned(), 36, true)]);
    assert_eq!((session.grid.0, session.grid_adding), (GRID, None));
    assert_eq!(session.status_line(), "Add grid: the engine refused: Something went wrong.");
}

#[test]
fn a_grid_is_renamed_with_the_rename_op() {
    let (mut session, answers, engine) = opened(&scene());
    session.rename_grid(GRID, " Station ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Rename grid", json!({ "kind": "rename", "uid": GRID, "name": "Station" })));
    // The engine sends the grid back whole, with its new name.
    let renamed = format!(r#"{{"uid":{GRID},"name":"Station","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0],"decals":[]}}"#);
    answer(&mut session, &answers, seq, "Rename grid", delta(&[], &[], &[renamed], &[], &[]));
    assert_eq!(rows(&session)[0].1, "Station");

    // No name, or the name it has, sends nothing.
    session.rename_grid(GRID, "  ");
    assert_eq!(session.status_line(), "Grid not renamed: give it a name");
    session.rename_grid(GRID, "Station");
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_grid_is_removed_with_all_on_it_and_an_undo_brings_it_back() {
    let (mut session, answers, engine) = opened(&scene());
    session.selection = vec![CHAIR];
    // The crowbar inside the wall counts as standing on the grid.
    assert_eq!(
        session.grid_removal_question(GRID).as_deref(),
        Some("Remove the grid grid (2) with its 36 tiles and 0 decals? The 3 entities on it go too. Undo brings it all back.")
    );
    session.remove_grid(GRID);
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Remove grid", json!({ "kind": "grid.remove", "grid": GRID })));

    // What each holds first, the grid last.
    answer(&mut session, &answers, seq, "Remove grid", delta(&[], &[CROWBAR, WALL, CHAIR, GRID], &[], &[], &[]));
    let doc = session.doc.as_ref().unwrap();
    assert!(doc.grids.is_empty() && doc.entities.is_empty() && session.selection.is_empty());
    assert!(rows(&session).is_empty());

    // The undo gives the grid back whole, and all that stood on it.
    let back = [entity(CHAIR, "Chair", GRID, false, None), entity(WALL, "WallSolid", GRID, false, None), entity(CROWBAR, "Crowbar", WALL, true, None)];
    answer(&mut session, &answers, seq + 1, "Undo", delta(&back, &[], &[grid_json(GRID, "grid", 0.0, 0.0)], &[], &[]));
    let doc = session.doc.as_ref().unwrap();
    assert_eq!((doc.entities.len(), doc.entities_on(GRID), doc.contents(WALL).len()), (3, 3, 1));
    assert_eq!((doc.active_grid, session.grid.0), (GRID, GRID));
}

#[test]
fn in_a_grid_file_grids_are_neither_added_nor_removed() {
    let (mut session, _answers, engine) = opened(&scene().replace(r#""category":"Map""#, r#""category":"Grid""#));
    assert_eq!(session.grid_edits_refused(), Some(GRID_FILE));
    session.add_grid("Shuttle");
    assert_eq!(session.status_line(), GRID_FILE);
    session.remove_grid(GRID);
    assert!(asked(&engine).is_empty());
    assert_eq!(rows(&session).len(), 1);

    // Renaming its one grid is allowed.
    session.rename_grid(GRID, "Shuttle");
    assert_eq!(one_edit(&engine).0, "Rename grid");

    let (map, _answers, _engine) = opened(&scene());
    assert_eq!(map.grid_edits_refused(), None);
}

#[test]
fn a_map_that_is_a_grid_itself_keeps_that_grid() {
    // A planet's map is a grid too, with its ground as its tiles. The engine sends it as a grid.
    let (mut session, _answers, engine) = opened(&scene().replace(r#""mapUid":1"#, &format!(r#""mapUid":{GRID}"#)));
    assert_eq!(rows(&session), [(GRID, "grid".to_owned(), 36, true)]);
    assert!(session.grid_rows()[0].ground);
    assert_eq!(session.doc.as_ref().unwrap().entities_on(GRID), 3);

    session.remove_grid(GRID);
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), MAP_GROUND);

    // The grid of an ordinary map is no ground.
    let (map, _answers, _engine) = opened(&scene());
    assert!(!map.grid_rows()[0].ground);
}
