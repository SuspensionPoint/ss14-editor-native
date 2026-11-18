//! The air overlay in the session: the engine's answers kept in step, the rectangles built
//! again only when something they show changes, and the box by the mouse.

use super::*;
use crate::air::{AirAnswer, AirView, GridAirJson, Mix};

/// Station air on two floor cells, a wall with no air and a floor with none.
fn answer(kelvin: f32) -> AirAnswer {
    let air = Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles: vec![21.824879, 82.10312] };
    AirAnswer {
        gases: vec!["Oxygen".into(), "Nitrogen".into()],
        grids: vec![GridAirJson { grid: GRID, mixes: vec![air], tiles: vec![1, 1, 0, 2, 1, 0], blocked: vec![3, 1], unfilled: vec![4, 1], ..Default::default() }],
        map_is_space: true,
    }
}

#[test]
fn the_air_of_a_map_and_of_an_edit_s_grids_is_kept() {
    let (mut session, _engine) = Session::offline(&scene());
    session.air_arrived(1, None, Ok(answer(293.15)));
    let air = session.doc.as_ref().unwrap().air.as_ref().unwrap();
    assert_eq!((air.grids[&GRID].tiles.len(), air.revision), (2, 1));

    // The grid read again after an edit takes the place of what was known.
    session.air_arrived(1, Some(vec![GRID]), Ok(answer(250.0)));
    let air = session.doc.as_ref().unwrap().air.as_ref().unwrap();
    assert_eq!((air.grids[&GRID].mixes[0].temperature, air.revision), (250.0, 2));

    // An answer for a document that is not open, and a failed read after an edit, change nothing.
    session.air_arrived(9, None, Ok(AirAnswer::default()));
    session.air_arrived(1, Some(vec![GRID]), Err("busy".into()));
    assert_eq!(session.doc.as_ref().unwrap().air.as_ref().unwrap().revision, 2);
    // A failed read of a whole map is said.
    session.air_arrived(1, None, Err("Unknown engine command 'air'.".into()));
    assert_eq!(session.status_line(), "The engine couldn't read the air: Unknown engine command 'air'.");
}

#[test]
fn the_overlay_is_built_again_only_when_what_it_shows_changes() {
    let (mut session, _engine) = Session::offline(&scene());
    session.air_arrived(1, None, Ok(answer(293.15)));
    // Off, nothing is drawn.
    let (off, quads) = session.air_overlay(32.0);
    assert!(quads.is_empty());

    session.show_air = true;
    let (on, quads) = session.air_overlay(32.0);
    // Two squares of air, and the taped floor's square with its four stripes. The wall is left.
    assert_eq!((on != off, quads.len() / FLOATS_PER_QUAD), (true, 7));

    // Panning and zooming change nothing of it, while the marks stay as strong.
    session.pan((40.0, 10.0));
    session.zoom(2.0, (100.0, 100.0), VIEW);
    assert_eq!(session.air_overlay(64.0).0, on);
    // Zoomed out far enough that the marks fade, they are built again, and the colours stay.
    let (faded, quads) = session.air_overlay(4.0);
    assert_eq!((faded != on, quads.len() / FLOATS_PER_QUAD), (true, 3));
    assert_eq!(session.air_overlay(5.0).0, faded);

    // Another view, the air read again, and the grid moved each build it again.
    session.air_view = AirView::Pressure;
    let pressure = session.air_overlay(32.0).0;
    assert_ne!(pressure, faded);
    session.air_arrived(1, Some(vec![GRID]), Ok(answer(250.0)));
    let read = session.air_overlay(32.0).0;
    assert_ne!(read, pressure);
    session.doc.as_mut().unwrap().grids[0].x = 5.0;
    let moved = session.air_overlay(32.0);
    assert_ne!(moved.0, read);
    assert_eq!(moved.1[0] % 1.0, 0.0);
    assert!(moved.1.chunks_exact(FLOATS_PER_QUAD).all(|quad| quad[0] >= 5.0));

    // Switched off, the rectangles go.
    session.show_air = false;
    assert!(session.air_overlay(32.0).1.is_empty());
}

