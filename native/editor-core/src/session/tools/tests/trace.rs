//! Paint's and Erase's lasso and polygon, and Fill or Outline for their closed shapes.

use super::super::marquee::polygon_cells;
use super::super::shapes::{disc_cells, edge_cells};
use super::super::tiles::rectangle_cells;
use super::decals::with_decals;
use super::*;
use crate::toolbox::{BarTool, SelectShape, Shape, ShapeFill, Target};

/// Sorted, for comparing sets of cells.
fn sorted<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    items.sort();
    items
}

/// Presses at the first point on the map, moves through the others and lets go at the last.
fn lasso(session: &mut Session, points: &[(f32, f32)]) {
    session.pointer_down(at(session, points[0]), VIEW, Button::Primary);
    for &point in &points[1..] {
        session.pointer_move(at(session, point), VIEW);
    }
    let last = *points.last().unwrap();
    session.pointer_up(at(session, last), VIEW);
}

/// Clicks each corner in turn, and closes the polygon with Enter.
fn polygon(session: &mut Session, points: &[(f32, f32)]) {
    for &point in points {
        click(session, point);
    }
    assert!(session.enter_key());
}

/// A session with Paint in a shape and wood in hand.
fn painting(shape: Shape) -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    session.take_bar_tool(BarTool::Paint);
    session.set_shape(shape);
    (session, engine)
}

/// The cells of tile ops, sorted.
fn tiled(ops: &[Value]) -> Vec<(i32, i32)> {
    sorted(ops.iter().filter(|op| op["kind"] == "tile").map(|op| (op["x"].as_i64().unwrap() as i32, op["y"].as_i64().unwrap() as i32)))
}

/// The cells of spawn ops, sorted.
fn spawned(ops: &[Value]) -> Vec<(i32, i32)> {
    sorted(ops.iter().filter(|op| op["kind"] == "spawn").map(|op| (op["x"].as_f64().unwrap().floor() as i32, op["y"].as_f64().unwrap().floor() as i32)))
}

/// Shapes on the floor of the scene, which is six cells each way: a triangle, a lopsided
/// shape crossing itself, a little loop inside one cell, and a sliver too thin for a middle.
const SHAPES: [&[(f32, f32)]; 4] = [
    &[(0.1, 0.1), (4.1, 0.1), (0.1, 4.1)],
    &[(0.3, 0.2), (5.6, 1.1), (1.2, 5.7), (4.4, 4.9), (2.1, 0.4)],
    &[(2.1, 3.1), (2.4, 3.1), (2.2, 3.3)],
    &[(0.1, 0.1), (1.9, 0.1), (1.9, 0.4)],
];

#[test]
fn a_lasso_and_a_polygon_paint_exactly_the_cells_select_would_take() {
    for points in SHAPES {
        let wanted = sorted(polygon_cells(points, 1000).unwrap());
        assert!(!wanted.is_empty());

        // What Select's lasso takes as a region.
        let (mut session, _engine) = Session::offline(&scene());
        session.set_tool(Tool::Area);
        session.set_select_shape(SelectShape::Lasso);
        lasso(&mut session, points);
        let region = sorted(session.region.as_ref().unwrap().cells().iter().copied());
        assert_eq!(region, wanted, "{points:?}");

        // Paint's lasso and polygon paint the same cells, one edit with no stroke each.
        for (shape, label) in [(Shape::Lasso, "Lasso of tiles"), (Shape::Polygon, "Polygon of tiles")] {
            let (mut session, engine) = painting(shape);
            assert_eq!(session.tool(), if shape == Shape::Lasso { Tool::Lasso } else { Tool::Polygon });
            if shape == Shape::Lasso {
                lasso(&mut session, points);
            } else {
                polygon(&mut session, points);
            }
            let edits = edits(&engine);
            assert_eq!(labels(&edits), [label], "{points:?}");
            assert_eq!(edits[0].1, None);
            assert_eq!(tiled(&edits[0].2), wanted, "{shape:?} {points:?}");
        }
    }
}

