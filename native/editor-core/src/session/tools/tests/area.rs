//! The area tool: a region of cells, selected by boxes, and moved with what stands on it.

use super::super::area::{delete_ops, move_ops};
use super::super::select::quarter_on;
use super::*;
use crate::model::Decal;

const SHIFT: Held = Held { shift: true, ctrl: false, space: false };
const CTRL: Held = Held { shift: false, ctrl: true, space: false };
const CTRL_SHIFT: Held = Held { shift: true, ctrl: true, space: false };

/// A session with the area tool in hand.
fn area() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    (session, engine)
}

/// The cells of the region, in order. None with no region.
fn region(session: &Session) -> Vec<(i32, i32)> {
    let mut cells: Vec<(i32, i32)> = session.region.as_ref().map(|region| region.cells().iter().copied().collect()).unwrap_or_default();
    cells.sort_unstable();
    cells
}

/// Drags from the middle of one cell to another with keys held, and lets the keys go.
fn drag_holding(session: &mut Session, held: Held, from: (i32, i32), to: (i32, i32)) {
    session.held = held;
    drag(session, from, to);
    session.held = Held::default();
}

/// A session that is answered: the scene has been opened in it as the engine opens a map.
fn answered() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    open(&mut session, &answers);
    session.set_tool(Tool::Area);
    (session, answers, asked)
}

/// The engine says it has opened the scene.
fn open(session: &mut Session, answers: &Sender<FromEngine>) {
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) }).ok().expect("a session to send to");
    assert!(session.pump(VIEW));
}

#[test]
fn a_box_selects_its_cells_corners_included() {
    let (mut session, engine) = area();
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 2)), VIEW);
    // The box is drawn as it is dragged, with its size.
    assert_eq!(outline(&session), (OutlineStyle::Drag, [(1.0, 1.0), (4.0, 1.0), (4.0, 3.0), (1.0, 3.0)]));
    assert_eq!(session.drag_label(), Some(((2.5, 3.0), "3x2".to_owned())));
    assert!(region(&session).is_empty());
    session.pointer_up(over(&session, (3, 2)), VIEW);
    assert_eq!(region(&session), [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2)]);
    assert_eq!(session.region.as_ref().map(|region| region.grid()), Some(GRID));

    // Dragged the other way it is the same box, and takes the place of the region.
    drag(&mut session, (4, 5), (3, 4));
    assert_eq!(region(&session), [(3, 4), (3, 5), (4, 4), (4, 5)]);
    assert_eq!(session.region_size(), 4);
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_edit_menu_names_the_region_while_the_area_tool_holds_one() {
    let (mut session, _engine) = area();
    assert_eq!(session.edit_target(), "selection");
    click(&mut session, (4.25, 0.75));
    assert_eq!(session.edit_target(), "region");
    // Another tool acts on the selection.
    session.set_tool(Tool::Select);
    assert_eq!(session.edit_target(), "selection");
}

#[test]
fn a_click_selects_one_cell() {
    let (mut session, engine) = area();
    click(&mut session, (4.25, 0.75));
    assert_eq!(region(&session), [(4, 0)]);
    assert_eq!(session.region_cells(), [[(4.0, 0.0), (5.0, 0.0), (5.0, 1.0), (4.0, 1.0)]]);
    // A click beside the region selects that cell in its place.
    click(&mut session, (5.5, 0.5));
    assert_eq!(region(&session), [(5, 0)]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn shift_adds_a_box_and_ctrl_takes_one_away() {
    let (mut session, _engine) = area();
    drag(&mut session, (0, 0), (1, 1));

    session.held = SHIFT;
    session.pointer_down(over(&session, (3, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 1)), VIEW);
    // The box says what it does, and the region is drawn as it is.
    assert_eq!(outline(&session).0, OutlineStyle::DragAdd);
    assert_eq!(session.region_cells().len(), 4);
    session.pointer_up(over(&session, (3, 1)), VIEW);
    assert_eq!(region(&session), [(0, 0), (0, 1), (1, 0), (1, 1), (3, 0), (3, 1)]);

    session.held = CTRL;
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 1)), VIEW);
    assert_eq!(outline(&session).0, OutlineStyle::DragTakeAway);
    session.pointer_up(over(&session, (3, 1)), VIEW);
    assert_eq!(region(&session), [(0, 0), (0, 1), (1, 0), (3, 0)]);

    // Ctrl wins over Shift.
    drag_holding(&mut session, CTRL_SHIFT, (0, 0), (0, 0));
    assert_eq!(region(&session), [(0, 1), (1, 0), (3, 0)]);
}

#[test]
fn taking_every_cell_away_leaves_no_region() {
    let (mut session, _engine) = area();
    drag(&mut session, (1, 1), (2, 2));
    drag_holding(&mut session, CTRL, (0, 0), (3, 3));

    assert!(session.region.is_none());
    assert_eq!((session.region_size(), session.region_cells().len(), session.region_edges().len(), session.region_label()), (0, 0, 0, None));
    // A box that takes away from no region makes none.
    drag_holding(&mut session, CTRL, (0, 0), (1, 1));
    assert!(session.region.is_none());
}

#[test]
fn the_outline_follows_the_shape_and_has_no_edge_inside() {
    let (mut session, _engine) = area();
    // An L of three cells.
    drag(&mut session, (0, 0), (1, 0));
    drag_holding(&mut session, SHIFT, (0, 1), (0, 1));
    assert_eq!(region(&session), [(0, 0), (0, 1), (1, 0)]);

    let edges = session.region_edges();
    assert_eq!(edges.len(), 8);
    // The two edges between its cells are not drawn.
    assert!(!edges.contains(&[(1.0, 0.0), (1.0, 1.0)]) && !edges.contains(&[(0.0, 1.0), (1.0, 1.0)]));
    for edge in [[(0.0, 0.0), (1.0, 0.0)], [(1.0, 0.0), (2.0, 0.0)], [(2.0, 0.0), (2.0, 1.0)], [(1.0, 1.0), (2.0, 1.0)], [(1.0, 1.0), (1.0, 2.0)], [(0.0, 2.0), (1.0, 2.0)], [(0.0, 1.0), (0.0, 2.0)], [(0.0, 0.0), (0.0, 1.0)]] {
        assert!(edges.contains(&edge), "{edge:?}");
    }
    // The fill is a rectangle for each cell, on top of the map.
    let (under, over) = session.overlay_quads();
    assert_eq!((under.len(), over.len()), (0, 3 * FLOATS_PER_QUAD));
    assert_eq!(over[8..12], [50.0 / 255.0, 130.0 / 255.0, 1.0, 0.25]);
}