#[test]
fn a_room_open_to_space_is_outlined_labelled_and_named_and_validate_finds_it() {
    let (mut session, _engine) = Session::offline(&scene());
    // A room of the grid's floors from (0, 0) to (2, 1), walled but for a gap above (1, 1).
    let mut told = answer(293.15);
    let mut cells = Vec::new();
    for x in -1..=3 {
        for y in -1..=2 {
            let inside = (0..=2).contains(&x) && (0..=1).contains(&y);
            if inside || (x, y) != (1, 2) {
                cells.extend([x, y, if inside { 0 } else { 31 }]);
            }
        }
    }
    told.grids[0].cells = cells;
    session.air_arrived(1, None, Ok(told));
    // The orange rectangles of its dashed outline and its arrow.
    let leaks = |session: &mut Session| {
        let orange = crate::air::LEAK.map(|channel| channel as f32 / 255.0);
        session.air_overlay(32.0).1.chunks_exact(FLOATS_PER_QUAD).filter(|quad| quad[8..11] == orange).count()
    };
    let labels = |session: &mut Session, tile_pixels: f32| session.air_labels(tile_pixels).into_iter().map(|label| label.text).collect::<Vec<_>>();
    // In the Breathable view and the Rooms view, with its label, and not in the scales.
    session.show_air = true;
    assert_eq!(leaks(&mut session), 10 * 2 + 2);
    assert_eq!(labels(&mut session, 32.0), ["Leaking to space"]);
    session.air_view = AirView::Pressure;
    assert_eq!((leaks(&mut session), labels(&mut session, 32.0).len()), (0, 0));
    session.air_view = AirView::Rooms;
    assert_eq!(leaks(&mut session), 10 * 2 + 2);
    // Zoomed out, its label hides.
    assert_eq!(labels(&mut session, 32.0), ["Leaking to space"]);
    assert!(labels(&mut session, 9.0).is_empty());
    session.hover = Some((2.5, 0.5));
    assert_eq!(session.air_hover().unwrap(), ["Room 1, 6 tiles, open to space through 1 tile"]);

    // Validate Map finds it at its leak, and a jump outlines that cell.
    let found: Vec<_> = session.validate().into_iter().filter(|finding| finding.rule == crate::validate::Rule::OpenRoom).collect();
    assert_eq!((found.len(), found[0].grid, found[0].cell), (1, GRID, (1, 1)));
    session.jump_to_finding(&found[0]);
    assert!(session.outlines().iter().any(|outline| outline.style == OutlineStyle::Found && outline.corners[0] == (1.0, 1.0)));

    session.show_air = false;
    assert_eq!(leaks(&mut session), 0);
    assert!(labels(&mut session, 32.0).is_empty());
}

#[test]
fn the_box_by_the_mouse_tells_the_tile_under_it_in_any_view() {
    let (mut session, _engine) = Session::offline(&scene());
    session.air_arrived(1, None, Ok(answer(293.15)));
    session.hover = Some((1.5, 1.5));
    // Only while the overlay is on.
    assert_eq!(session.air_hover(), None);
    assert!(session.air_legend().is_none());

    session.show_air = true;
    for view in [AirView::Breathable, AirView::Pressure, AirView::Temperature] {
        session.air_view = view;
        assert_eq!(session.air_hover().unwrap(), ["Normal air", "101.3 kPa, 20.0 °C", "Nitrogen 79.0%", "Oxygen 21.0%"]);
        assert_eq!(session.air_hover_icon().map(|(icon, _)| icon), Some(crate::air::Icon::Tick));
        assert!(session.air_legend().is_some_and(|legend| legend.special.len() == if view == AirView::Breathable { 3 } else { 2 }));
    }
    // The Rooms view tells the room, and this answer has none.
    session.air_view = AirView::Rooms;
    assert_eq!(session.air_hover(), None);
    assert!(session.air_legend().is_some_and(|legend| legend.title == "Air: rooms"));
    session.air_view = AirView::Breathable;
    session.hover = Some((3.5, 1.5));
    assert_eq!(session.air_hover().unwrap()[0], "Wall");
    session.hover = Some((4.5, 1.5));
    assert_eq!(session.air_hover().unwrap()[0], "Floor with no air");
    // A cell of the floor the file stored nothing for, and one off the grid.
    session.hover = Some((0.5, 4.5));
    assert_eq!(session.air_hover(), None);
    session.hover = Some((-3.5, 1.5));
    assert_eq!(session.air_hover(), None);
}
