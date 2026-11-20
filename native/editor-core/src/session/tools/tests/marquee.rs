//! Select's shapes past the rectangle: the ellipse, the lasso, the polygon and Select same, on
//! a region of cells and on entities and decals, with their modes.

use super::super::marquee::{contiguous, ellipse_cells, inside_ellipse, inside_polygon, polygon_cells, TooMany};
use super::decals::decals_on_map;
use super::*;
use crate::model::GridTile;
use crate::toolbox::{SelectShape, Target};

const SHIFT: Held = Held { shift: true, ctrl: false, space: false };
const CTRL: Held = Held { shift: false, ctrl: true, space: false };

// ---- the shapes' geometry ----

/// Every cell of a box whose middle `inside` takes, row by row as the shapes give them.
fn brute(left: i32, bottom: i32, right: i32, top: i32, inside: impl Fn((f32, f32)) -> bool) -> Vec<(i32, i32)> {
    let mut cells = Vec::new();
    for y in bottom..=top {
        for x in left..=right {
            if inside((x as f32 + 0.5, y as f32 + 0.5)) {
                cells.push((x, y));
            }
        }
    }
    cells
}

/// Sorted, for comparing sets of cells.
fn sorted(mut cells: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    cells.sort_unstable();
    cells
}

#[test]
fn a_polygon_holds_its_left_and_bottom_edges_and_not_its_right_and_top() {
    let square = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    assert!(inside_polygon(&square, (1.0, 1.0)));
    assert!(inside_polygon(&square, (0.0, 1.0)));
    assert!(inside_polygon(&square, (1.0, 0.0)));
    assert!(!inside_polygon(&square, (2.0, 1.0)));
    assert!(!inside_polygon(&square, (1.0, 2.0)));
    assert!(!inside_polygon(&square, (-0.5, 1.0)));
    // Fewer than three corners have no inside.
    assert!(!inside_polygon(&[], (0.0, 0.0)));
    assert!(!inside_polygon(&[(0.0, 0.0), (2.0, 2.0)], (1.0, 1.0)));
}

