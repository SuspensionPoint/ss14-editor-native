use super::super::tools::tests::{scene, CHAIR, WALL};
use super::*;
use crate::validate::Rule;

/// A session over the test scene, whose prototypes have names and a wall with its tag.
fn session() -> Session {
    let (mut session, _engine) = Session::offline(&scene());
    session.set_prototypes(vec![
        Prototype { id: "Chair".into(), name: "chair".into(), ..Default::default() },
        Prototype { id: "Crowbar".into(), name: "crowbar".into(), ..Default::default() },
        Prototype { id: "WallSolid".into(), name: "solid wall".into(), tags: vec![Arc::from("Wall")], ..Default::default() },
    ]);
    session
}

#[test]
fn the_search_reads_the_names_the_engine_told() {
    let session = session();
    let found: Vec<i32> = session.find_entities("solid").iter().map(|found| found.uid).collect();
    assert_eq!(found, [WALL]);
    // The crowbar is inside the wall, and is found where the wall is.
    let crowbar = session.find_entities("crowbar");
    assert_eq!((crowbar.len(), crowbar[0].world), (1, (3.5, 1.5)));
}

#[test]
fn a_jump_to_a_result_selects_it_and_centres_on_it_close_up() {
    let mut session = session();
    session.set_tool(Tool::Paint);
    session.selection = vec![WALL];
    session.selected_decals = vec![(2, 7)];

    let chair = session.find_entities("chair")[0].uid;
    session.jump_to_entity(chair);
    assert_eq!((session.tool(), &session.selection[..], session.selected_decals.is_empty()), (Tool::Select, &[CHAIR][..], true));
    assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), (1.5, 1.5, JUMP_ZOOM));

    // An entity the mirror does not have changes nothing.
    session.jump_to_entity(999);
    assert_eq!(session.selection, [CHAIR]);
}

#[test]
fn the_session_checks_its_map_with_the_engine_s_facts() {
    // The test scene's wall stands on steel.
    let findings = session().validate();
    assert_eq!(findings.iter().map(|finding| (finding.rule, finding.entity)).collect::<Vec<_>>(), [(Rule::FloorUnderWall, Some(WALL))]);
}

#[test]
fn a_jump_to_a_finding_centres_on_its_cell_and_outlines_it_for_three_seconds() {
    let mut session = session();
    session.camera = Camera { x: 0.0, y: 0.0, zoom: 0.5 };
    let finding = session.validate().remove(0);
    session.jump_to_finding(&finding);

    assert_eq!((session.camera.x, session.camera.y, session.camera.zoom), (3.5, 1.5, JUMP_ZOOM));
    assert!(session.highlight_live());
    let outlines = session.outlines();
    let found: Vec<_> = outlines.iter().filter(|outline| outline.style == OutlineStyle::Found).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].corners, [(3.0, 1.0), (4.0, 1.0), (4.0, 2.0), (3.0, 2.0)]);
    // It selects nothing.
    assert!(session.selection.is_empty());

    // A view already close keeps its zoom.
    session.camera.zoom = 6.0;
    session.jump_to_finding(&finding);
    assert_eq!(session.camera.zoom, 6.0);

    // Three seconds on, the outline is gone.
    session.jump_to_cell(finding.grid, finding.cell, Instant::now() - FOUND_FOR);
    assert!(!session.highlight_live());
    assert!(session.outlines().iter().all(|outline| outline.style != OutlineStyle::Found));
}