#[test]
fn a_lasso_shows_the_cells_it_would_paint_while_it_is_dragged() {
    let (mut session, engine) = painting(Shape::Lasso);
    let points = SHAPES[0];
    session.pointer_down(at(&session, points[0]), VIEW, Button::Primary);
    for &point in &points[1..] {
        session.pointer_move(at(&session, point), VIEW);
    }
    assert!(asked(&engine).is_empty());
    // Its ten cells are filled, its outline is closed, and its count is said over it.
    assert_eq!(session.overlay_quads().1.len(), 10 * FLOATS_PER_QUAD);
    let path = session.marquee_path().expect("an outline");
    assert!(path.closed && path.start.is_none() && path.style == OutlineStyle::Drag);
    assert_eq!(session.drag_label().map(|label| label.1), Some("10 cells".to_owned()));

    // With Outline, only the edge: cells with a side outside the triangle.
    let (mut session, _engine) = painting(Shape::Lasso);
    session.set_shape_fill(ShapeFill::Outline);
    session.pointer_down(at(&session, points[0]), VIEW, Button::Primary);
    for &point in &points[1..] {
        session.pointer_move(at(&session, point), VIEW);
    }
    assert_eq!(session.overlay_quads().1.len(), 9 * FLOATS_PER_QUAD);
    assert_eq!(session.drag_label().map(|label| label.1), Some("9 cells".to_owned()));
}

#[test]
fn a_polygon_closes_on_its_first_corner_or_a_double_click_and_shows_its_cells_on_the_way() {
    let triangle = SHAPES[0];
    let (mut session, engine) = painting(Shape::Polygon);
    click(&mut session, triangle[0]);
    click(&mut session, triangle[1]);
    session.pointer_move(at(&session, triangle[2]), VIEW);
    // Open, from its first corner to the cursor, with the cursor as the next corner.
    let path = session.marquee_path().expect("an outline");
    assert_eq!((path.points.len(), path.closed, path.start.is_some()), (3, false, true));
    assert_eq!(session.overlay_quads().1.len(), 10 * FLOATS_PER_QUAD);
    click(&mut session, triangle[2]);
    assert!(asked(&engine).is_empty());
    // A click near the first corner closes it.
    click(&mut session, (0.15, 0.12));
    assert_eq!(tiled(&asked(&engine)[0]), sorted(polygon_cells(triangle, 100).unwrap()));
    assert!(session.marquee_path().is_none() && !session.enter_key());

    // A second click on the last corner closes it too.
    let (mut session, engine) = painting(Shape::Polygon);
    for &point in triangle {
        click(&mut session, point);
    }
    click(&mut session, triangle[2]);
    assert_eq!(tiled(&asked(&engine)[0]).len(), 10);
}

#[test]
fn escape_a_right_click_or_another_tool_drops_a_polygon() {
    let triangle = SHAPES[0];
    for how in ["escape", "right-click", "tool"] {
        for bar in [BarTool::Paint, BarTool::Erase] {
            let (mut session, engine) = painting(Shape::Polygon);
            session.take_bar_tool(bar);
            session.set_shape(Shape::Polygon);
            session.selection = vec![CHAIR];
            for &point in triangle {
                click(&mut session, point);
            }
            assert!(session.marquee_path().is_some());
            match how {
                "escape" => session.escape(),
                "right-click" => session.pointer_down(at(&session, (2.0, 2.0)), VIEW, Button::Secondary),
                _ => session.take_bar_tool(BarTool::Fill),
            }
            assert!(session.marquee_path().is_none(), "{how} {bar:?}");
            assert!(!session.enter_key(), "{how} {bar:?}");
            assert!(asked(&engine).is_empty(), "{how} {bar:?}");
            // Escape went to the polygon alone, and the selection stays.
            assert_eq!(session.selection, [CHAIR], "{how} {bar:?}");
        }
    }
}

