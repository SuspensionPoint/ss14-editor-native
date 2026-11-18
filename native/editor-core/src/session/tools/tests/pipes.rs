//! The pipe tool: the fittings, from the web editor's own cases, and laying and removing.

use super::infra::{knowing, path, put, spawns};
use super::*;
use crate::session::tools::pipes::{angle, fit, open_sides, Piece, EAST, NORTH, SOUTH, WEST};

const SUPPLY: &str = "#0055CCFF";

/// The piece a cell open on these sides calls for, and its angle in whole degrees.
fn fitted(sides: u8) -> (Piece, f64) {
    let (piece, quarters) = fit(sides);
    (piece, angle(quarters).to_degrees().round())
}

#[test]
fn every_pattern_of_sides_gets_the_web_editor_s_piece_and_angle() {
    use Piece::*;
    let cases = [
        // None, or one: a straight along that side, north and south when there is none.
        (0, Straight, 0.0),
        (NORTH, Straight, 0.0),
        (SOUTH, Straight, 0.0),
        (EAST, Straight, 90.0),
        (WEST, Straight, 90.0),
        (NORTH | SOUTH, Straight, 0.0),
        (EAST | WEST, Straight, 90.0),
        // A bend joins south and west unturned.
        (SOUTH | WEST, Bend, 0.0),
        (EAST | SOUTH, Bend, 90.0),
        (NORTH | EAST, Bend, 180.0),
        (WEST | NORTH, Bend, -90.0),
        // A T is named by the side it lacks, north unturned.
        (EAST | SOUTH | WEST, Junction, 0.0),
        (NORTH | EAST | SOUTH, Junction, 90.0),
        (NORTH | EAST | WEST, Junction, 180.0),
        (NORTH | SOUTH | WEST, Junction, -90.0),
        (NORTH | EAST | SOUTH | WEST, Fourway, 0.0),
    ];
    for (sides, piece, degrees) in cases {
        assert_eq!(fitted(sides), (piece, degrees), "sides {sides:04b}");
    }
}

#[test]
fn a_piece_turned_any_way_is_fitted_back_to_the_same_sides() {
    for piece in [Piece::Straight, Piece::Bend, Piece::Junction, Piece::Fourway] {
        for quarters in 0..4 {
            let sides = open_sides(piece, quarters);
            let (again, turned) = fit(sides);
            assert_eq!((again, open_sides(again, turned)), (piece, sides), "{piece:?} turned {quarters}");
        }
    }
}

/// A session that knows the gas and disposal pieces, with the pipe tool in hand laying supply.
fn piping() -> (Session, Receiver<ToEngine>) {
    let mut ids: Vec<&str> = PipeNetwork::Gas.pieces().ids().to_vec();
    ids.extend(PipeNetwork::Disposal.pieces().ids());
    let (mut session, engine) = knowing(&ids);
    session.choose_pipe(PipeNetwork::Gas, Some(SUPPLY.into()));
    (session, engine)
}

/// The prototype, the cell and the angle in whole degrees of each spawn op, in order.
fn laid(ops: &[Value]) -> Vec<(String, (f64, f64), f64)> {
    let spawned = spawns(ops);
    let angles = ops.iter().filter(|op| op["kind"] == "spawn").map(|op| op["rotation"].as_f64().unwrap().to_degrees().round());
    spawned.into_iter().zip(angles).map(|((prototype, at), degrees)| (prototype, at, degrees)).collect()
}

fn piece(prototype: &str, cell: (i32, i32), degrees: f64) -> (String, (f64, f64), f64) {
    (prototype.to_owned(), (cell.0 as f64 + 0.5, cell.1 as f64 + 0.5), degrees)
}

#[test]
fn an_l_gets_straights_and_a_bend_in_one_edit_with_the_colour() {
    let (mut session, engine) = piping();
    assert_eq!(session.tool(), Tool::Pipe);
    path(&mut session, Button::Primary, &[(1, 1), (1, 3), (3, 3)]);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Lay pipes", None));
    assert_eq!(
        laid(&edits[0].2),
        [
            piece("GasPipeStraight", (1, 1), 0.0),
            piece("GasPipeStraight", (1, 2), 0.0),
            // Joined from the south and on to the east.
            piece("GasPipeBend", (1, 3), 90.0),
            piece("GasPipeStraight", (2, 3), 90.0),
            piece("GasPipeStraight", (3, 3), 90.0),
        ]
    );
    // Each takes the colour through its component data.
    let colour = json!([{ "type": "AtmosPipeColor", "color": SUPPLY }]).to_string();
    assert!(edits[0].2.iter().all(|op| op["components"] == json!(colour)), "{:?}", edits[0].2);
}