#[test]
fn a_concave_polygon_leaves_its_notch_and_a_crossed_one_goes_by_even_and_odd() {
    // A U: the notch between its arms is outside.
    let u = [(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (2.0, 3.0), (2.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)];
    assert!(inside_polygon(&u, (0.5, 2.5)));
    assert!(inside_polygon(&u, (2.5, 2.5)));
    assert!(inside_polygon(&u, (1.5, 0.5)));
    assert!(!inside_polygon(&u, (1.5, 2.0)));
    assert_eq!(polygon_cells(&u, 100).unwrap(), [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (2, 2)]);
    // A bow tie crossing itself: both halves are inside.
    let bow = [(0.0, 0.0), (4.0, 4.0), (4.0, 0.0), (0.0, 4.0)];
    assert!(inside_polygon(&bow, (0.5, 2.0)));
    assert!(inside_polygon(&bow, (3.5, 2.0)));
    assert!(!inside_polygon(&bow, (2.0, 0.5)));
}

#[test]
fn the_cells_of_a_polygon_are_those_whose_middles_it_holds() {
    // Some lopsided shapes, against every cell tested one at a time.
    let shapes: [&[(f32, f32)]; 4] = [
        &[(0.1, 0.1), (4.1, 0.1), (0.1, 4.1)],
        &[(-3.3, 1.7), (2.2, -4.9), (6.4, 0.3), (1.1, 1.2), (3.9, 5.8)],
        &[(0.5, 0.5), (7.5, 0.5), (7.5, 2.5), (0.5, 2.5)],
        &[(-1.2, -1.2), (5.7, 2.3), (-0.4, 6.6), (2.0, 2.0)],
    ];
    for shape in shapes {
        let expected = brute(-10, -10, 10, 10, |middle| inside_polygon(shape, middle));
        assert!(!expected.is_empty());
        assert_eq!(sorted(polygon_cells(shape, 1000).unwrap()), sorted(expected), "{shape:?}");
    }
    // The triangle: every cell with x + y up to 3.
    assert_eq!(polygon_cells(&[(0.1, 0.1), (4.1, 0.1), (0.1, 4.1)], 100).unwrap().len(), 10);
}

#[test]
fn a_shape_too_small_for_a_middle_takes_the_cells_its_corners_are_in() {
    // A little loop inside one cell.
    assert_eq!(polygon_cells(&[(2.1, 3.1), (2.4, 3.1), (2.2, 3.3)], 100).unwrap(), [(2, 3)]);
    // A thin sliver across two cells, between the rows of middles.
    assert_eq!(polygon_cells(&[(0.1, 0.1), (1.9, 0.1), (1.9, 0.2)], 100).unwrap(), [(0, 0), (1, 0)]);
    // Points all on a line, and a single point.
    assert_eq!(polygon_cells(&[(0.5, 0.5), (1.5, 1.5), (2.5, 2.5)], 100).unwrap(), [(0, 0), (1, 1), (2, 2)]);
    assert_eq!(polygon_cells(&[(-0.5, 4.2)], 100).unwrap(), [(-1, 4)]);
    assert_eq!(polygon_cells(&[], 100).unwrap(), []);
}

#[test]
fn a_shape_of_too_many_cells_is_refused() {
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    assert_eq!(polygon_cells(&square, 100).unwrap().len(), 100);
    assert_eq!(polygon_cells(&square, 99), Err(TooMany));
    assert_eq!(ellipse_cells((0, 0), (9, 9), 5), Err(TooMany));
    assert_eq!(contiguous((0, 0), |_| true, 50), Err(TooMany));
}

#[test]
fn an_ellipse_of_cells_fills_its_box_by_the_cells_middles() {
    // One cell, and a box one cell wide, are all inside.
    assert_eq!(ellipse_cells((3, 3), (3, 3), 10).unwrap(), [(3, 3)]);
    assert_eq!(ellipse_cells((0, 4), (0, 0), 10).unwrap(), [(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
    assert_eq!(ellipse_cells((0, 0), (1, 1), 10).unwrap().len(), 4);
    // Four by four leaves its corners.
    let four = ellipse_cells((0, 0), (3, 3), 100).unwrap();
    assert_eq!(four.len(), 12);
    assert!(![(0, 0), (3, 0), (0, 3), (3, 3)].iter().any(|corner| four.contains(corner)));
    // Each against its cells tested one at a time, either way round.
    for (from, to) in [((0, 0), (7, 3)), ((5, -2), (-4, 9)), ((2, 2), (12, 3))] {
        let (low, high) = ((from.0.min(to.0), from.1.min(to.1)), (from.0.max(to.0), from.1.max(to.1)));
        let a = (low.0 as f32, low.1 as f32);
        let b = (high.0 as f32 + 1.0, high.1 as f32 + 1.0);
        let expected = brute(low.0, low.1, high.0, high.1, |middle| inside_ellipse(a, b, middle));
        assert_eq!(sorted(ellipse_cells(from, to, 1000).unwrap()), sorted(expected), "{from:?} {to:?}");
    }
    // An ellipse on the map with no width or height has no inside.
    assert!(!inside_ellipse((0.0, 0.0), (0.0, 4.0), (0.0, 2.0)));
    assert!(inside_ellipse((0.0, 0.0), (4.0, 2.0), (2.0, 1.0)));
    assert!(!inside_ellipse((0.0, 0.0), (4.0, 2.0), (0.2, 0.2)));
}

#[test]
fn contiguous_cells_touch_side_by_side() {
    let held = [(0, 0), (1, 0), (1, 1), (3, 0), (2, 2)];
    let same = |cell: (i32, i32)| held.contains(&cell);
    // (2, 2) touches (1, 1) only at a corner, and (3, 0) is across a gap.
    assert_eq!(contiguous((0, 0), same, 100).unwrap(), [(0, 0), (1, 0), (1, 1)]);
    assert_eq!(contiguous((3, 0), same, 100).unwrap(), [(3, 0)]);
}

// ---- a region of cells (Tiles on) ----

/// A session with Select in hand on Tiles, in a shape.
fn region_in(shape: SelectShape) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.set_tool(Tool::Area);
    session.set_select_shape(shape);
    assert_eq!(session.tool(), Tool::Area);
    (session, engine)
}

/// The cells of the region, in order.
fn region(session: &Session) -> Vec<(i32, i32)> {
    sorted(session.region.as_ref().map(|region| region.cells().iter().copied().collect()).unwrap_or_default())
}

/// Presses at the first point on the map, moves through the others and lets go at the last.
fn trace(session: &mut Session, points: &[(f32, f32)]) {
    session.pointer_down(at(session, points[0]), VIEW, Button::Primary);
    for &point in &points[1..] {
        session.pointer_move(at(session, point), VIEW);
    }
    let last = *points.last().unwrap();
    session.pointer_up(at(session, last), VIEW);
}

/// Clicks each point in turn.
fn clicks(session: &mut Session, points: &[(f32, f32)]) {
    for &point in points {
        click(session, point);
    }
}

/// Lays a tile on cells of the grid, named as the engine names it.
fn lay(session: &mut Session, name: &str, cells: &[(i32, i32)]) {
    let doc = session.doc.as_mut().unwrap();
    let id = doc.tile_names.iter().position(|known| known == name).unwrap_or_else(|| {
        doc.tile_names.push(name.to_owned());
        doc.tile_names.len() - 1
    }) as u32;
    for &cell in cells {
        doc.grids[0].tiles.insert(cell, GridTile { id, variant: 0, rotation_mirroring: 0 });
    }
}

const TRIANGLE: [(f32, f32); 3] = [(0.1, 0.1), (4.1, 0.1), (0.1, 4.1)];
/// The cells of `TRIANGLE`: those with x + y up to 3.
const TRIANGLE_CELLS: [(i32, i32); 10] = [(0, 0), (0, 1), (0, 2), (0, 3), (1, 0), (1, 1), (1, 2), (2, 0), (2, 1), (3, 0)];

#[test]
fn an_ellipse_selects_the_cells_inside_it_with_its_count_and_ghost_while_dragged() {
    let (mut session, engine) = region_in(SelectShape::Ellipse);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 3)), VIEW);
    // Its outline goes round the box of cells, and the cells it would take are filled.
    let path = session.marquee_path().expect("an outline");
    assert!(path.closed && path.style == OutlineStyle::Drag && path.start.is_none());
    assert!(path.points.iter().all(|point| point.0 >= -0.01 && point.0 <= 4.01 && point.1 >= -0.01 && point.1 <= 4.01));
    assert_eq!(session.overlay_quads().1.len(), 12 * FLOATS_PER_QUAD);
    assert_eq!(session.drag_label(), Some(((2.0, 3.5), "12 tiles".to_owned())));
    assert!(region(&session).is_empty());

    session.pointer_up(over(&session, (3, 3)), VIEW);
    let cells = region(&session);
    assert_eq!(cells.len(), 12);
    assert!(!cells.contains(&(0, 0)) && !cells.contains(&(3, 3)) && cells.contains(&(1, 1)));
    assert!(session.marquee_path().is_none());
    // Selecting asks nothing of the engine.
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_lasso_selects_the_cells_it_goes_round_and_closes_itself() {
    let (mut session, _engine) = region_in(SelectShape::Lasso);
    session.pointer_down(at(&session, (0.1, 0.1)), VIEW, Button::Primary);
    for point in [(1.5, 0.1), (2.9, 0.1), (2.9, 1.5), (2.9, 2.9), (1.5, 2.9)] {
        session.pointer_move(at(&session, point), VIEW);
    }
    let path = session.marquee_path().expect("an outline");
    assert_eq!((path.points.len(), path.closed), (6, true));
    session.pointer_up(at(&session, (0.1, 2.9)), VIEW);
    assert_eq!(region(&session), sorted(brute(0, 0, 2, 2, |_| true)));

    // A lasso that goes nowhere takes the cell it was pressed in.
    let (mut session, _engine) = region_in(SelectShape::Lasso);
    trace(&mut session, &[(4.5, 4.5)]);
    assert_eq!(region(&session), [(4, 4)]);
}

#[test]
fn a_polygon_takes_a_corner_a_click_and_closes_on_its_first_corner() {
    let (mut session, engine) = region_in(SelectShape::Polygon);
    clicks(&mut session, &TRIANGLE[..2]);
    session.pointer_move(at(&session, TRIANGLE[2]), VIEW);
    // Open, from its first corner to the cursor, with the first corner marked.
    let path = session.marquee_path().expect("an outline");
    let start = path.start.expect("its first corner");
    assert_eq!((path.points.len(), path.closed), (3, false));
    assert!((start.0 - TRIANGLE[0].0).abs() < 1e-3 && (start.1 - TRIANGLE[0].1).abs() < 1e-3, "{start:?}");
    assert_eq!(session.drag_label().map(|label| label.1), Some("10 tiles".to_owned()));
    click(&mut session, TRIANGLE[2]);
    assert!(region(&session).is_empty());
    // A click on the first corner closes it.
    click(&mut session, (0.15, 0.12));
    assert_eq!(region(&session), TRIANGLE_CELLS);
    assert!(session.marquee_path().is_none() && asked(&engine).is_empty());
}

#[test]
fn a_double_click_or_enter_closes_a_polygon_and_escape_or_a_right_click_drops_it() {
    // A second click on the last corner.
    let (mut session, _engine) = region_in(SelectShape::Polygon);
    clicks(&mut session, &TRIANGLE);
    click(&mut session, TRIANGLE[2]);
    assert_eq!(region(&session), TRIANGLE_CELLS);

    // Enter.
    let (mut session, _engine) = region_in(SelectShape::Polygon);
    assert!(!session.enter_key());
    clicks(&mut session, &TRIANGLE);
    assert!(session.enter_key());
    assert_eq!(region(&session), TRIANGLE_CELLS);

    // Escape keeps the region there was.
    let (mut session, _engine) = region_in(SelectShape::Polygon);
    session.region = Region::new(GRID, [(5, 5)]);
    session.held = SHIFT;
    clicks(&mut session, &TRIANGLE);
    session.escape();
    assert!(session.marquee_path().is_none());
    assert_eq!(region(&session), [(5, 5)]);
    assert!(!session.enter_key());

    // A right-click drops it the same, and a change of tool.
    clicks(&mut session, &TRIANGLE);
    session.pointer_down(at(&session, (2.0, 2.0)), VIEW, Button::Secondary);
    assert!(session.marquee_path().is_none());
    assert_eq!(region(&session), [(5, 5)]);
    clicks(&mut session, &TRIANGLE[..2]);
    session.set_tool(Tool::Area);
    assert!(session.marquee_path().is_none() && !session.enter_key());
}

#[test]
fn a_polygon_with_two_corners_takes_the_cells_they_are_in() {
    let (mut session, _engine) = region_in(SelectShape::Polygon);
    clicks(&mut session, &[(1.5, 1.5), (3.5, 2.5)]);
    assert!(session.enter_key());
    assert_eq!(region(&session), [(1, 1), (3, 2)]);
}

#[test]
fn shift_adds_ctrl_subtracts_and_the_mode_does_the_same_for_every_shape() {
    let (mut session, _engine) = region_in(SelectShape::Ellipse);
    session.region = Region::new(GRID, [(5, 5)]);
    session.held = SHIFT;
    drag(&mut session, (0, 0), (1, 1));
    assert_eq!(region(&session), [(0, 0), (0, 1), (1, 0), (1, 1), (5, 5)]);
    // Ctrl takes a lasso's cells out.
    session.set_select_shape(SelectShape::Lasso);
    session.held = CTRL;
    trace(&mut session, &[(-0.5, -0.5), (1.9, -0.5), (1.9, 0.9), (-0.5, 0.9)]);
    assert_eq!(region(&session), [(0, 1), (1, 1), (5, 5)]);
    // The Add mode adds a polygon with no key held.
    session.held = Held::default();
    session.set_select_mode(SelectMode::Add);
    session.set_select_shape(SelectShape::Polygon);
    clicks(&mut session, &TRIANGLE);
    assert!(session.enter_key());
    assert_eq!(region(&session), sorted([&TRIANGLE_CELLS[..], &[(5, 5)]].concat()));
    // Subtract takes Select same's cells away, and the cells left need not touch.
    session.set_select_mode(SelectMode::Subtract);
    session.set_select_shape(SelectShape::Same);
    lay(&mut session, "FloorWood", &[(0, 0), (0, 1)]);
    click(&mut session, (0.5, 0.5));
    assert_eq!(region(&session), [(0, 2), (0, 3), (1, 0), (1, 1), (1, 2), (2, 0), (2, 1), (3, 0), (5, 5)]);
}

#[test]
fn a_plain_press_on_the_region_still_moves_it_whatever_the_shape() {
    let (mut session, engine) = region_in(SelectShape::Lasso);
    session.region = Region::new(GRID, [(1, 1), (2, 1)]);
    drag(&mut session, (1, 1), (1, 2));
    assert_eq!(region(&session), [(1, 2), (2, 2)]);
    assert_eq!(labels(&edits(&engine)), ["Move region"]);
}

#[test]
fn a_shape_of_too_many_cells_selects_nothing_and_says_so() {
    let (mut session, _engine) = region_in(SelectShape::Polygon);
    session.region = Region::new(GRID, [(5, 5)]);
    session.held = SHIFT;
    clicks(&mut session, &[(0.0, 0.0), (400.0, 0.0), (400.0, 400.0)]);
    assert_eq!(session.drag_label().map(|label| label.1), Some("Over 50,000 tiles".to_owned()));
    assert!(session.enter_key());
    assert_eq!(region(&session), [(5, 5)]);
    assert_eq!(session.status_line(), "Not selected: the shape covers more than 50,000 tiles");
}

#[test]
fn select_same_takes_the_touching_cells_of_a_tile_or_all_of_them_on_the_grid() {
    let (mut session, engine) = region_in(SelectShape::Same);
    lay(&mut session, "FloorWood", &[(0, 0), (1, 0), (1, 1), (4, 4), (5, 4)]);
    click(&mut session, (1.5, 0.5));
    assert_eq!(region(&session), [(0, 0), (1, 0), (1, 1)]);
    // Off Contiguous, every wood cell on the grid.
    session.set_select_contiguous(false);
    click(&mut session, (1.5, 0.5));
    assert_eq!(region(&session), [(0, 0), (1, 0), (1, 1), (4, 4), (5, 4)]);
    // The steel around it, touching.
    session.set_select_contiguous(true);
    click(&mut session, (3.5, 3.5));
    assert_eq!(region(&session).len(), 36 - 5);
    assert!(asked(&engine).is_empty());

    // Space is not a tile to match: nothing changes and the status line says what to do.
    session.doc.as_mut().unwrap().grids[0].tiles.remove(&(2, 2));
    click(&mut session, (2.5, 2.5));
    assert_eq!(region(&session).len(), 36 - 5);
    assert_eq!(session.status_line(), "Click a tile to select the tiles like it");
}

#[test]
fn a_region_of_any_shape_moves_and_deletes_by_its_own_cells() {
    let (mut session, engine) = region_in(SelectShape::Ellipse);
    drag(&mut session, (0, 0), (3, 3));
    session.delete_key();
    let ops = asked(&engine).remove(0);
    let tiles: Vec<(i64, i64)> = sorted(cells(ops.iter().filter(|op| op["kind"] == "tile")).into_iter().map(|(x, y)| (x as i32, y as i32)).collect()).into_iter().map(|(x, y)| (x as i64, y as i64)).collect();
    assert_eq!(tiles.len(), 12);
    assert!(!tiles.contains(&(0, 0)) && !tiles.contains(&(3, 3)));
    // The chair stands on (1, 1), inside, and the wall on (3, 1), inside too.
    let deleted: Vec<i64> = ops.iter().filter(|op| op["kind"] == "delete").map(|op| op["uid"].as_i64().unwrap()).collect();
    assert_eq!(deleted, [CHAIR as i64, WALL as i64]);

    // A polygon region moves its cells alone.
    let (mut session, engine) = region_in(SelectShape::Polygon);
    clicks(&mut session, &TRIANGLE);
    assert!(session.enter_key());
    session.nudge(Arrow::Right, false);
    assert_eq!(region(&session), sorted(TRIANGLE_CELLS.iter().map(|&(x, y)| (x + 1, y)).collect()));
    let ops = asked(&engine).remove(0);
    assert_eq!(ops.iter().filter(|op| op["kind"] == "air").count(), 10);
}

// ---- entities and decals (Tiles off) ----

const CHAIR_2: i32 = 20;
const CHAIR_FAR: i32 = 21;

/// A session with Select in hand on entities and decals, in a shape, and two more chairs: one
/// on the cell beside the first chair, and one far off on a layer of its own.
fn things_in(shape: SelectShape) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = decals_on_map(&[(1, "Arrows", (1.0, 4.0)), (2, "Arrows", (2.0, 4.0)), (3, "Grass", (3.0, 4.0)), (4, "Arrows", (5.0, 0.0))]);
    let doc = session.doc.as_mut().unwrap();
    let chair = doc.entities[&CHAIR].clone();
    doc.entities.insert(CHAIR_2, Entity { uid: CHAIR_2, x: 2.5, ..chair.clone() });
    doc.entities.insert(CHAIR_FAR, Entity { uid: CHAIR_FAR, x: 5.5, y: 5.5, ..chair });
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), -2), (CHAIR_2, (2.5, 1.5), 0), (CHAIR_FAR, (5.5, 5.5), -2)]);
    session.set_tool(Tool::Select);
    session.set_select_shape(shape);
    assert_eq!(session.tool(), Tool::Select);
    (session, engine)
}