#[test]
fn a_lasso_of_entities_puts_one_on_each_cell_with_a_tile_turned_by_r_and_skips_what_holds_one() {
    let (mut session, engine) = super::floors::knowing_walls();
    // A table, turned a quarter the other way.
    session.choose_entity(5);
    session.take_bar_tool(BarTool::Paint);
    session.set_shape(Shape::Lasso);
    assert_eq!((session.tool(), session.target()), (Tool::Lasso, Target::Entities));
    tap_r(&mut session, true);
    // A box from (4, 4) to (7, 5), half of it past the floor's edge at 6.
    let points = [(4.1, 4.1), (7.9, 4.1), (7.9, 5.9), (4.1, 5.9)];
    lasso(&mut session, &points);
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Lasso of entities"]);
    let ops = &edits[0].2;
    assert_eq!(spawned(ops), [(4, 4), (4, 5), (5, 4), (5, 5)]);
    assert!(ops.iter().all(|op| angle(op) == 90.0), "{ops:?}");

    // A wall gets a tile laid under it where there is none, in the same edit, and a cell that
    // holds one already is skipped.
    session.choose_entity(0);
    lasso(&mut session, &[(2.1, 1.1), (4.9, 1.1), (4.9, 1.9), (2.1, 1.9)]);
    let ops = &asked(&engine)[0];
    assert_eq!(spawned(ops), [(2, 1), (4, 1)]);
    lasso(&mut session, &[(5.1, 5.1), (6.9, 5.1), (6.9, 5.9), (5.1, 5.9)]);
    let ops = &asked(&engine)[0];
    assert_eq!(tiled(ops), [(6, 5)]);
    assert_eq!(spawned(ops), [(5, 5), (6, 5)]);
}

#[test]
fn a_polygon_of_decals_puts_one_on_each_cell_with_a_tile() {
    let (mut session, engine) = with_decals();
    session.choose_decal("Grass");
    session.take_bar_tool(BarTool::Paint);
    session.set_shape(Shape::Polygon);
    polygon(&mut session, &[(4.1, 4.1), (7.9, 4.1), (7.9, 5.9), (4.1, 5.9)]);
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Polygon of decals"]);
    let ops = &edits[0].2;
    assert_eq!(ops.len(), 4, "{ops:?}");
    assert!(ops.iter().all(|op| op["kind"] == "decal" && op["x"].as_f64().unwrap() <= 5.0 && op["y"].as_f64().unwrap() <= 5.0), "{ops:?}");
}