#[test]
fn a_press_on_the_region_begins_a_move_and_one_outside_begins_a_box() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (2, 2));

    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    assert!(matches!(session.drag, Drag::AreaMove(_)));
    session.pointer_left();
    session.pointer_down(over(&session, (3, 1)), VIEW, Button::Primary);
    assert!(matches!(session.drag, Drag::AreaBox(_)));
    session.pointer_left();

    // With Shift or Ctrl a press on the region begins a box as well.
    for held in [SHIFT, CTRL, CTRL_SHIFT] {
        session.held = held;
        session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
        assert!(matches!(session.drag, Drag::AreaBox(_)), "{held:?}");
        session.pointer_left();
    }
    assert_eq!(region(&session).len(), 4);
    assert!(asked(&engine).is_empty());
}

#[test]
fn changing_the_tool_drops_the_region() {
    let (mut session, _engine) = area();
    drag(&mut session, (1, 1), (2, 2));

    // The same tool taken again keeps it.
    session.set_tool(Tool::Area);
    assert!(session.key_down('S', false));
    assert_eq!(session.region_size(), 4);
    session.set_tool(Tool::Paint);
    assert!(session.region.is_none());
    session.set_tool(Tool::Area);
    assert!(session.region.is_none());

    // Choosing a tile takes the paint tool in hand, which is a change of tool.
    drag(&mut session, (1, 1), (2, 2));
    session.choose_tile("FloorWood");
    assert!(session.region.is_none());
}

#[test]
fn opening_a_map_drops_the_region() {
    let (mut session, answers, _engine) = answered();
    drag(&mut session, (1, 1), (2, 2));
    assert_eq!(session.region_size(), 4);

    open(&mut session, &answers);
    assert!(session.region.is_none());
    assert_eq!(session.tool(), Tool::Area);
}

#[test]
fn a_region_goes_with_its_grid() {
    let (mut session, answers, _engine) = answered();
    drag(&mut session, (1, 1), (2, 2));
    let history = r#"{"canUndo":true,"canRedo":false,"revision":1,"undoLabel":null,"redoLabel":null}"#;
    let changed = |removed: i32| {
        let delta = format!(r#"{{"tiles":[],"entities":[],"removed":[{removed}],"gridsChanged":[],"history":{history},"created":[]}}"#);
        FromEngine::Changed { list: None, delta: Some(Box::new(serde_json::from_str(&delta).unwrap())), what: "Delete", seq: 0, asked: Instant::now(), timing: Timing::default() }
    };

    // Something else goes, and the region stays.
    answers.send(changed(CHAIR)).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.region_size(), 4);
    answers.send(changed(GRID)).ok().expect("a session to send to");
    session.pump(VIEW);
    assert!(session.region.is_none());
}

#[test]
fn a_right_click_off_the_region_clears_it() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (2, 2));
    // On the region it opens the menu, and the region stays.
    session.pointer_down(over(&session, (1, 1)), VIEW, Button::Secondary);
    session.pointer_up(over(&session, (1, 1)), VIEW);
    assert_eq!((session.region_size(), session.region_menu().is_some()), (4, true));
    // Off it, the region goes with its menu.
    session.pointer_down(over(&session, (4, 4)), VIEW, Button::Secondary);
    session.pointer_up(over(&session, (4, 4)), VIEW);
    assert!(session.region.is_none() && session.region_menu().is_none());

    // Escape reaches the session as this, and ends a box that is being dragged too.
    drag(&mut session, (1, 1), (2, 2));
    session.pointer_down(over(&session, (4, 4)), VIEW, Button::Primary);
    session.clear_region();
    assert!(session.region.is_none() && matches!(session.drag, Drag::None));
    session.pointer_up(over(&session, (5, 5)), VIEW);
    assert!(session.region.is_none());
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_label_counts_the_tiles_above_the_region() {
    let (mut session, _engine) = area();
    assert_eq!(session.region_label(), None);
    click(&mut session, (4.5, 0.5));
    assert_eq!(session.region_label(), Some(((4.5, 1.0), "1 tile".to_owned())));
    drag(&mut session, (1, 1), (3, 2));
    assert_eq!(session.region_label(), Some(((2.5, 3.0), "6 tiles".to_owned())));
}

#[test]
fn a_region_is_of_the_cells_of_its_own_grid() {
    let (mut session, _engine) = area();
    // The grid's origin is elsewhere on the map, and it is turned a quarter against the clock.
    let grid = &mut session.doc.as_mut().unwrap().grids[0];
    (grid.x, grid.y, grid.rotation) = (10.0, -5.0, std::f32::consts::FRAC_PI_2);
    session.camera.x = 8.0;
    session.camera.y = -3.0;

    // Its cell (1, 2) is from 7 to 8 across the map and from -4 to -3 up it.
    click(&mut session, (7.5, -3.5));
    assert_eq!(region(&session), [(1, 2)]);
    let corners = session.region_cells()[0];
    for (corner, wanted) in corners.iter().zip([(8.0, -4.0), (8.0, -3.0), (7.0, -3.0), (7.0, -4.0)]) {
        assert!((corner.0 - wanted.0).abs() < 1e-4 && (corner.1 - wanted.1).abs() < 1e-4, "{corners:?}");
    }
    assert_eq!(session.region_edges().len(), 4);
}

// ---- moving ----

/// A region of these cells of the grid.
fn region_of(cells: &[(i32, i32)]) -> Region {
    Region::new(GRID, cells.iter().copied()).unwrap()
}

/// The mirror of the session's document, to put things on the map with.
fn doc(session: &mut Session) -> &mut Doc {
    session.doc.as_mut().unwrap()
}

/// A decal of the grid with its corner at a position.
fn decal(id: i32, x: f32, y: f32) -> Decal {
    Decal { id, prototype: "Arrows".into(), x, y, color: Some("#FF0000FF".into()), angle: 1.5, z_index: 3, cleanable: true, order: id.into() }
}

