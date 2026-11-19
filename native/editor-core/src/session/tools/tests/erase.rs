//! The eraser on each target: everything of it that can be seen, or with Only the kind in hand.

use super::decals::decals_on_map;
use super::*;
use crate::toolbox::{BarTool, Shape, Target};

/// The uids an edit's delete ops ask to delete, in their order.
fn deleted(ops: &[Value]) -> Vec<i64> {
    assert!(ops.iter().all(|op| op["kind"] == "delete"), "{ops:?}");
    ops.iter().map(|op| op["uid"].as_i64().unwrap()).collect()
}

/// A session with the eraser on entities and nothing in hand, the chair drawn among the
/// objects and the wall among the structures.
fn erasing_entities() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), -2)]);
    session.take_bar_tool(BarTool::Erase);
    session.set_target(Target::Entities);
    (session, engine)
}

#[test]
fn erase_on_entities_takes_every_entity_it_crosses_as_one_stroke() {
    let (mut session, engine) = erasing_entities();
    assert_eq!((session.tool(), session.palette()), (Tool::Erase, &Palette::None));
    drag(&mut session, (0, 1), (4, 1));

    let edits = edits(&engine);
    assert!(one_stroke(&edits) && labels(&edits).iter().all(|label| *label == "Erase entities"), "{edits:?}");
    let all: Vec<i64> = edits.iter().flat_map(|edit| deleted(&edit.2)).collect();
    // The crowbar inside the wall goes with it, and is not named.
    assert_eq!(all, [CHAIR as i64, WALL as i64]);
}

#[test]
fn erase_leaves_what_is_on_a_hidden_layer_even_with_only() {
    let (mut session, engine) = erasing_entities();
    session.layers.structures = false;
    drag(&mut session, (0, 1), (4, 1));
    assert_eq!(edits(&engine).iter().flat_map(|edit| deleted(&edit.2)).collect::<Vec<_>>(), [CHAIR as i64]);

    // Show entities off hides them all.
    session.layers.structures = true;
    session.layers.entities = false;
    drag(&mut session, (0, 1), (4, 1));
    assert!(asked(&engine).iter().all(Vec::is_empty));

    // Only the wall, with its layer hidden, takes nothing either.
    session.layers.entities = true;
    session.layers.structures = false;
    session.set_prototypes(vec![Prototype { id: "WallSolid".into(), name: "wall".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
    session.set_erase_only(true);
    drag(&mut session, (0, 1), (4, 1));
    assert!(asked(&engine).iter().all(Vec::is_empty));
}

#[test]
fn erase_takes_an_entity_the_map_does_not_draw_only_when_only_names_it() {
    // Nothing is drawn: the chair has no picture.
    let (mut session, engine) = Session::offline(&scene());
    session.take_bar_tool(BarTool::Erase);
    session.set_target(Target::Entities);
    drag(&mut session, (0, 1), (2, 1));
    assert!(asked(&engine).iter().all(Vec::is_empty));

    session.set_prototypes(vec![Prototype { id: "Chair".into(), name: "chair".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
    session.set_erase_only(true);
    drag(&mut session, (0, 1), (2, 1));
    assert_eq!(edits(&engine).iter().flat_map(|edit| deleted(&edit.2)).collect::<Vec<_>>(), [CHAIR as i64]);
}

#[test]
fn erase_never_takes_what_stands_on_the_map_off_the_grid() {
    // A thing on the map itself, over the grid's cell (2, 1).
    let loose = r#"{"uid":13,"yamlUid":13,"prototype":"Beacon","parent":1,"x":2.5,"y":1.5,"rotation":0,"anchored":false,"contained":false,"name":null},"#;
    let (mut session, engine) = Session::offline(&scene().replacen(r#""entities":["#, &format!(r#""entities":[{loose}"#), 1));
    draw(&mut session, &[(13, (2.5, 1.5), 0), (CHAIR, (1.5, 1.5), 0)]);
    session.take_bar_tool(BarTool::Erase);
    session.set_target(Target::Entities);
    session.set_shape(Shape::Rectangle);
    drag(&mut session, (0, 0), (5, 5));
    let edits = edits(&engine);
    assert_eq!((labels(&edits), deleted(&edits[0].2)), (vec!["Erase entities"], vec![CHAIR as i64]));
}

#[test]
fn erase_with_only_on_and_nothing_of_its_target_in_hand_asks_for_one() {
    let (mut session, engine) = erasing_entities();
    session.set_erase_only(true);
    drag(&mut session, (0, 1), (4, 1));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Choose an entity to erase, or turn off Only");
    session.set_target(Target::Tiles);
    session.choose_tile("FloorWood");
    assert_ne!(session.status_line(), "Choose an entity to erase, or turn off Only");
}

#[test]
fn erase_with_only_on_tiles_takes_just_that_tile() {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    // Wood on two cells of the steel floor, as the mirror would have it.
    {
        let doc = session.doc.as_mut().unwrap();
        let wood = doc.tile_names.len() as u32;
        doc.tile_names.push("FloorWood".into());
        for x in 0..2 {
            doc.grids[0].tiles.get_mut(&(x, 0)).unwrap().id = wood;
        }
    }
    session.take_bar_tool(BarTool::Erase);
    session.set_erase_only(true);
    session.set_shape(Shape::Rectangle);
    drag(&mut session, (0, 0), (3, 0));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Erase tiles"]);
    assert_eq!(cells(&edits[0].2), [(0, 0), (1, 0)]);
    // Off, it takes every tile.
    session.set_erase_only(false);
    drag(&mut session, (2, 0), (3, 0));
    assert_eq!(cells(&edits_ops(&engine)), [(2, 0), (3, 0)]);
}

/// The ops of every edit asked for since the last call, one after another.
fn edits_ops(engine: &Receiver<ToEngine>) -> Vec<Value> {
    asked(engine).into_iter().flatten().collect()
}

#[test]
fn erase_on_decals_takes_every_decal_while_their_layer_shows() {
    let (mut session, engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Grass", (1.0, 4.0)), (3, "Arrows", (5.0, 5.0))]);
    session.take_bar_tool(BarTool::Erase);
    session.set_target(Target::Decals);
    session.layers.decals = false;
    drag(&mut session, (0, 4), (3, 4));
    assert!(asked(&engine).iter().all(Vec::is_empty));

    session.layers.decals = true;
    drag(&mut session, (0, 4), (3, 4));
    let edits = edits(&engine);
    assert!(one_stroke(&edits) && labels(&edits).iter().all(|label| *label == "Erase decals"), "{edits:?}");
    let removed: Vec<i64> = edits.iter().flat_map(|edit| &edit.2).map(|op| op["uid"].as_i64().unwrap()).collect();
    assert_eq!(removed, [1, 2]);
}
