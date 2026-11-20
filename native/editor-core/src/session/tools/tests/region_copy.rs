//! Copy, cut and paste of the area tool's region, its right-click menu, and the Edit menu on a region.

use super::super::select::quarter_on;
use super::paste::{answered, hover, one_copy, open};
use super::*;
use crate::model::Decal;

/// The text the engine answers a copy of the region's entities with.
pub(super) const TEXT: &str = "entities: [chair, wall, crowbar]";

/// A decal of the game's, at the corner of a cell, unturned and untinted.
fn decal(id: i32, x: f32, y: f32) -> Decal {
    Decal { id, prototype: "Arrows".into(), x, y, color: None, angle: 0.0, z_index: 0, cleanable: false, order: id.into() }
}

/// The region from (1, 1) to (3, 2) with the area tool, as `answered` gives the session. The
/// chair and the wall stand on it. Its cell (2, 2) has no tile, its cell (1, 1) has a tile of
/// variant 2 turned once, and a decal is on its cell (1, 2) and another off it.
pub(super) fn region_held() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = answered();
    let grid = &mut session.doc.as_mut().unwrap().grids[0];
    grid.tiles.remove(&(2, 2));
    let tile = grid.tiles.get_mut(&(1, 1)).unwrap();
    (tile.variant, tile.rotation_mirroring) = (2, 1);
    grid.decals.insert(7, decal(7, 1.25, 2.5));
    grid.decals.insert(8, decal(8, 4.5, 4.5));
    session.decal_prototypes = vec![DecalPrototype { id: "Arrows".into(), ..Default::default() }];
    session.set_tool(Tool::Area);
    drag(&mut session, (1, 1), (3, 2));
    (session, answers, engine)
}

/// The engine answers the copy of the region's entities. The cell it names is not the region's.
pub(super) fn answer(session: &mut Session, answers: &Sender<FromEngine>, seq: u64) {
    answers.send(FromEngine::Copied { seq, text: TEXT.into(), roots: 2, entities: 3, from: (1, 1) }).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// A session as `region_held` gives it whose region has been copied, with the copy answered.
fn region_copied() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = region_held();
    session.copy();
    let (seq, _) = one_copy(&engine);
    answer(&mut session, &answers, seq);
    (session, answers, engine)
}

/// A tile op as a stamp sends it.
fn tile(x: i32, y: i32, variant: u8, byte: u8) -> Value {
    json!({ "kind": "tile", "grid": GRID, "x": x, "y": y, "tile": "FloorSteel", "variant": variant, "rotationMirroring": byte })
}

/// A right-click on the middle of a cell.
pub(super) fn right_click(session: &mut Session, cell: (i32, i32)) {
    session.pointer_down(over(session, cell), VIEW, Button::Secondary);
    session.pointer_up(over(session, cell), VIEW);
}

/// What each entry of the region's menu is, and whether it can be chosen.
fn entries(session: &Session) -> Vec<(MenuAction, bool)> {
    session.region_menu().map(|(_, entries)| entries.iter().map(|entry| (entry.action, entry.enabled)).collect()).unwrap_or_default()
}

#[test]
fn a_region_copy_holds_its_tiles_and_decals_beside_the_entities_on_it() {
    let (mut session, answers, engine) = region_held();
    session.copy();
    // The engine is asked for the entities that stand on the region alone.
    let (seq, uids) = one_copy(&engine);
    assert_eq!(uids, [CHAIR, WALL]);
    assert!(session.clipboard().is_none());

    answer(&mut session, &answers, seq);
    let clipboard = session.clipboard().expect("a clipboard");
    assert!(clipboard.is_region());
    // Measured from the region's lowest cell each way, and not from the cell the engine named.
    assert_eq!((clipboard.text(), clipboard.entities(), clipboard.from()), (TEXT, 3, (1, 1)));
    assert_eq!(clipboard.region_cells(), [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)]);
    // The cell with no tile has none, and the tiles keep their variant and turn.
    let tiles: Vec<((i32, i32), u8, u8)> = clipboard.region_tiles().iter().map(|tile| (tile.cell, tile.variant, tile.rotation_mirroring)).collect();
    assert_eq!(tiles, [((0, 0), 2, 1), ((0, 1), 0, 0), ((1, 0), 0, 0), ((2, 0), 0, 0), ((2, 1), 0, 0)]);
    assert!(clipboard.region_tiles().iter().all(|tile| tile.tile == "FloorSteel"));
    // The decal on the region, measured from the origin cell's corner. The one off it is left.
    let decals: Vec<(f32, f32)> = clipboard.region_decals().iter().map(|decal| (decal.x, decal.y)).collect();
    assert_eq!(decals, [(0.25, 1.5)]);
    assert_eq!(session.status_line(), "Copied 5 tiles and 3 entities");
    // A copy changes nothing, and the region stays.
    assert!(asked(&engine).is_empty());
    assert_eq!(session.region_size(), 6);
}