#[test]
fn an_ellipse_selects_what_has_its_middle_inside_it() {
    let (mut session, _engine) = things_in(SelectShape::Ellipse);
    trace(&mut session, &[(0.0, 0.0), (3.0, 3.0)]);
    // The chairs at (1.5, 1.5) and (2.5, 1.5) are inside, the wall at (3.5, 1.5) is not.
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR, CHAIR_2][..], &[][..]));
    // Its outline was the ellipse of the box on the map, with no cells filled.
    let (mut session, _engine) = things_in(SelectShape::Ellipse);
    session.pointer_down(at(&session, (0.0, 0.0)), VIEW, Button::Primary);
    session.pointer_move(at(&session, (4.0, 2.0)), VIEW);
    let path = session.marquee_path().expect("an outline");
    assert!(path.points.contains(&(4.0, 1.0)));
    assert!(session.overlay_quads().1.is_empty() && session.drag_label().is_none());
}

#[test]
fn a_lasso_begins_even_over_an_entity_and_takes_decals_by_their_middle() {
    let (mut session, _engine) = things_in(SelectShape::Lasso);
    // Pressed on the chair, which is not selected: a lasso, not a move. The chair's middle is
    // left out, and the second chair's is in.
    trace(&mut session, &[(1.9, 1.9), (1.9, 5.0), (3.0, 5.0), (3.0, 1.0)]);
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR_2][..], &[(GRID, 2)][..]));
    // A plain press on what is selected moves it.
    let (mut session, engine) = things_in(SelectShape::Lasso);
    session.selection = vec![CHAIR];
    trace(&mut session, &[(1.5, 1.5), (1.5, 2.5)]);
    assert_eq!(labels(&edits(&engine)), ["Move"]);
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn a_polygon_of_things_keeps_to_the_select_targets_and_the_hidden_layers() {
    let (mut session, _engine) = things_in(SelectShape::Polygon);
    let around = [(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)];
    session.toggle_select_target(Target::Decals);
    session.layers.structures = false;
    clicks(&mut session, &around);
    assert!(session.enter_key());
    // No wall, no far chair, no decals.
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR, CHAIR_2][..], &[][..]));
    // Ctrl takes out one chair, by a polygon round it alone.
    session.held = CTRL;
    clicks(&mut session, &[(2.1, 1.1), (2.9, 1.1), (2.9, 1.9)]);
    session.held = Held::default();
    assert!(session.enter_key());
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn select_same_takes_every_touching_entity_of_its_prototype_or_all_on_the_grid() {
    let (mut session, engine) = things_in(SelectShape::Same);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [CHAIR, CHAIR_2]);
    session.set_select_contiguous(false);
    click(&mut session, (2.5, 1.5));
    assert_eq!(session.selection, [CHAIR, CHAIR_2, CHAIR_FAR]);
    // Not one on a hidden layer.
    session.layers.structures = false;
    click(&mut session, (2.5, 1.5));
    assert_eq!(session.selection, [CHAIR, CHAIR_2]);
    // Shift adds the wall's kind, and Ctrl takes the chairs out again.
    session.layers.structures = true;
    session.held = SHIFT;
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.selection, [CHAIR, CHAIR_2, WALL]);
    session.held = CTRL;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    // A click on nothing changes nothing, and Select same never asks the engine.
    session.held = Held::default();
    click(&mut session, (0.5, 5.5));
    assert_eq!(session.selection, [WALL]);
    assert!(asked(&engine).is_empty());
}

#[test]
fn select_same_takes_decals_of_one_prototype_and_keeps_to_the_targets() {
    let (mut session, _engine) = things_in(SelectShape::Same);
    click(&mut session, (1.5, 4.5));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[][..], &[(GRID, 1), (GRID, 2)][..]));
    session.set_select_contiguous(false);
    click(&mut session, (2.5, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 2), (GRID, 4)]);
    // With Entities alone, a click on a decal finds nothing to take.
    session.toggle_select_target(Target::Decals);
    session.held = SHIFT;
    click(&mut session, (3.5, 4.5));
    assert_eq!(session.selected_decals, [(GRID, 1), (GRID, 2), (GRID, 4)]);
    // With Decals alone, a click on a chair over a decal takes no chair.
    session.held = Held::default();
    session.toggle_select_target(Target::Decals);
    session.toggle_select_target(Target::Entities);
    session.doc.as_mut().unwrap().grids[0].decals.get_mut(&3).unwrap().x = 1.0;
    session.doc.as_mut().unwrap().grids[0].decals.get_mut(&3).unwrap().y = 1.0;
    click(&mut session, (1.5, 1.5));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[][..], &[(GRID, 3)][..]));
}