#[test]
fn a_path_from_the_middle_of_a_line_makes_a_t_and_leaves_the_rest_of_the_line() {
    let (mut session, engine) = piping();
    for (uid, x) in [(50, 1), (51, 2), (52, 3)] {
        put(&mut session, uid, "GasPipeStraight", (x, 2), std::f64::consts::FRAC_PI_2);
    }
    path(&mut session, Button::Primary, &[(2, 2), (2, 4)]);

    let ops = &asked(&engine)[0];
    // The middle of the line is a T open to the north, east and west, in place of the straight.
    assert_eq!(ops[0], json!({ "kind": "delete", "uid": 51 }));
    assert_eq!(laid(ops), [piece("GasPipeTJunction", (2, 2), 180.0), piece("GasPipeStraight", (2, 3), 0.0), piece("GasPipeStraight", (2, 4), 0.0)]);
    // The ends of the line are asked nothing.
    assert!(ops.iter().all(|op| op["uid"] != 50 && op["uid"] != 52));
}

#[test]
fn a_path_across_a_line_makes_a_cross() {
    let (mut session, engine) = piping();
    for (uid, y) in [(50, 1), (51, 2), (52, 3)] {
        put(&mut session, uid, "GasPipeStraight", (2, y), 0.0);
    }
    path(&mut session, Button::Primary, &[(1, 2), (3, 2)]);

    let ops = &asked(&engine)[0];
    assert_eq!(ops.iter().filter(|op| op["kind"] == "delete").cloned().collect::<Vec<_>>(), [json!({ "kind": "delete", "uid": 51 })]);
    assert_eq!(laid(ops), [piece("GasPipeStraight", (1, 2), 90.0), piece("GasPipeFourway", (2, 2), 0.0), piece("GasPipeStraight", (3, 2), 90.0)]);
}

#[test]
fn a_pipe_of_the_same_prototype_is_turned_in_place_and_keeps_its_uid() {
    let (mut session, engine) = piping();
    // A lone straight, north to south. A path from it to the east turns it.
    put(&mut session, 50, "GasPipeStraight", (1, 2), 0.0);
    path(&mut session, Button::Primary, &[(1, 2), (3, 2)]);

    let ops = &asked(&engine)[0];
    assert_eq!(ops[0], json!({ "kind": "rotate", "uid": 50, "rotation": std::f64::consts::FRAC_PI_2 }));
    assert!(ops.iter().all(|op| op["kind"] != "delete"));
    assert_eq!(laid(ops), [piece("GasPipeStraight", (2, 2), 90.0), piece("GasPipeStraight", (3, 2), 90.0)]);
}

#[test]
fn a_pipe_already_fitted_is_asked_nothing() {
    let (mut session, engine) = piping();
    // A straight turned half round runs north to south as one unturned does.
    put(&mut session, 50, "GasPipeStraight", (1, 2), std::f64::consts::PI);
    path(&mut session, Button::Primary, &[(1, 2), (1, 4)]);
    let ops = &asked(&engine)[0];
    assert!(ops.iter().all(|op| op["uid"] != 50), "{ops:?}");
    assert_eq!(laid(ops).len(), 2);
}

#[test]
fn a_line_drawn_alongside_another_stays_apart_and_an_open_end_is_joined() {
    let (mut session, engine) = piping();
    // A line from west to east along row 2, its east end open towards (4, 2).
    for (uid, x) in [(50, 1), (51, 2), (52, 3)] {
        put(&mut session, uid, "GasPipeStraight", (x, 2), std::f64::consts::FRAC_PI_2);
    }
    // Alongside it on row 3.
    path(&mut session, Button::Primary, &[(1, 3), (3, 3)]);
    assert_eq!(laid(&asked(&engine)[0]), [piece("GasPipeStraight", (1, 3), 90.0), piece("GasPipeStraight", (2, 3), 90.0), piece("GasPipeStraight", (3, 3), 90.0)]);

    // From the cell its open end points at, up to the north: that cell joins it with a bend.
    path(&mut session, Button::Primary, &[(4, 2), (4, 4)]);
    let ops = &asked(&engine)[0];
    assert_eq!(laid(ops)[0], piece("GasPipeBend", (4, 2), -90.0));
    assert!(ops.iter().all(|op| op["kind"] == "spawn"));
}