#[test]
fn a_region_with_nothing_on_it_is_copied_at_once() {
    let (mut session, _answers, engine) = answered();
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 3), (1, 4));
    session.copy();
    // Nothing for the engine to copy, and nothing asked.
    assert_eq!(engine.try_iter().count(), 0);
    let clipboard = session.clipboard().expect("a clipboard");
    assert_eq!((clipboard.text(), clipboard.entities(), clipboard.region_tiles().len(), clipboard.from()), ("", 0, 4, (0, 3)));
    assert_eq!(session.status_line(), "Copied 4 tiles");
}

#[test]
fn a_cut_of_a_region_deletes_it_when_the_copy_has_come() {
    let (mut session, answers, engine) = region_held();
    session.cut();
    // The region goes at once. The copy alone is asked for first.
    assert!(session.region.is_none());
    let (seq, _) = one_copy(&engine);

    answer(&mut session, &answers, seq);
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Cut region"]);
    // What stands on it, its decal, its tiles and last their air, as a delete of the region has them.
    let kinds: Vec<&str> = edits[0].2.iter().map(|op| op["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["delete", "delete", "decal.remove", "tile", "tile", "tile", "tile", "tile", "air", "air", "air", "air", "air"]);
    assert_eq!((edits[0].2[0]["uid"].as_i64(), edits[0].2[1]["uid"].as_i64()), (Some(CHAIR.into()), Some(WALL.into())));
    assert!(session.clipboard().is_some_and(Clipboard::is_region));
}

#[test]
fn a_stamp_is_one_edit_of_the_tiles_then_the_entities_then_the_decals() {
    let (mut session, _answers, engine) = region_copied();
    session.set_tool(Tool::Paint);
    session.begin_paste();
    // The area tool in hand, and its region dropped.
    assert_eq!((session.tool(), session.is_pasting(), session.region_size()), (Tool::Area, true, 0));

    click(&mut session, (2.25, 3.75));
    let paste = json!({ "kind": "paste", "text": TEXT, "grid": GRID, "fromX": 1, "fromY": 1, "x": 2.0, "y": 3.0, "quarters": 0 });
    let decal = json!({ "kind": "decal", "grid": GRID, "x": 2.25, "y": 4.5, "prototype": "Arrows", "color": null, "rotation": 0.0, "zIndex": 0, "cleanable": false });
    let ops = vec![tile(2, 3, 2, 1), tile(2, 4, 0, 0), tile(3, 3, 0, 0), tile(4, 3, 0, 0), tile(4, 4, 0, 0), paste, decal];
    // One edit with no stroke, which is one step to undo.
    assert_eq!(edits(&engine), [("Paste region", None, ops)]);
}

#[test]
fn stamps_repeat_until_a_right_click_or_escape() {
    let (mut session, _answers, engine) = region_copied();
    session.begin_paste();
    click(&mut session, (0.5, 3.5));
    click(&mut session, (3.5, 3.5));
    let stamps = edits(&engine);
    assert_eq!(labels(&stamps), ["Paste region", "Paste region"]);
    assert_eq!(cells(&stamps[1].2[..1]), [(3, 3)]);
    assert!(session.is_pasting());

    // A right-click ends it and opens no menu.
    right_click(&mut session, (1, 1));
    assert_eq!((session.is_pasting(), session.region_menu().is_none()), (false, true));
    // A click then is the area tool's own, which asks nothing.
    click(&mut session, (3.5, 3.5));
    assert!(asked(&engine).is_empty());

    // So does Escape.
    session.begin_paste();
    session.escape();
    assert!(!session.is_pasting());
}

#[test]
fn r_turns_the_ghost_s_cells_and_the_stamp() {
    let (mut session, _answers, engine) = region_copied();
    session.tiles = vec![Tile { id: "FloorSteel".into(), sprite: None, allows_rotation: true, can_crowbar: false }];
    session.begin_paste();
    hover(&mut session, (2.5, 3.5));
    assert_eq!(session.region_paste_cells(), [(2, 3), (2, 4), (3, 3), (3, 4), (4, 3), (4, 4)]);
    // The ghost fills each cell, and has the chair and the wall as they were drawn. The tiles
    // have no pictures here.
    assert_eq!(session.overlay_quads().1.len(), (6 + 2) * FLOATS_PER_QUAD);

    // R turns with the clock about the middle of the origin cell, which stays on the cursor.
    tap_r(&mut session, false);
    assert_eq!(session.region_paste_cells(), [(2, 1), (2, 2), (2, 3), (3, 1), (3, 2), (3, 3)]);
    click(&mut session, (2.5, 3.5));
    let ops = asked(&engine).concat();
    let tiles: Vec<&Value> = ops.iter().filter(|op| op["kind"] == "tile").collect();
    assert_eq!(cells(tiles.iter().copied()), [(2, 3), (3, 3), (2, 2), (2, 1), (3, 1)]);
    // The engine counts a turn with the clock as three against it, and the tiles' byte turns so.
    assert_eq!(tiles.iter().map(|op| op["rotationMirroring"].as_i64().unwrap()).collect::<Vec<_>>(), [0, 3, 3, 3, 3]);
    let paste = ops.iter().find(|op| op["kind"] == "paste").unwrap();
    assert_eq!((paste["x"].as_f64(), paste["y"].as_f64(), paste["quarters"].as_i64()), (Some(2.0), Some(3.0), Some(3)));
    // The decal turns as one piece with the region.
    let decal = ops.iter().find(|op| op["kind"] == "decal").unwrap();
    let mut angle = 0.0;
    for _ in 0..3 {
        angle = quarter_on(angle, false);
    }
    assert_eq!((decal["x"].as_f64(), decal["y"].as_f64(), decal["rotation"].as_f64()), (Some(3.5), Some(2.75), Some(angle)));
}

#[test]
fn a_decal_of_a_kind_the_game_lacks_is_left_out_of_a_stamp() {
    let (mut session, _answers, engine) = region_copied();
    session.decal_prototypes.clear();
    session.begin_paste();
    click(&mut session, (2.5, 3.5));
    let ops = asked(&engine).concat();
    assert!(!ops.is_empty() && ops.iter().all(|op| op["kind"] != "decal"));
}

#[test]
fn one_clipboard_holds_either_kind_and_outlives_the_map() {
    let (mut session, answers, engine) = region_copied();

    // A copy of entities takes the region's place.
    session.set_tool(Tool::Select);
    session.selection = vec![CHAIR];
    session.copy();
    let (seq, uids) = one_copy(&engine);
    assert_eq!(uids, [CHAIR]);
    answer(&mut session, &answers, seq);
    assert!(session.clipboard().is_some_and(|clipboard| !clipboard.is_region() && clipboard.region_tiles().is_empty()));
    session.begin_paste();
    assert_eq!(session.tool(), Tool::Select);

    // And a copy of a region takes theirs.
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 4), (0, 4));
    session.copy();
    assert!(session.clipboard().is_some_and(Clipboard::is_region));

    // A map opened keeps it, and pastes it with the area tool.
    open(&mut session, &answers);
    session.set_tool(Tool::Paint);
    session.begin_paste();
    assert_eq!((session.tool(), session.is_pasting()), (Tool::Area, true));
    click(&mut session, (5.5, 5.5));
    assert_eq!(asked(&engine).concat(), [tile(5, 5, 0, 0)]);
}

