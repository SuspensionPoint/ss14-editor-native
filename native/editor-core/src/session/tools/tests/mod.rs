//! What the tools ask of the engine, read from a session that has no engine behind it.

use super::*;
use crate::drawlist::{KIND_DISPOSALS, KIND_ENTITY, KIND_PLUMBING, KIND_SUBFLOOR};

mod air;
mod air_fix;
mod animate;
mod area;
mod backdrops;
mod decal_select;
mod decals;
mod entity;
mod erase;
mod every_tool;
mod favourites;
mod floors;
mod grids;
mod infra;
mod links;
mod look;
mod marquee;
mod nudge;
mod palettes;
mod paste;
mod pick;
mod pipes;
mod place;
mod prefabs;
mod problems;
mod region_copy;
mod room;
mod select;
mod shapes;
mod tiles;
mod toolbar;
mod trace;
mod turning;
mod view;

const VIEW: (u32, u32) = (640, 640);

const GRID: i32 = 2;
pub(in crate::session) const CHAIR: i32 = 10;
pub(in crate::session) const WALL: i32 = 11;
const CROWBAR: i32 = 12;

/// One grid at the origin with a floor of steel six cells each way. A chair stands loose on the
/// middle of cell (1, 1), a wall is fixed to the floor on the middle of cell (3, 1), and a
/// crowbar is inside the wall.
pub(in crate::session) fn scene() -> String {
    let tiles: Vec<String> = (0..36).map(|i| format!("{},{},1,0,0", i % 6, i / 6)).collect();
    let tiles = tiles.join(",");
    format!(
        r#"{{"document":{{"id":1,"source":"test.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[{tiles}]}}],
            "entities":[
              {{"uid":{CHAIR},"yamlUid":10,"prototype":"Chair","parent":{GRID},"x":1.5,"y":1.5,"rotation":0,"anchored":false,"contained":false,"name":null}},
              {{"uid":{WALL},"yamlUid":11,"prototype":"WallSolid","parent":{GRID},"x":3.5,"y":1.5,"rotation":0,"anchored":true,"contained":false,"name":null}},
              {{"uid":{CROWBAR},"yamlUid":12,"prototype":"Crowbar","parent":{WALL},"x":0,"y":0,"rotation":0,"anchored":false,"contained":true,"name":null}}],
            "history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#
    )
}

/// An edit as it was asked for: its name in the history, its stroke and its ops.
type Edit = (&'static str, Option<u64>, Vec<Value>);

/// Every edit asked for since the last call.
fn edits(from: &Receiver<ToEngine>) -> Vec<Edit> {
    from.try_iter().filter_map(|command| match command { ToEngine::Edit { ops, label, stroke, .. } => Some((label, stroke, ops)), _ => None }).collect()
}

/// The name of every edit asked for since the last call, and whether the engine may pass it
/// over for a later one of its stroke.
fn coalesced(from: &Receiver<ToEngine>) -> Vec<(&'static str, bool)> {
    from.try_iter().filter_map(|command| match command { ToEngine::Edit { label, coalesce, .. } => Some((label, coalesce)), _ => None }).collect()
}

/// The number of every edit asked for since the last call, which the engine's answer carries back.
fn numbers(from: &Receiver<ToEngine>) -> Vec<u64> {
    from.try_iter().filter_map(|command| match command { ToEngine::Edit { seq, .. } => Some(seq), _ => None }).collect()
}

/// The stroke of every edit asked for since the last call, and whether the engine may pass it
/// over for a later one of its stroke.
fn strokes(from: &Receiver<ToEngine>) -> Vec<(Option<u64>, bool)> {
    from.try_iter().filter_map(|command| match command { ToEngine::Edit { stroke, coalesce, .. } => Some((stroke, coalesce)), _ => None }).collect()
}

/// The angle an op asks for, in degrees to the millionth.
fn angle(op: &Value) -> f64 {
    (op["rotation"].as_f64().unwrap().to_degrees() * 1e6).round() / 1e6
}

/// The entity each op of an edit turns, and the angle it asks for (`angle`).
#[track_caller]
fn turns(ops: &[Value]) -> Vec<(i32, f64)> {
    assert!(ops.iter().all(|op| op["kind"] == "rotate"), "{ops:?}");
    ops.iter().map(|op| (op["uid"].as_i64().unwrap() as i32, angle(op))).collect()
}

/// Presses R, with Shift or without, and lets it go.
fn tap_r(session: &mut Session, shift: bool) {
    session.key_down('R', shift);
    session.key_up('R');
}

/// Turns the wheel over the middle of the view, and lets any zoom it owes run out. A notch
/// towards the user is -1.
fn wheel(session: &mut Session, notches: f32) {
    session.wheel(notches, (320.0, 320.0), VIEW);
    glide_out(session);
}

/// Gives every frame the wheel's zoom still owes, a sixtieth of a second each, and says how
/// many there were.
fn glide_out(session: &mut Session) -> usize {
    let mut frames = 0;
    while session.zooming() {
        session.advance_zoom(1.0 / 60.0);
        frames += 1;
    }
    frames
}

/// The ops of every edit asked for since the last call.
fn asked(from: &Receiver<ToEngine>) -> Vec<Vec<Value>> {
    edits(from).into_iter().map(|edit| edit.2).collect()
}

/// The names the edits go by in the history.
fn labels(edits: &[Edit]) -> Vec<&'static str> {
    edits.iter().map(|edit| edit.0).collect()
}