#[test]
fn erase_in_a_lasso_takes_what_can_be_seen_or_with_only_the_kind_in_hand() {
    let around = [(0.1, 0.1), (5.9, 0.1), (5.9, 2.9), (0.1, 2.9)];
    let erasing = || {
        let (mut session, engine) = Session::offline(&scene());
        draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), -2)]);
        session.take_bar_tool(BarTool::Erase);
        session.set_target(Target::Entities);
        session.set_shape(Shape::Lasso);
        (session, engine)
    };
    let deleted = |ops: &[Value]| -> Vec<i64> { ops.iter().map(|op| op["uid"].as_i64().unwrap()).collect() };

    let (mut session, engine) = erasing();
    lasso(&mut session, &around);
    let erased = edits(&engine);
    assert_eq!(labels(&erased), ["Erase entities"]);
    assert_eq!(erased[0].1, None);
    assert_eq!(deleted(&erased[0].2), [CHAIR as i64, WALL as i64]);
    // Its cells were filled red while it was drawn.
    let path_style = {
        let (mut session, _engine) = erasing();
        session.pointer_down(at(&session, around[0]), VIEW, Button::Primary);
        session.pointer_move(at(&session, around[2]), VIEW);
        session.marquee_path().unwrap().style
    };
    assert_eq!(path_style, OutlineStyle::DragTakeAway);

    // A hidden layer keeps what is on it.
    let (mut session, engine) = erasing();
    session.layers.structures = false;
    lasso(&mut session, &around);
    assert_eq!(deleted(&asked(&engine)[0]), [CHAIR as i64]);

    // Only the wall takes the wall alone, and not with its layer hidden.
    let (mut session, engine) = erasing();
    session.set_prototypes(vec![Prototype { id: "WallSolid".into(), name: "wall".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
    session.set_erase_only(true);
    lasso(&mut session, &around);
    assert_eq!(deleted(&asked(&engine)[0]), [WALL as i64]);
    session.layers.structures = false;
    lasso(&mut session, &around);
    assert!(asked(&engine).iter().all(Vec::is_empty));

    // On tiles, Only keeps to the tile in hand, and Outline to the edge.
    let (mut session, engine) = Session::offline(&scene());
    session.take_bar_tool(BarTool::Erase);
    session.set_target(Target::Tiles);
    session.set_shape(Shape::Polygon);
    session.set_shape_fill(ShapeFill::Outline);
    polygon(&mut session, &[(0.1, 0.1), (3.9, 0.1), (3.9, 3.9), (0.1, 3.9)]);
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Erase tiles"]);
    assert!(edits[0].2.iter().all(|op| op["tile"] == "Space"));
    assert_eq!(tiled(&edits[0].2), sorted(edge_cells(&rectangle_cells((0, 0), (3, 3)))));
    session.choose_tile("FloorWood");
    session.take_bar_tool(BarTool::Erase);
    session.set_erase_only(true);
    polygon(&mut session, &[(0.1, 0.1), (3.9, 0.1), (3.9, 3.9), (0.1, 3.9)]);
    assert!(asked(&engine).iter().all(Vec::is_empty));
}

#[test]
fn a_lasso_past_the_limits_is_refused_with_a_line_on_the_status_bar() {
    // 101 by 100 is 10,100 cells, too many for entities and fine for tiles.
    let box_of = |width: f32, height: f32| [(0.1, 0.1), (width - 0.1, 0.1), (width - 0.1, height - 0.1), (0.1, height - 0.1)];
    let (mut session, engine) = super::floors::knowing_walls();
    session.choose_entity(5);
    session.take_bar_tool(BarTool::Paint);
    session.set_shape(Shape::Lasso);
    lasso(&mut session, &box_of(101.0, 100.0));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not placed: the lasso has more than 10,000 cells");
    lasso(&mut session, &box_of(100.0, 100.0));
    assert_eq!(asked(&engine).len(), 1);

    // Erase on entities is refused the same.
    session.take_bar_tool(BarTool::Erase);
    session.set_shape(Shape::Polygon);
    polygon(&mut session, &box_of(101.0, 100.0));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not erased: the polygon has more than 10,000 cells");

    // Tiles go up to 50,000 cells.
    let (mut session, engine) = painting(Shape::Lasso);
    lasso(&mut session, &box_of(101.0, 100.0));
    assert_eq!(tiled(&asked(&engine)[0]).len(), 10_100);
    lasso(&mut session, &box_of(250.0, 201.0));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Not painted: the lasso covers more than 50,000 cells");
}

#[test]
fn the_edge_of_a_shape_is_its_cells_with_a_side_outside_it() {
    // A rectangle's border, and a thin one is all edge.
    assert_eq!(sorted(edge_cells(&rectangle_cells((0, 0), (3, 2)))), [(0, 0), (0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 2), (3, 0), (3, 1), (3, 2)]);
    assert_eq!(edge_cells(&rectangle_cells((0, 0), (0, 4))).len(), 5);
    assert_eq!(edge_cells(&rectangle_cells((2, 2), (3, 3))).len(), 4);
    assert_eq!(edge_cells(&[(7, 7)]), [(7, 7)]);
    assert!(edge_cells(&[]).is_empty());
    // A circle's ring: radius 2 has 13 cells and its middle five are inside.
    let disc = disc_cells((0, 0), 2);
    assert_eq!(disc.len(), 13);
    let ring = sorted(edge_cells(&disc));
    assert_eq!(ring.len(), 8);
    assert!(![(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|cell| ring.contains(cell)));
    // Radius 0 is its one cell.
    assert_eq!(edge_cells(&disc_cells((3, 3), 0)), [(3, 3)]);
    // The triangle of a lasso: of its ten cells only (1, 1) is inside.
    let triangle = polygon_cells(SHAPES[0], 100).unwrap();
    assert_eq!(sorted(edge_cells(&triangle)), sorted(triangle.iter().copied().filter(|&cell| cell != (1, 1))));
    // Every edge cell is one of the shape's, and every other cell has its four sides inside.
    for points in SHAPES {
        let cells = polygon_cells(points, 1000).unwrap();
        let edge = edge_cells(&cells);
        for cell in &cells {
            let inside = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dy)| cells.contains(&(cell.0 + dx, cell.1 + dy)));
            assert_eq!(edge.contains(cell), !inside, "{points:?} {cell:?}");
        }
    }
}

#[test]
fn outline_paints_the_edge_of_each_closed_shape_and_fill_the_whole() {
    // Rectangle from (0, 0) to (3, 3), circle of radius 2 about (2, 2), lasso and polygon of
    // the box round (0, 0) to (3, 3).
    let square = [(0.1, 0.1), (3.9, 0.1), (3.9, 3.9), (0.1, 3.9)];
    let filled_square = rectangle_cells((0, 0), (3, 3));
    let disc = disc_cells((2, 2), 2);
    for fill in ShapeFill::ALL {
        let outline = |cells: &[(i32, i32)]| sorted(if fill == ShapeFill::Outline { edge_cells(cells) } else { cells.to_vec() });
        for shape in [Shape::Rectangle, Shape::Circle, Shape::Lasso, Shape::Polygon] {
            let (mut session, engine) = painting(shape);
            session.set_shape_fill(fill);
            let wanted = match shape {
                Shape::Rectangle => {
                    drag(&mut session, (0, 0), (3, 3));
                    outline(&filled_square)
                }
                Shape::Circle => {
                    drag(&mut session, (2, 2), (4, 2));
                    outline(&disc)
                }
                Shape::Lasso => {
                    lasso(&mut session, &square);
                    outline(&filled_square)
                }
                _ => {
                    polygon(&mut session, &square);
                    outline(&filled_square)
                }
            };
            let edits = edits(&engine);
            assert_eq!(edits.len(), 1, "{shape:?} {fill:?}");
            assert_eq!(tiled(&edits[0].2), wanted, "{shape:?} {fill:?}");
        }
    }
    // Fill is the default, and a line is a line with Outline on.
    assert_eq!(ShapeFill::default(), ShapeFill::Fill);
    let (mut session, engine) = painting(Shape::Line);
    session.set_shape_fill(ShapeFill::Outline);
    drag(&mut session, (0, 0), (3, 0));
    assert_eq!(tiled(&asked(&engine)[0]).len(), 4);
}

#[test]
fn a_rectangle_with_outline_shows_its_border_while_it_is_dragged() {
    let (mut session, _engine) = painting(Shape::Rectangle);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 3)), VIEW);
    // Filled, it shows its outline alone, as it always has.
    let filled = session.overlay_quads().1.len();
    session.pointer_up(over(&session, (3, 3)), VIEW);
    session.set_shape_fill(ShapeFill::Outline);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.pointer_move(over(&session, (3, 3)), VIEW);
    assert_eq!(session.overlay_quads().1.len(), filled + 12 * FLOATS_PER_QUAD);
}

#[test]
fn fill_or_outline_is_kept_for_paint_and_erase_apart_and_read_at_the_press() {
    let (mut session, engine) = painting(Shape::Rectangle);
    session.set_shape_fill(ShapeFill::Outline);
    session.take_bar_tool(BarTool::Erase);
    assert_eq!(session.shape_fill(), Some(ShapeFill::Fill));
    session.take_bar_tool(BarTool::Paint);
    assert_eq!(session.shape_fill(), Some(ShapeFill::Outline));
    // A change during a drag changes nothing of it.
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    session.set_shape_fill(ShapeFill::Fill);
    session.pointer_move(over(&session, (3, 3)), VIEW);
    session.pointer_up(over(&session, (3, 3)), VIEW);
    assert_eq!(tiled(&asked(&engine)[0]).len(), 12);
    // Select and the other tools have none.
    session.take_bar_tool(BarTool::Select);
    assert_eq!(session.shape_fill(), None);
}