#[test]
fn a_right_click_removes_a_pipe_and_refits_its_neighbours() {
    let (mut session, engine) = piping();
    // An L: up column 1 and along row 3.
    put(&mut session, 50, "GasPipeStraight", (1, 2), 0.0);
    put(&mut session, 51, "GasPipeBend", (1, 3), std::f64::consts::FRAC_PI_2);
    put(&mut session, 52, "GasPipeStraight", (2, 3), std::f64::consts::FRAC_PI_2);
    put(&mut session, 53, "GasPipeStraight", (3, 3), std::f64::consts::FRAC_PI_2);
    // A disposal pipe on the same cell is of another network.
    put(&mut session, 54, "DisposalPipe", (2, 3), 0.0);
    path(&mut session, Button::Secondary, &[(2, 3)]);

    let edits = edits(&engine);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].0, edits[0].1), ("Remove pipes", None));
    let ops = &edits[0].2;
    // The bend has only its south left and becomes a straight. The straight at the east end
    // keeps its shape and is asked nothing.
    assert_eq!(ops[..2], [json!({ "kind": "delete", "uid": 52 }), json!({ "kind": "delete", "uid": 51 })]);
    assert_eq!(laid(ops), [piece("GasPipeStraight", (1, 3), 0.0)]);
    assert_eq!(ops.len(), 3);
}

#[test]
fn a_right_drag_removes_along_it_and_refits_once() {
    let (mut session, engine) = piping();
    // A cross at (2, 2).
    put(&mut session, 50, "GasPipeFourway", (2, 2), 0.0);
    for (uid, cell, degrees) in [(51, (1, 2), 90.0), (52, (3, 2), 90.0), (53, (2, 1), 0.0), (54, (2, 3), 0.0)] {
        put(&mut session, uid, "GasPipeStraight", cell, f64::to_radians(degrees));
    }
    // The arms to the west and the north, both next to the cross.
    path(&mut session, Button::Secondary, &[(1, 2), (1, 3), (2, 3)]);

    let ops = &asked(&engine)[0];
    let deleted: Vec<&Value> = ops.iter().filter(|op| op["kind"] == "delete").map(|op| &op["uid"]).collect();
    assert_eq!(deleted, [&json!(51), &json!(54), &json!(50)]);
    // Left open to the east and the south, the cross is a bend.
    assert_eq!(laid(ops), [piece("GasPipeBend", (2, 2), 90.0)]);
}

#[test]
fn disposal_lays_its_own_pieces_with_no_colour_and_a_junction_for_a_cross() {
    let (mut session, engine) = piping();
    session.choose_pipe(PipeNetwork::Disposal, None);
    // A T of disposal, to be crossed: the junction is turned in place.
    put(&mut session, 50, "DisposalJunction", (2, 2), 0.0);
    put(&mut session, 51, "DisposalPipe", (1, 2), std::f64::consts::FRAC_PI_2);
    put(&mut session, 52, "DisposalPipe", (3, 2), std::f64::consts::FRAC_PI_2);
    put(&mut session, 53, "DisposalPipe", (2, 1), 0.0);
    // A gas pipe beside it is of another network.
    put(&mut session, 54, "GasPipeStraight", (2, 4), 0.0);
    path(&mut session, Button::Primary, &[(2, 2), (2, 4)]);

    let ops = &asked(&engine)[0];
    assert_eq!(laid(ops), [piece("DisposalPipe", (2, 3), 0.0), piece("DisposalPipe", (2, 4), 0.0)]);
    assert!(ops.iter().all(|op| op.get("components").is_none() && op["uid"] != 54));
    // The T is now open all round, which is the same prototype at the same turn.
    assert!(ops.iter().all(|op| op["kind"] == "spawn"), "{ops:?}");
}

#[test]
fn a_network_the_fork_lacks_lays_nothing_and_says_so() {
    let (mut session, engine) = knowing(&["GasPipeStraight", "GasPipeBend"]);
    assert!(!session.pipe_network_available(PipeNetwork::Gas));
    session.choose_pipe(PipeNetwork::Gas, None);
    path(&mut session, Button::Primary, &[(1, 1), (3, 1)]);
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not laid: the fork has no GasPipeTJunction");
}

#[test]
fn an_uncoloured_gas_pipe_has_no_component_data() {
    let (mut session, engine) = piping();
    session.choose_pipe(PipeNetwork::Gas, None);
    path(&mut session, Button::Primary, &[(1, 1)]);
    let ops = &asked(&engine)[0];
    assert_eq!(ops, &[json!({ "kind": "spawn", "prototype": "GasPipeStraight", "parent": GRID, "x": 1.5, "y": 1.5, "rotation": 0.0 })]);
}