/// Whether edits are one step to undo: there are some, and they are all of one stroke.
fn one_stroke(edits: &[Edit]) -> bool {
    !edits.is_empty() && edits[0].1.is_some() && edits.iter().all(|edit| edit.1 == edits[0].1)
}

/// The cells that tile ops set, in the order of the ops.
fn cells<'a>(ops: impl IntoIterator<Item = &'a Value>) -> Vec<(i64, i64)> {
    ops.into_iter().map(|op| (op["x"].as_i64().unwrap(), op["y"].as_i64().unwrap())).collect()
}

/// The point of the view over a map position, wherever the camera is.
fn at(session: &Session, world: (f32, f32)) -> (f32, f32) {
    session.camera.view_at(world, VIEW)
}

/// The point of the view over the middle of a cell, wherever the camera is. It takes the grid
/// to be at the origin and not turned.
fn over(session: &Session, cell: (i32, i32)) -> (f32, f32) {
    at(session, (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5))
}

/// Presses the primary button over the middle of one cell, moves to another and lets go.
fn drag(session: &mut Session, from: (i32, i32), to: (i32, i32)) {
    session.pointer_down(over(session, from), VIEW, Button::Primary);
    session.pointer_move(over(session, to), VIEW);
    session.pointer_up(over(session, to), VIEW);
}

/// Presses the primary button over a map position and lets go.
fn click(session: &mut Session, world: (f32, f32)) {
    session.pointer_down(at(session, world), VIEW, Button::Primary);
    session.pointer_up(at(session, world), VIEW);
}

/// The one outline the interface would draw over the map.
#[track_caller]
fn outline(session: &Session) -> (OutlineStyle, [(f32, f32); 4]) {
    let outlines = session.outlines();
    assert_eq!(outlines.len(), 1);
    (outlines[0].style, outlines[0].corners)
}

/// Puts one prototype in the palette and chooses it. The placement mode is the game's name for
/// where a mapper may put it.
fn choose(session: &mut Session, placement_mode: &str) {
    session.set_prototypes(vec![Prototype {
        id: "Table".into(),
        name: "table".into(),
        placement_mode: placement_mode.into(),
        ..Default::default()
    }]);
    session.choose_entity(0);
}

/// Gives the session the engine's picture of entities: a square one tile wide around each
/// map position, drawn in the order given, at the depth given. Replaces any picture before.
pub(in crate::session) fn draw(session: &mut Session, entities: &[(i32, (f32, f32), i32)]) {
    let entities: Vec<_> = entities.iter().map(|&(uid, at, depth)| (uid, at, depth, KIND_ENTITY)).collect();
    draw_kinds(session, &entities);
}

/// As `draw`, with what kind of thing the engine says each one is.
fn draw_kinds(session: &mut Session, entities: &[(i32, (f32, f32), i32, u8)]) {
    let document = session.doc.as_ref().unwrap().id;
    let items: Vec<Value> = entities.iter().enumerate().map(|(order, (uid, _, depth, kind))| json!([uid, depth, order, 0.0, 1, kind])).collect();
    let header = json!({ "doc": document, "full": true, "items": items }).to_string();
    let mut message = b"DRW1".to_vec();
    message.extend((header.len() as i32).to_le_bytes());
    message.extend(header.as_bytes());
    for &(_, (x, y), ..) in entities {
        let quad: [f32; FLOATS_PER_QUAD] = [x - 0.5, y - 0.5, x + 0.5, y - 0.5, x + 0.5, y + 0.5, x - 0.5, y + 0.5, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 32.0, 32.0];
        message.extend(quad.iter().flat_map(|float| float.to_le_bytes()));
    }
    session.lists.apply(&message).expect("draw list");
    session.document = Some(document);
}

/// A session with the select tool, whose chair and wall are drawn where they stand.
fn selecting() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.set_tool(Tool::Select);
    (session, engine)
}

/// A session as `selecting` gives it, with the chair selected, a tool in hand, and in the
/// palette what that tool puts down: a table for the place tool, and wood for every other.
fn holding(tool: Tool) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = selecting();
    choose(&mut session, "SnapgridCenter");
    if tool != Tool::Place {
        session.choose_tile("FloorWood");
    }
    session.selection = vec![CHAIR];
    session.set_tool(tool);
    (session, engine)
}