#[test]
fn the_menu_opens_on_the_region_with_paste_enabled_only_with_a_clipboard() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 3), (1, 4));
    assert!(session.region_menu().is_none());
    right_click(&mut session, (1, 4));
    let all = [MenuAction::Copy, MenuAction::Cut, MenuAction::Delete, MenuAction::Paste, MenuAction::SaveAsPrefab];
    assert_eq!(entries(&session), all.iter().map(|&action| (action, action != MenuAction::Paste)).collect::<Vec<_>>());
    assert_eq!(session.region_menu().map(|(at, _)| at), Some((1.5, 4.5)));

    // An entry that is not enabled does nothing, and closes the menu.
    session.choose_from_region_menu(MenuAction::Paste);
    assert!(session.region_menu().is_none() && session.region_size() == 4 && !session.is_pasting());
    assert!(asked(&engine).is_empty());

    // Copy: the clipboard is filled, the menu closes and the region stays.
    right_click(&mut session, (0, 3));
    session.choose_from_region_menu(MenuAction::Copy);
    assert!(session.region_menu().is_none() && session.clipboard().is_some() && session.region_size() == 4);
    right_click(&mut session, (0, 3));
    assert!(entries(&session).contains(&(MenuAction::Paste, true)));
    session.close_region_menu();
    // Nor does any entry with the menu closed.
    session.choose_from_region_menu(MenuAction::Delete);
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_menu_s_delete_cut_and_paste_act_on_the_region() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 3), (1, 4));
    right_click(&mut session, (0, 3));
    session.choose_from_region_menu(MenuAction::Delete);
    assert_eq!(labels(&edits(&engine)), ["Delete region"]);
    assert!(session.region.is_none());

    drag(&mut session, (0, 3), (1, 4));
    right_click(&mut session, (0, 3));
    session.choose_from_region_menu(MenuAction::Cut);
    // Nothing stands on it, so the copy is made at once, and the delete with it.
    assert_eq!(labels(&edits(&engine)), ["Cut region"]);
    assert!(session.region.is_none() && session.clipboard().is_some_and(Clipboard::is_region));

    drag(&mut session, (4, 4), (4, 4));
    right_click(&mut session, (4, 4));
    session.choose_from_region_menu(MenuAction::Paste);
    assert_eq!((session.is_pasting(), session.region_size()), (true, 0));
}