/// The kind of each op, in order.
fn kinds(ops: &[Value]) -> Vec<&str> {
    ops.iter().map(|op| op["kind"].as_str().unwrap()).collect()
}

/// The entity each `move` op of an edit moves, and where to.
fn moves(ops: &[Value]) -> Vec<(i64, f64, f64)> {
    ops.iter().filter(|op| op["kind"] == "move").map(|op| (op["uid"].as_i64().unwrap(), op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect()
}

/// The `tile` ops of an edit.
fn tile_ops(ops: &[Value]) -> Vec<&Value> {
    ops.iter().filter(|op| op["kind"] == "tile").collect()
}

/// The ops of the one edit asked for since the last call.
#[track_caller]
fn one_edit(from: &Receiver<ToEngine>) -> Vec<Value> {
    let mut edits = asked(from);
    assert_eq!(edits.len(), 1);
    edits.remove(0)
}

#[test]
fn a_region_move_writes_the_destination_before_it_moves_what_stands_there_and_clears_the_source_last() {
    let (mut session, _engine) = area();
    doc(&mut session).grids[0].decals.insert(7, decal(7, 2.0, 1.0));
    // The chair, the decal and the wall stand on the row, which goes three cells up.
    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1), (3, 1)]), (0, 3));

    assert_eq!(kinds(&ops), ["tile", "tile", "tile", "move", "move", "decal.update", "tile", "tile", "tile", "air", "air", "air"]);
    assert_eq!(cells(&ops[..3]), [(1, 4), (2, 4), (3, 4)]);
    assert!(ops[..3].iter().all(|op| op["tile"] == "FloorSteel" && op["grid"] == GRID));
    assert_eq!(moves(&ops), [(CHAIR as i64, 1.5, 4.5), (WALL as i64, 3.5, 4.5)]);
    // The cells left behind name no tile, which sets space.
    assert_eq!(cells(&ops[6..9]), [(1, 1), (2, 1), (3, 1)]);
    assert!(ops[6..9].iter().all(|op| op.get("tile").is_none() && op["grid"] == GRID));
}

/// Where each `air` op of an edit carries the air from, and to.
fn air_moves(ops: &[Value]) -> Vec<((i64, i64), (i64, i64))> {
    let number = |op: &Value, field: &str| op[field].as_i64().unwrap();
    ops.iter()
        .filter(|op| op["kind"] == "air")
        .inspect(|op| assert_eq!(op["grid"], GRID))
        .map(|op| ((number(op, "fromX"), number(op, "fromY")), (number(op, "x"), number(op, "y"))))
        .collect()
}

#[test]
fn a_region_move_carries_the_air_of_each_tile_to_where_the_tile_goes_last() {
    let (mut session, _engine) = area();
    // A cell of the region with no tile has no air to carry.
    doc(&mut session).grids[0].tiles.remove(&(2, 1));
    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1), (3, 1)]), (1, 0));

    // After every other op, overlap and all. The engine reads every cell before it writes one.
    assert_eq!(kinds(&ops)[ops.len() - 2..], ["air", "air"]);
    assert_eq!(air_moves(&ops), [((1, 1), (2, 1)), ((3, 1), (4, 1))]);
}

#[test]
fn a_turned_region_carries_its_air_to_the_turned_cells() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, false);
    // With the clock about the middle of (2, 1): the middle stays, and only places turn.
    assert_eq!(air_moves(&one_edit(&engine)), [((1, 1), (2, 2)), ((2, 1), (2, 1)), ((3, 1), (2, 0))]);
}

/// The decal each `decal.update` op of an edit moves, in order.
fn decal_moves(ops: &[Value]) -> Vec<i64> {
    ops.iter().filter(|op| op["kind"] == "decal.update").map(|op| op["uid"].as_i64().unwrap()).collect()
}

#[test]
fn a_region_move_sends_the_decals_of_the_chunks_it_goes_towards_first() {
    let (mut session, _engine) = area();
    // Two decals in the chunk at 0,0 and two in the one to its right, each pair in draw order
    // against its numbers, and one in the chunk above.
    let decals = &mut doc(&mut session).grids[0].decals;
    for (id, x, y, order) in [(1, 2.0, 1.0, 11), (2, 3.0, 1.0, 10), (3, 17.0, 1.0, 21), (4, 18.0, 1.0, 20), (5, 2.0, 17.0, 30)] {
        decals.insert(id, Decal { order, ..decal(id, x, y) });
    }
    let doc = session.doc.as_ref().unwrap();
    let region = region_of(&[(2, 1), (3, 1), (17, 1), (18, 1), (2, 17)]);
    // Right: the right chunk leaves first, so the left one's decals find it empty.
    assert_eq!(decal_moves(&move_ops(doc, &region, (16, 0))), [4, 3, 2, 1, 5]);
    // Left: the other way about.
    assert_eq!(decal_moves(&move_ops(doc, &region, (-16, 0))), [2, 1, 5, 4, 3]);
    // Down: the lower chunks first, each row as the game draws it.
    assert_eq!(decal_moves(&move_ops(doc, &region, (0, -16))), [2, 1, 4, 3, 5]);
    assert_eq!(decal_moves(&move_ops(doc, &region, (0, 16))), [5, 2, 1, 4, 3]);
}

#[test]
fn a_cell_that_is_source_and_destination_is_written_once_and_not_cleared() {
    let (session, _engine) = area();
    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1), (3, 1)]), (1, 0));

    let tiles = tile_ops(&ops);
    assert_eq!(cells(tiles.iter().copied()), [(2, 1), (3, 1), (4, 1), (1, 1)]);
    // Only the last sets space: the one cell that nothing went to.
    assert_eq!(tiles.iter().map(|op| op.get("tile").is_some()).collect::<Vec<_>>(), [true, true, true, false]);
}

#[test]
fn what_stands_on_the_region_moves_with_it_and_what_stands_beside_it_does_not() {
    let (mut session, engine) = area();
    // A thing at the chair's place that is not of this grid, and a decal beside the region.
    let stranger = Entity { uid: 20, parent: 99, ..doc(&mut session).entities[&CHAIR].clone() };
    doc(&mut session).entities.insert(20, stranger);
    doc(&mut session).grids[0].decals.insert(7, decal(7, 2.0, 1.0));

    // The chair's cell alone, dragged two cells up. The wall is two cells along.
    click(&mut session, (1.5, 1.5));
    drag(&mut session, (1, 1), (1, 3));
    let ops = one_edit(&engine);
    assert_eq!(kinds(&ops), ["tile", "move", "tile", "air"]);
    assert_eq!(moves(&ops), [(CHAIR as i64, 1.5, 3.5)]);
}

#[test]
fn a_contained_entity_gets_no_op() {
    let (mut session, _engine) = area();
    // The crowbar is inside the wall, at (0, 0) of the wall. Another is inside the grid itself.
    let held = Entity { uid: 20, parent: GRID, x: 3.5, y: 1.5, ..doc(&mut session).entities[&CROWBAR].clone() };
    doc(&mut session).entities.insert(20, held);

    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(0, 0), (3, 1)]), (0, 2));
    assert_eq!(moves(&ops), [(WALL as i64, 3.5, 3.5)]);
}

#[test]
fn a_thing_on_a_hidden_layer_moves_with_its_tile() {
    let (mut session, engine) = area();
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.layers = Layers { structures: false, objects: false, ..Layers::default() };
    assert_eq!(session.entity_at((3.5, 1.5)), None);

    click(&mut session, (3.5, 1.5));
    drag(&mut session, (3, 1), (3, 3));
    assert_eq!(moves(&one_edit(&engine)), [(WALL as i64, 3.5, 3.5)]);
}

#[test]
fn a_decal_moves_by_its_corner_with_every_field_resent() {
    let (mut session, _engine) = area();
    let decals = &mut doc(&mut session).grids[0].decals;
    // Its corner is in cell (1, 1), and most of it lies over the cells beside that one.
    decals.insert(7, decal(7, 1.75, 1.5));
    // Its corner is in cell (0, 1), and most of it lies over cell (1, 1).
    decals.insert(8, decal(8, 0.75, 1.0));
    // One with no colour of its own. No decal here is of a prototype the engine listed.
    decals.insert(9, Decal { color: None, cleanable: false, ..decal(9, 1.0, 1.0) });

    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1)]), (2, -1));
    let decals: Vec<&Value> = ops.iter().filter(|op| op["kind"] == "decal.update").collect();
    let moved = |id: i32, x: f32, y: f32, colour: Option<&str>, cleanable: bool| {
        json!({ "kind": "decal.update", "uid": id, "grid": GRID, "x": x, "y": y, "prototype": "Arrows", "color": colour, "rotation": 1.5, "zIndex": 3, "cleanable": cleanable })
    };
    assert_eq!(decals, [&moved(7, 3.75, 0.5, Some("#FF0000FF"), true), &moved(9, 3.0, 0.0, None, false)]);
}

#[test]
fn tiles_move_with_their_variant_and_turn() {
    let (mut session, _engine) = area();
    let wood = doc(&mut session).tile_index("FloorWood");
    let tiles = &mut doc(&mut session).grids[0].tiles;
    tiles.insert((1, 1), crate::model::GridTile { id: wood, variant: 3, rotation_mirroring: 5 });

    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1)]), (0, 2));
    assert_eq!(ops[0], json!({ "kind": "tile", "grid": GRID, "x": 1, "y": 3, "tile": "FloorWood", "variant": 3, "rotationMirroring": 5 }));
    assert_eq!(ops[1], json!({ "kind": "tile", "grid": GRID, "x": 2, "y": 3, "tile": "FloorSteel", "variant": 0, "rotationMirroring": 0 }));
}

#[test]
fn space_in_the_region_overwrites_nothing() {
    let (mut session, _engine) = area();
    // The middle cell of the row has no tile, and the chair stands on it.
    doc(&mut session).grids[0].tiles.remove(&(2, 1));
    doc(&mut session).entities.get_mut(&CHAIR).unwrap().x = 2.5;

    let ops = move_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1), (3, 1)]), (0, 2));
    // Nothing is written to (2, 3), which keeps its tile, and (2, 1) is not cleared.
    assert_eq!(cells(tile_ops(&ops)), [(1, 3), (3, 3), (1, 1), (3, 1)]);
    // What stands on the space goes with the region all the same.
    assert_eq!(moves(&ops), [(CHAIR as i64, 2.5, 3.5), (WALL as i64, 3.5, 3.5)]);
}

#[test]
fn nothing_is_asked_until_the_button_comes_up() {
    let (mut session, engine) = area();
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.tile_sprites.insert("FloorSteel".into(), Some("/Textures/Tiles/steel.png".into()));
    drag(&mut session, (1, 1), (3, 1));

    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 2)), VIEW);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    assert!(asked(&engine).is_empty());
    // The outline and the count are where the region would go, and the mirror is as it was.
    assert_eq!(session.region_label(), Some(((2.5, 4.0), "3 tiles".to_owned())));
    assert!(session.region_edges().contains(&[(1.0, 3.0), (2.0, 3.0)]) && session.outlines().is_empty());
    assert_eq!(region(&session), [(1, 1), (2, 1), (3, 1)]);
    // The ghost is there too: three tiles and the two things that stand on them, all faint.
    let (_, over_map) = session.overlay_quads();
    assert_eq!(over_map.len(), 5 * FLOATS_PER_QUAD);
    assert!(over_map.chunks_exact(FLOATS_PER_QUAD).all(|quad| quad[11] == 0.5 && quad[1] == 3.0 && quad[5] == 4.0));

    // The pointer is lost: nothing is asked, and the region stays where it was.
    session.pointer_left();
    session.pointer_up(over(&session, (2, 3)), VIEW);
    assert!(asked(&engine).is_empty());
    assert_eq!(region(&session), [(1, 1), (2, 1), (3, 1)]);
    assert_eq!(session.region_label(), Some(((2.5, 2.0), "3 tiles".to_owned())));
}

#[test]
fn a_region_dropped_where_it_began_asks_nothing() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));

    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (4, 3)), VIEW);
    session.pointer_move(over(&session, (2, 1)), VIEW);
    session.pointer_up(over(&session, (2, 1)), VIEW);
    assert!(asked(&engine).is_empty());
    assert_eq!(region(&session), [(1, 1), (2, 1), (3, 1)]);
}