#[test]
fn the_menu_closes_on_a_press_escape_or_a_change_of_tool() {
    let (mut session, _engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 3), (1, 4));

    right_click(&mut session, (0, 3));
    session.pointer_down(over(&session, (5, 5)), VIEW, Button::Middle);
    session.pointer_up(over(&session, (5, 5)), VIEW);
    assert!(session.region_menu().is_none());

    // Escape closes the menu alone, and the region stays.
    right_click(&mut session, (0, 3));
    session.escape();
    assert_eq!((session.region_menu().is_none(), session.region_size()), (true, 4));

    right_click(&mut session, (0, 3));
    session.close_region_menu();
    assert!(session.region_menu().is_none());

    right_click(&mut session, (0, 3));
    session.set_tool(Tool::Select);
    assert!(session.region_menu().is_none());
}

#[test]
fn the_edit_menu_acts_on_the_region_when_the_area_tool_holds_one() {
    let (mut session, engine) = Session::offline(&scene());
    session.selection = vec![CHAIR];
    session.set_tool(Tool::Area);
    assert!(!session.holds_region());
    // With no region the entities are copied.
    assert!(session.can_copy() && session.can_delete());
    session.copy();
    assert!(matches!(engine.try_iter().collect::<Vec<_>>()[..], [ToEngine::Copy { .. }]));

    drag(&mut session, (0, 3), (1, 4));
    assert!(session.holds_region());
    // Delete takes the region, and not the selected chair.
    session.delete_key();
    assert_eq!(labels(&edits(&engine)), ["Delete region"]);

    // Copy takes the region, which has nothing on it, at once.
    drag(&mut session, (0, 3), (1, 4));
    session.copy();
    assert_eq!(engine.try_iter().count(), 0);
    assert!(session.clipboard().is_some_and(Clipboard::is_region));

    // With nothing selected and no region there is nothing to act on.
    session.selection.clear();
    session.clear_region();
    assert!(!session.can_copy() && !session.can_delete());
    session.set_tool(Tool::Select);
    assert!(!session.holds_region());
}