#[test]
fn a_region_move_is_one_edit_with_no_stroke() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));
    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    for cell in [(2, 2), (2, 3), (3, 4)] {
        session.pointer_move(over(&session, cell), VIEW);
    }
    session.pointer_up(over(&session, (3, 4)), VIEW);

    let asked: Vec<ToEngine> = engine.try_iter().collect();
    let [ToEngine::Edit { ops, label, stroke, coalesce, .. }] = &asked[..] else { panic!("{} commands", asked.len()) };
    assert_eq!((*label, *stroke, *coalesce), ("Move region", None, false));
    assert_eq!(kinds(ops), ["tile", "tile", "tile", "move", "move", "tile", "tile", "tile", "air", "air", "air"]);
    assert_eq!(cells(&ops[..3]), [(2, 4), (3, 4), (4, 4)]);
}

#[test]
fn the_region_follows_its_tiles() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));
    drag(&mut session, (2, 1), (2, 4));
    assert_eq!(region(&session), [(1, 4), (2, 4), (3, 4)]);
    assert_eq!(asked(&engine).len(), 1);

    // A press where it was begins a box now, and one where it is moves it again.
    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    assert!(matches!(session.drag, Drag::AreaBox(_)));
    session.pointer_left();
    session.region_answered(u64::MAX, false);
    drag(&mut session, (3, 4), (3, 5));
    assert_eq!(region(&session), [(1, 5), (2, 5), (3, 5)]);
    assert_eq!(cells(&one_edit(&engine)[..3]), [(1, 5), (2, 5), (3, 5)]);
}

#[test]
fn undo_drops_the_region() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));
    drag(&mut session, (2, 1), (2, 4));
    session.undo();
    assert!(session.region.is_none());

    // Redo does as well, and either ends a move that is being dragged.
    drag(&mut session, (1, 1), (3, 1));
    session.pointer_down(over(&session, (2, 1)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (2, 3)), VIEW);
    session.redo();
    assert!(session.region.is_none() && matches!(session.drag, Drag::None));
    session.pointer_up(over(&session, (2, 3)), VIEW);
    assert_eq!(asked(&engine).len(), 1);
}

#[test]
fn a_region_of_more_than_fifty_thousand_cells_is_not_moved() {
    let (mut session, engine) = area();
    session.region = Region::new(GRID, (0..50_001).map(|index| (index % 300, index / 300)));

    drag(&mut session, (1, 1), (1, 3));
    assert_eq!(session.status_line(), "Not moved: the region has more than 50,000 tiles");
    assert!(asked(&engine).is_empty());
    assert_eq!(session.region_size(), 50_001);

    // One of fifty thousand is.
    session.region = Region::new(GRID, (0..50_000).map(|index| (index % 300, index / 300)));
    drag(&mut session, (1, 1), (1, 3));
    assert_eq!(asked(&engine).len(), 1);
}

// ---- deleting ----

/// The entity each `delete` op of an edit names.
fn deleted(ops: &[Value]) -> Vec<i64> {
    ops.iter().filter(|op| op["kind"] == "delete").map(|op| op["uid"].as_i64().unwrap()).collect()
}

#[test]
fn deleting_a_region_removes_what_stands_on_it_before_its_tiles() {
    let (mut session, _engine) = area();
    let decals = &mut doc(&mut session).grids[0].decals;
    decals.insert(8, decal(8, 3.25, 1.5));
    decals.insert(7, decal(7, 2.0, 1.0));
    // One beside the region stays.
    decals.insert(9, decal(9, 4.0, 1.0));
    let ops = delete_ops(session.doc.as_ref().unwrap(), &region_of(&[(1, 1), (2, 1), (3, 1)]));

    assert_eq!(kinds(&ops), ["delete", "delete", "decal.remove", "decal.remove", "tile", "tile", "tile", "air", "air", "air"]);
    assert_eq!(ops[..2], [json!({ "kind": "delete", "uid": CHAIR }), json!({ "kind": "delete", "uid": WALL })]);
    assert_eq!(ops[2..4], [json!({ "kind": "decal.remove", "grid": GRID, "uid": 7 }), json!({ "kind": "decal.remove", "grid": GRID, "uid": 8 })]);
    // A tile op that names no tile sets space.
    assert_eq!(cells(&ops[4..7]), [(1, 1), (2, 1), (3, 1)]);
    assert!(ops[4..7].iter().all(|op| op.get("tile").is_none() && op["grid"] == GRID));
    // The air of each emptied cell goes nowhere.
    assert_eq!(ops[7], json!({ "kind": "air", "grid": GRID, "fromX": 1, "fromY": 1, "clear": true }));
    assert_eq!(ops[7..].iter().map(|op| (op["fromX"].as_i64().unwrap(), op["fromY"].as_i64().unwrap())).collect::<Vec<_>>(), [(1, 1), (2, 1), (3, 1)]);
}

#[test]
fn a_contained_entity_is_not_named() {
    let (mut session, _engine) = area();
    // The crowbar is inside the wall, at (0, 0) of the wall. Another is inside the grid itself.
    let held = Entity { uid: 20, parent: GRID, x: 3.5, y: 1.5, ..doc(&mut session).entities[&CROWBAR].clone() };
    doc(&mut session).entities.insert(20, held);
    // A cell of the region with no tile is not cleared.
    doc(&mut session).grids[0].tiles.remove(&(0, 0));

    let ops = delete_ops(session.doc.as_ref().unwrap(), &region_of(&[(0, 0), (3, 1)]));
    assert_eq!(deleted(&ops), [WALL as i64]);
    assert_eq!(cells(tile_ops(&ops)), [(3, 1)]);
}

#[test]
fn a_thing_on_a_hidden_layer_goes_with_its_tile() {
    let (mut session, engine) = area();
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.layers = Layers { structures: false, objects: false, ..Layers::default() };
    assert_eq!(session.entity_at((3.5, 1.5)), None);

    click(&mut session, (3.5, 1.5));
    session.delete_key();
    assert_eq!(deleted(&one_edit(&engine)), [WALL as i64]);
}

#[test]
fn deleting_a_region_is_one_edit_with_no_stroke_and_drops_the_region() {
    let (mut session, engine) = area();
    session.selection = vec![CHAIR];
    drag(&mut session, (2, 1), (3, 1));
    session.delete_key();

    let asked: Vec<ToEngine> = engine.try_iter().collect();
    let [ToEngine::Edit { ops, label, stroke, coalesce, .. }] = &asked[..] else { panic!("{} commands", asked.len()) };
    assert_eq!((*label, *stroke, *coalesce), ("Delete region", None, false));
    // The wall on the region goes, and the selected chair beside it does not.
    assert_eq!(kinds(ops), ["delete", "tile", "tile", "air", "air"]);
    assert_eq!(deleted(ops), [WALL as i64]);
    assert!(session.region.is_none());

    // With no region the key is the selection's again, as it is with any tool.
    session.delete_key();
    assert_eq!(edits(&engine), [("Delete", None, vec![json!({ "kind": "delete", "uid": CHAIR })])]);
}

#[test]
fn delete_with_another_tool_leaves_the_region_s_cells_alone() {
    for tool in Tool::ALL.map(|entry| entry.tool) {
        if tool == Tool::Area {
            continue;
        }
        let (mut session, engine) = area();
        session.selection = vec![CHAIR];
        session.tool = tool;
        // No region outlives its tool. One is put here to show the key does not look for it.
        session.region = Some(region_of(&[(1, 1), (2, 1), (3, 1)]));
        session.delete_key();

        assert_eq!(edits(&engine), [("Delete", None, vec![json!({ "kind": "delete", "uid": CHAIR })])], "{tool:?}");
        assert_eq!(session.region_size(), 3, "{tool:?}");
    }
}

#[test]
fn a_region_of_more_than_fifty_thousand_cells_is_not_deleted() {
    let (mut session, engine) = area();
    session.region = Region::new(GRID, (0..50_001).map(|index| (index % 300, index / 300)));
    session.delete_key();
    assert_eq!(session.status_line(), "Not deleted: the region has more than 50,000 tiles");
    assert!(asked(&engine).is_empty());
    assert_eq!(session.region_size(), 50_001);

    // One of fifty thousand is: the grid's 36 tiles and their air, the chair and the wall.
    session.region = Region::new(GRID, (0..50_000).map(|index| (index % 300, index / 300)));
    session.delete_key();
    assert_eq!(kinds(&one_edit(&engine)).len(), 74);
}

// ---- turning ----

const QUARTER: f64 = std::f64::consts::FRAC_PI_2;

/// Presses R and lets it go, and has the engine's answer to it heard, as it would be before
/// a hand presses R again. Nothing answers here, and a region waits for its last turn.
fn turn(session: &mut Session, shift: bool) {
    tap_r(session, shift);
    session.region_answered(u64::MAX, false);
}

/// The entity each `rotate` op of an edit turns, and the angle it asks for.
fn rotations(ops: &[Value]) -> Vec<(i64, f64)> {
    ops.iter().filter(|op| op["kind"] == "rotate").map(|op| (op["uid"].as_i64().unwrap(), op["rotation"].as_f64().unwrap())).collect()
}

/// Where each `decal.update` op of an edit puts its decal, and the angle it asks for.
fn decals_put(ops: &[Value]) -> Vec<(i64, f64, f64, f64)> {
    let number = |op: &Value, field: &str| op[field].as_f64().unwrap();
    ops.iter().filter(|op| op["kind"] == "decal.update").map(|op| (op["uid"].as_i64().unwrap(), number(op, "x"), number(op, "y"), number(op, "rotation"))).collect()
}

#[test]
fn a_quarter_turn_takes_each_cell_about_the_pivot() {
    let (mut session, engine) = area();
    doc(&mut session).grids[0].decals.insert(7, decal(7, 2.0, 1.0));
    // A row of three. The middle of its box is the middle of cell (2, 1).
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, false);

    assert_eq!(region(&session), [(2, 0), (2, 1), (2, 2)]);
    let asked: Vec<ToEngine> = engine.try_iter().collect();
    let [ToEngine::Edit { ops, label, stroke, coalesce, .. }] = &asked[..] else { panic!("{} commands", asked.len()) };
    assert_eq!((*label, *stroke, *coalesce), ("Turn region", None, false));
    // The order of a move: where the tiles go, the entities, the decals, the cells left, the air.
    assert_eq!(kinds(ops), ["tile", "tile", "tile", "move", "rotate", "move", "rotate", "decal.update", "tile", "tile", "air", "air", "air"]);
    // With the clock the left end goes to the top and the right end to the bottom.
    assert_eq!(cells(&ops[..3]), [(2, 2), (2, 1), (2, 0)]);
    assert!(ops[..3].iter().all(|op| op["tile"] == "FloorSteel" && op["grid"] == GRID));
    assert_eq!(cells(&ops[8..10]), [(1, 1), (3, 1)]);
    assert!(ops[8..10].iter().all(|op| op.get("tile").is_none()));
}

#[test]
fn four_turns_bring_a_region_of_three_by_two_back_where_it_was() {
    let (mut session, _engine) = area();
    // The middle of its box is neither a cell's corner nor a cell's middle. The pivot is
    // half a cell to the right of it, the corner at (3, 2), and stays there.
    drag(&mut session, (1, 1), (3, 2));
    let began = region(&session);
    turn(&mut session, false);
    assert_eq!(region(&session), [(2, 1), (2, 2), (2, 3), (3, 1), (3, 2), (3, 3)]);
    for _ in 0..3 {
        turn(&mut session, false);
    }
    assert_eq!(region(&session), began);
}

#[test]
fn a_move_takes_the_pivot_along_and_other_cells_forget_it() {
    let (mut session, _engine) = area();
    drag(&mut session, (1, 1), (3, 2));
    turn(&mut session, false);
    // One cell up, and three more turns: where it began, one cell up.
    drag(&mut session, (2, 1), (2, 2));
    session.region_answered(u64::MAX, false);
    for _ in 0..3 {
        turn(&mut session, false);
    }
    assert_eq!(region(&session), [(1, 2), (1, 3), (2, 2), (2, 3), (3, 2), (3, 3)]);

    // A box dragged anew is another region, which turns about the middle of its own box:
    // two by three, about the middle of cell (3, 2).
    drag(&mut session, (2, 1), (3, 3));
    turn(&mut session, false);
    assert_eq!(region(&session), [(2, 2), (2, 3), (3, 2), (3, 3), (4, 2), (4, 3)]);
}

#[test]
fn an_l_stays_an_l_when_turned() {
    let (mut session, engine) = area();
    drag(&mut session, (0, 0), (1, 0));
    drag_holding(&mut session, SHIFT, (0, 1), (0, 1));
    turn(&mut session, false);

    // About the corner at (1, 1). The cell of its box that is not of the L is not read: it
    // is written as the cell one of the L's goes to, and not cleared.
    assert_eq!(region(&session), [(0, 0), (0, 1), (1, 1)]);
    assert_eq!(session.region_edges().len(), 8);
    let ops = one_edit(&engine);
    assert_eq!(cells(tile_ops(&ops)), [(0, 1), (1, 1), (0, 0), (1, 0)]);
    assert_eq!(tile_ops(&ops).iter().map(|op| op.get("tile").is_some()).collect::<Vec<_>>(), [true, true, true, false]);
}

#[test]
fn an_entity_turns_about_the_pivot_and_faces_a_quarter_on() {
    let (mut session, engine) = area();
    doc(&mut session).entities.get_mut(&WALL).unwrap().rotation = std::f64::consts::PI;
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, false);

    let ops = one_edit(&engine);
    assert_eq!(moves(&ops), [(CHAIR as i64, 2.5, 2.5), (WALL as i64, 2.5, 0.5)]);
    // With the clock is a quarter less, kept within the turn the angle is in.
    assert_eq!(rotations(&ops), [(CHAIR as i64, -QUARTER), (WALL as i64, QUARTER)]);
}

#[test]
fn a_decal_turns_about_the_pivot_by_its_middle() {
    let (mut session, engine) = area();
    let decals = &mut doc(&mut session).grids[0].decals;
    // One drawn exactly on cell (1, 1), and one a quarter of a cell to the right of it.
    decals.insert(7, decal(7, 1.0, 1.0));
    decals.insert(8, decal(8, 1.25, 1.0));
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, false);

    // The first is exactly on the cell its cell went to. The second is a quarter of a cell
    // below that, as the region's picture has it turned, with its corner in the next cell.
    let angle = quarter_on(1.5, true);
    assert_eq!(decals_put(&one_edit(&engine)), [(7, 2.0, 2.0, angle), (8, 2.0, 1.75, angle)]);
    assert!((angle - (1.5 - QUARTER)).abs() < 1e-12);
}

#[test]
fn a_decal_whose_corner_would_land_off_the_tiles_falls_back_onto_its_own_cell_s_turned_cell() {
    let (mut session, engine) = area();
    // Its corner is in cell (1, 1), three quarters of the way up it.
    doc(&mut session).grids[0].decals.insert(7, decal(7, 1.0, 1.75));
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, true);
    // Turned against the clock as one piece its corner is in cell (1, 0), beside the turned
    // region, which has a tile.
    let ops = one_edit(&engine);
    assert_eq!(decals_put(&ops), [(7, 1.25, 0.0, quarter_on(1.5, false))]);

    // With no tile there it goes to cell (2, 0), where its own cell went, and stands in it
    // as it stood in its own, turned.
    let (mut session, engine) = area();
    doc(&mut session).grids[0].decals.insert(7, decal(7, 1.0, 1.75));
    doc(&mut session).grids[0].tiles.remove(&(1, 0));
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, true);
    assert_eq!(decals_put(&one_edit(&engine)), [(7, 2.25, 0.0, quarter_on(1.5, false))]);
}

#[test]
fn a_tile_that_allows_turning_has_its_byte_turned_and_one_that_does_not_keeps_it() {
    let (mut session, engine) = area();
    session.tiles = vec![Tile { id: "FloorWood".into(), sprite: None, allows_rotation: true, can_crowbar: false }, Tile { id: "FloorSteel".into(), sprite: None, allows_rotation: false, can_crowbar: false }];
    let wood = doc(&mut session).tile_index("FloorWood");
    let steel = doc(&mut session).tile_index("FloorSteel");
    let tiles = &mut doc(&mut session).grids[0].tiles;
    // Mirrored and a quarter on, and one that is not drawn by its byte.
    tiles.insert((1, 1), crate::model::GridTile { id: wood, variant: 3, rotation_mirroring: 5 });
    tiles.insert((2, 1), crate::model::GridTile { id: steel, variant: 1, rotation_mirroring: 2 });
    drag(&mut session, (1, 1), (2, 1));

    // About the middle of cell (2, 1). With the clock the count of quarters goes down, and
    // the mirroring stays.
    turn(&mut session, false);
    let ops = one_edit(&engine);
    assert_eq!(ops[0], json!({ "kind": "tile", "grid": GRID, "x": 2, "y": 2, "tile": "FloorWood", "variant": 3, "rotationMirroring": 4 }));
    assert_eq!(ops[1], json!({ "kind": "tile", "grid": GRID, "x": 2, "y": 1, "tile": "FloorSteel", "variant": 1, "rotationMirroring": 2 }));

    // Against the clock it goes up. The mirror here has not been told of the first turn.
    session.region = Some(region_of(&[(1, 1), (2, 1)]));
    turn(&mut session, true);
    let ops = one_edit(&engine);
    assert_eq!((&ops[0]["rotationMirroring"], &ops[1]["rotationMirroring"]), (&json!(6), &json!(2)));
}

#[test]
fn shift_r_turns_the_other_way() {
    let (mut session, engine) = area();
    drag(&mut session, (1, 1), (3, 1));
    turn(&mut session, true);

    // Against the clock the left end goes to the bottom and the right end to the top.
    let ops = one_edit(&engine);
    assert_eq!(cells(&ops[..3]), [(2, 0), (2, 1), (2, 2)]);
    assert_eq!(moves(&ops), [(CHAIR as i64, 2.5, 0.5), (WALL as i64, 2.5, 2.5)]);
    assert_eq!(rotations(&ops), [(CHAIR as i64, QUARTER), (WALL as i64, QUARTER)]);
    // And one turn each way is where it began.
    turn(&mut session, false);
    assert_eq!(region(&session), [(1, 1), (2, 1), (3, 1)]);
}

#[test]
fn r_with_the_area_tool_turns_the_region_and_does_not_choose_the_rectangle() {
    let (mut session, engine) = area();
    session.selection = vec![CHAIR];
    // With no region R is still the area tool's, and turns nothing.
    assert!(session.key_down('R', false));
    session.key_up('R');
    assert_eq!((session.tool(), asked(&engine).len()), (Tool::Area, 0));

    // The turn is asked for as R comes up. The wheel zooms in the meantime.
    drag(&mut session, (4, 4), (5, 4));
    assert!(session.key_down('R', false));
    assert_eq!((session.tool(), session.wheel_turns()), (Tool::Area, false));
    let zoom = session.camera.zoom;
    wheel(&mut session, 1.0);
    assert!(session.camera.zoom > zoom && asked(&engine).is_empty());
    session.key_up('R');
    assert_eq!(labels(&edits(&engine)), ["Turn region"]);
    assert_eq!((session.tool(), region(&session)), (Tool::Area, vec![(5, 4), (5, 5)]));

    // R that is lost turns nothing.
    session.region_answered(u64::MAX, false);
    session.key_down('R', false);
    session.key_lost('R');
    session.key_up('R');
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_turn_waits_for_the_engine_s_answer_to_the_last() {
    let (mut session, answers, engine) = answered();
    drag(&mut session, (1, 1), (3, 1));
    tap_r(&mut session, false);
    let turned = region(&session);
    let first = numbers(&engine);
    assert_eq!(first.len(), 1);

    // The mirror still has the tiles where they were, so a second turn is not asked for.
    tap_r(&mut session, false);
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not turned: the engine hasn't answered the region's last change yet");
    assert_eq!(region(&session), turned);

    // Answered, it turns again.
    answers.send(FromEngine::Changed { list: None, delta: None, what: "Turn region", seq: first[0], asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump(VIEW);
    tap_r(&mut session, false);
    let second = numbers(&engine);
    assert_eq!(second.len(), 1);

    // A turn the engine refuses leaves the tiles where they were, and the region goes.
    answers.send(FromEngine::Refused { seq: second[0], what: "Turn region", error: "A decal needs a tile under it.".into() }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert!(session.region.is_none());
}

#[test]
fn a_move_and_a_delete_wait_for_the_engine_s_answer_to_the_last_move() {
    let (mut session, answers, engine) = answered();
    drag(&mut session, (1, 1), (3, 1));
    drag(&mut session, (2, 1), (2, 2));
    let moved = region(&session);
    let first = numbers(&engine);
    assert_eq!(first.len(), 1);

    // The mirror still has the tiles where they were, so the region is not moved on.
    drag(&mut session, (2, 2), (2, 3));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not moved: the engine hasn't answered the region's last change yet");
    assert_eq!(region(&session), moved);

    // It is not deleted either, and neither is a region boxed anew meanwhile.
    session.delete_key();
    assert_eq!(session.status_line(), "Not deleted: the engine hasn't answered the region's last change yet");
    drag_holding(&mut session, SHIFT, (4, 2), (4, 2));
    session.delete_key();
    tap_r(&mut session, false);
    assert!(asked(&engine).is_empty());
    assert_eq!(session.region_size(), 4);

    // Answered, it goes.
    answers.send(FromEngine::Changed { list: None, delta: None, what: "Move region", seq: first[0], asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump(VIEW);
    session.delete_key();
    assert_eq!(numbers(&engine).len(), 1);
    assert!(session.region.is_none());
}

#[test]
fn a_region_of_more_than_fifty_thousand_cells_is_not_turned() {
    let (mut session, engine) = area();
    session.region = Region::new(GRID, (0..50_001).map(|index| (index % 300, index / 300)));
    tap_r(&mut session, false);
    assert_eq!(session.status_line(), "Not turned: the region has more than 50,000 tiles");
    assert!(asked(&engine).is_empty());
}

#[test]
fn an_angle_goes_on_by_quarters_within_the_turn_it_is_in() {
    let (pi, tau) = (std::f64::consts::PI, std::f64::consts::TAU);
    // The cases of the engine check (`RegionCheck.cs`, `QuarterCases`): an angle, and what it
    // is a quarter on against the clock. With the clock each is the other way round.
    let cases = [
        (0.0, QUARTER),
        (QUARTER, pi),
        // A half turn is the last of its four, and the next is the first of the same four.
        (pi, -QUARTER),
        (-QUARTER, 0.0),
        // Three quarters is in the next turn up, and stays there.
        (3.0 * QUARTER, tau),
        (tau, 5.0 * QUARTER),
        (5.0 * QUARTER, 3.0 * pi),
        (3.0 * pi, 3.0 * QUARTER),
        // Minus a half turn is the last of the four below.
        (-pi, -5.0 * QUARTER),
        (-5.0 * QUARTER, -tau),
        // Half way between two quarters, as a thing set cornerwise is.
        (0.75 * pi, -0.75 * pi),
        (-0.75 * pi, -0.25 * pi),
        (-0.25 * pi, 0.25 * pi),
        (0.25 * pi, 0.75 * pi),
    ];
    for (angle, on) in cases {
        assert_eq!(quarter_on(angle, false), on, "{angle}");
        assert_eq!(quarter_on(on, true), angle, "{on}");
    }

    // An angle that is no whole number of quarters keeps what it has over, and four turns
    // either way bring any angle back.
    assert!((quarter_on(0.1, false) - (QUARTER + 0.1)).abs() < 1e-15);
    for angle in [0.0, QUARTER, pi, -QUARTER, 3.0 * QUARTER, tau, -pi, 0.1, 1.5, -3.0, 2.356194490192345, 7.0] {
        for clockwise in [false, true] {
            let back = (0..4).fold(angle, |angle, _| quarter_on(angle, clockwise));
            assert!((back - angle).abs() < 1e-12, "{angle} {clockwise}: {back}");
            if angle % QUARTER == 0.0 {
                assert_eq!(back, angle, "{angle} {clockwise}");
            }
        }
    }
}
