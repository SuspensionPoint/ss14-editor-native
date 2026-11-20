use std::time::Duration;

use super::tools::tests::{draw, scene, CHAIR, WALL};
use super::*;

#[test]
fn the_session_counts_what_it_shows() {
    let (mut session, _engine) = Session::offline(&scene());
    let numbers = session.performance();

    assert_eq!((numbers.entities, numbers.rectangles), (3, 0));
    assert_eq!(numbers.last_engine_ms, None);
    // The grid's floor is six cells each way.
    assert_eq!(session.grid_size(), Some((6, 6)));

    // With the engine's picture of two of them, one rectangle each.
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    assert_eq!(session.performance().rectangles, 2);
}

#[test]
fn a_change_from_the_engine_brings_its_times() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());

    let asked = Instant::now() - Duration::from_millis(5);
    let timing = Timing { engine: 3.4, ..Default::default() };
    answers.send(FromEngine::Changed { list: None, delta: None, what: "Paint tiles", seq: 0, asked, timing }).ok().expect("a session to send to");
    assert!(session.pump((640, 640)));

    let numbers = session.performance();
    assert_eq!(numbers.last_engine_ms, Some(3.4));
    // From asking to this pump, and it was asked five milliseconds ago.
    assert!(numbers.last_change_ms.is_some_and(|ms| ms >= 5.0), "{:?}", numbers.last_change_ms);
}

#[test]
fn an_edit_answered_says_what_it_did_and_keeps_its_times_for_the_display() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let change = |what: &'static str, seq: u64| FromEngine::Changed { list: None, delta: None, what, seq, asked: Instant::now(), timing: Timing::default() };

    answers.send(change("Paste 12 entities", 0)).ok().expect("a session to send to");
    session.pump((640, 640));
    assert_eq!(session.status_line(), "Pasted 12 entities");
    let breakdown = session.last_breakdown.clone().unwrap_or_default();
    assert!(breakdown.starts_with("Paste 12 entities: ") && breakdown.contains("waiting for the engine's thread"), "{breakdown}");

    // An edit that asked for a message of its own gives it once it is answered.
    session.edit_says = Some((7, "Linked 2 devices".into()));
    answers.send(change("Auto-link room", 7)).ok().expect("a session to send to");
    session.pump((640, 640));
    assert_eq!((session.status_line(), session.edit_says.is_none()), ("Linked 2 devices", true));
}

#[test]
fn an_edit_is_said_in_the_past() {
    let said = ["Paste 3 entities", "Delete", "Paint tiles", "Lay HV cable", "Remove pipes", "Undo", "Cut region", "Auto-link room", "Line of tiles", "T-Ray view changed"].map(edit_message);
    assert_eq!(said, ["Pasted 3 entities", "Deleted", "Painted tiles", "Laid HV cable", "Removed pipes", "Undone", "Cut region", "Auto-linked room", "Line of tiles", "T-Ray view changed"]);
}

#[test]
fn a_change_brings_the_selected_entity_s_components_when_it_changed_them() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let opened: SceneJson = serde_json::from_str(&scene()).unwrap();
    session.doc = Some(Doc::from_scene(opened, None));
    session.selection = vec![WALL];
    session.inspect(Some(WALL));
    session.details = Some((WALL, json!([{ "type": "DeviceList", "devices": [CHAIR] }])));

    // The chair is deleted, and the engine takes it out of the wall's list.
    let delta = format!(
        r#"{{"tiles":[],"entities":[],"removed":[{CHAIR}],"gridsChanged":[],"created":[],
            "history":{{"canUndo":true,"canRedo":false,"revision":1,"undoLabel":"Delete","redoLabel":null}},
            "details":[{{"uid":{CHAIR},"components":"[]"}},{{"uid":{WALL},"components":"[{{\"type\":\"DeviceList\",\"devices\":[]}}]"}}]}}"#
    );
    let delta = Some(Box::new(serde_json::from_str(&delta).unwrap()));
    answers.send(FromEngine::Changed { list: None, delta, what: "Delete", seq: 0, asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump((640, 640));

    assert_eq!(session.details, Some((WALL, json!([{ "type": "DeviceList", "devices": [] }]))));
}

#[test]
fn the_numbers_read_as_the_display_shows_them() {
    let numbers = Performance { entities: 10_256, rectangles: 31_825, last_engine_ms: Some(3.4), last_change_ms: Some(5.2) };
    assert_eq!(
        numbers.lines(Some(4.1)),
        ["frame      4.1 ms", "engine     3.4 ms   last edit, of 5.2 ms from asking to showing", "drawn      31,825 rectangles", "entities   10,256"]
    );

    // Before the first edit there is no time to show, and small numbers have no separator.
    let numbers = Performance { entities: 999, rectangles: 1_234_567, ..Default::default() };
    assert_eq!(numbers.lines(Some(16.66)), ["frame      16.7 ms", "engine     -", "drawn      1,234,567 rectangles", "entities   999"]);

    // Before the toolkit has measured a frame there is no cost to show either.
    assert_eq!(numbers.lines(None)[0], "frame      -");
}

#[test]
fn the_engine_being_ready_fills_the_palettes() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    assert!(session.prototypes().is_empty() && session.decal_prototypes.is_empty());

    let ready = FromEngine::Ready {
        document_kernel_ms: 0,
        view_kernel_ms: 0,
        warm_up_ms: 0,
        tiles: vec![Tile { id: "FloorWood".into(), sprite: None, allows_rotation: false, can_crowbar: false }],
        // One the game keeps out of its spawn menu is kept as well, where the engine listed it.
        prototypes: vec![Prototype { id: "MarkerHidden".into(), hide_spawn_menu: true, ..Default::default() }, Prototype { id: "Table".into(), ..Default::default() }],
        // One the game keeps out of its own menu is kept as well.
        decal_prototypes: vec![DecalPrototype { id: "Arrows".into(), show_menu: true, ..Default::default() }, DecalPrototype { id: "Hidden".into(), ..Default::default() }],
        ports: Ports::default(),
        depths: DepthGroups::default(),
    };
    answers.send(ready).ok().expect("a session to send to");
    assert!(session.pump((640, 640)));

    let ids = |decals: &[DecalPrototype]| decals.iter().map(|decal| decal.id.clone()).collect::<Vec<_>>();
    assert_eq!((session.tiles.len(), ids(&session.decal_prototypes)), (1, vec!["Arrows".to_owned(), "Hidden".to_owned()]));
    let prototypes: Vec<(&str, bool)> = session.prototypes().iter().map(|prototype| (prototype.id.as_str(), prototype.placeable())).collect();
    assert_eq!(prototypes, [("MarkerHidden", false), ("Table", true)]);
}

#[test]
fn the_engine_being_ready_sorts_draw_depths_by_the_fork_s_names() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    // Until the engine tells them, Starlight's: its walls at -2 are Structures.
    assert_eq!(group_of(crate::drawlist::KIND_ENTITY, -2, &session.lists.depths), group::STRUCTURES);

    // Space Wizards draw walls at -8, below their wall tops.
    let named = |name: &str, value: i32| DrawDepth { name: name.into(), value };
    let depths = DepthGroups::from_depths(&[named("BelowFloor", -15), named("Puddles", -12), named("SmallMobs", -9), named("Walls", -8), named("WallTops", -1), named("Objects", 0)]);
    let ready = FromEngine::Ready {
        document_kernel_ms: 0,
        view_kernel_ms: 0,
        warm_up_ms: 0,
        tiles: Vec::new(),
        prototypes: Vec::new(),
        decal_prototypes: Vec::new(),
        ports: Ports::default(),
        depths,
    };
    answers.send(ready).ok().expect("a session to send to");
    assert!(session.pump((640, 640)));
    assert_eq!(group_of(crate::drawlist::KIND_ENTITY, -8, &session.lists.depths), group::STRUCTURES);
    assert_eq!(group_of(crate::drawlist::KIND_ENTITY, -12, &session.lists.depths), group::FLOOR_OBJECTS);
}

#[test]
fn what_the_engine_reports_moves_the_load_on_and_a_map_that_will_not_open_fails_it() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    session.loading = Some(Loading::first_start(Some("Bagel.yml"), session.started));
    let send = |reply: FromEngine| answers.send(reply).ok().expect("a session to send to");

    send(FromEngine::Reached(Step::Runtime));
    send(FromEngine::Reached(Step::Kernels));
    session.pump((640, 640));
    let current = |session: &Session| session.loading.as_ref().and_then(Loading::current_step);
    assert_eq!(current(&session), Some(Step::Prototypes));
    // The interface drawing the old picture is no stage of this load.
    session.map_drawn();
    assert_eq!(current(&session), Some(Step::Prototypes));

    send(FromEngine::Ready { document_kernel_ms: 0, view_kernel_ms: 0, warm_up_ms: 0, tiles: Vec::new(), prototypes: Vec::new(), decal_prototypes: Vec::new(), ports: Ports::default(), depths: DepthGroups::default() });
    send(FromEngine::NotOpened("Could not find file 'Bagel.yml'.\n   at somewhere".into()));
    session.pump((640, 640));
    let loading = session.loading.as_ref().unwrap();
    assert_eq!(loading.failure(), Some((3, "Could not find file 'Bagel.yml'.\n   at somewhere")));
    // The status bar says it as before.
    assert_eq!(session.status_line(), "Not opened: Could not find file 'Bagel.yml'.");

    // Another map asked for starts a load of its own, of the map's stages.
    session.open("Cork.yml");
    assert_eq!(session.loading.as_ref().unwrap().stages()[0].1, "Opening Cork.yml");
}

/// The engine's answers to a map that opened, as its thread sends them, then a frame of
/// the interface drawing it.
fn answer_open(session: &mut Session, answers: &mpsc::Sender<FromEngine>) {
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    answers.send(FromEngine::Reached(Step::Map)).ok().expect("a session to send to");
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) }).ok().expect("a session to send to");
    session.pump((640, 640));
    session.map_drawn();
}

/// The screen of a load that finished, long after: gone, as the interface sees it before it
/// lets the load go.
fn fades_away(session: &Session) -> bool {
    let loading = session.loading.as_ref().expect("a load");
    loading.finished() && loading.frame(Instant::now() + Duration::from_secs(30)).gone
}

/// A session whose first start, with a map, has finished and whose screen the interface
/// has let go.
fn started_with_a_map() -> (Session, mpsc::Sender<FromEngine>, mpsc::Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    session.loading = Some(Loading::first_start(Some("Bagel.yml"), session.started));
    for step in [Step::Runtime, Step::Kernels] {
        answers.send(FromEngine::Reached(step)).ok().expect("a session to send to");
    }
    answers.send(FromEngine::Ready { document_kernel_ms: 0, view_kernel_ms: 0, warm_up_ms: 0, tiles: Vec::new(), prototypes: Vec::new(), decal_prototypes: Vec::new(), ports: Ports::default(), depths: DepthGroups::default() }).ok().expect("a session to send to");
    answer_open(&mut session, &answers);
    assert!(fades_away(&session));
    session.loading_done();
    (session, answers, asked)
}

/// A later load starts with the clown waiting left of an empty row, at full opacity.
fn starts_afresh(session: &Session, first: &str) {
    let loading = session.loading.as_ref().expect("a load of its own");
    assert_eq!(loading.stages().iter().map(|(step, _)| *step).collect::<Vec<_>>(), [Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(loading.stages()[0].1, first);
    assert_eq!(loading.current_step(), Some(Step::Map));
    let frame = loading.frame(Instant::now());
    assert!(frame.tiles.iter().all(Option::is_none));
    assert_eq!((frame.clown.x, frame.clown.opacity, frame.opacity, frame.gone), (crate::loading::CLOWN_START.0, 1.0, 1.0, false));
}

#[test]
fn a_second_open_shows_its_own_load_from_the_start_and_it_ends() {
    let (mut session, answers, _asked) = started_with_a_map();
    session.open("C:/maps/Cork.yml");
    starts_afresh(&session, "Opening Cork.yml");
    answer_open(&mut session, &answers);
    assert!(fades_away(&session));
    session.loading_done();

    // The same map again, and an open while the last screen is still fading.
    session.open("C:/maps/Cork.yml");
    starts_afresh(&session, "Opening Cork.yml");
    answer_open(&mut session, &answers);
    session.open("C:/maps/Bagel.yml");
    starts_afresh(&session, "Opening Bagel.yml");
    answer_open(&mut session, &answers);
    assert!(fades_away(&session));
}

#[test]
fn a_new_map_after_the_start_shows_its_load_and_it_ends() {
    let (mut session, answers, _asked) = started_with_a_map();
    session.new_map();
    starts_afresh(&session, "Starting an empty map");
    answer_open(&mut session, &answers);
    assert!(fades_away(&session));
}

#[test]
fn a_second_open_that_fails_shows_the_failure() {
    let (mut session, answers, _asked) = started_with_a_map();
    session.open("C:/maps/Nowhere.yml");
    starts_afresh(&session, "Opening Nowhere.yml");
    answers.send(FromEngine::NotOpened("Could not find file 'Nowhere.yml'.".into())).ok().expect("a session to send to");
    session.pump((640, 640));
    // The old map is drawn again, which finishes nothing of a load that failed.
    session.map_drawn();
    let loading = session.loading.as_ref().unwrap();
    assert_eq!(loading.failure(), Some((0, "Could not find file 'Nowhere.yml'.")));
    assert_eq!(loading.stage_line().0, "Opening Nowhere.yml failed.");
    assert!(!loading.frame(Instant::now() + Duration::from_secs(30)).gone);
}

#[test]
fn the_session_finds_a_prototype_by_its_id() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    assert_eq!(session.prototype("Table"), None);

    let named = |id: &str, name: &str, hide_spawn_menu: bool| Prototype { id: id.into(), name: name.into(), hide_spawn_menu, ..Default::default() };
    let ready = FromEngine::Ready {
        document_kernel_ms: 0,
        view_kernel_ms: 0,
        warm_up_ms: 0,
        tiles: Vec::new(),
        prototypes: vec![named("Chair", "chair", false), named("MarkerHidden", "marker", true), named("Table", "table", false)],
        decal_prototypes: Vec::new(),
        ports: Ports::default(),
        depths: DepthGroups::default(),
    };
    answers.send(ready).ok().expect("a session to send to");
    assert!(session.pump((640, 640)));

    let name = |id: &str| session.prototype(id).map(|prototype| prototype.name.as_str());
    assert_eq!((name("Chair"), name("Table")), (Some("chair"), Some("table")));
    // One hidden from the spawn menu is found as any other.
    assert_eq!(name("MarkerHidden"), Some("marker"));
    // An id is looked up whole, and letter for letter.
    assert_eq!((name("Tab"), name("table"), name("")), (None, None, None));
    // What is found is the one in the list.
    assert!(std::ptr::eq(session.prototype("Table").unwrap(), &session.prototypes()[2]));
}

#[test]
fn the_palette_offers_only_what_can_be_placed_and_matches_every_word() {
    let (mut session, _engine) = Session::offline(&scene());
    // Searched by the id and the name in lower case, as the engine's list is.
    let prototype = |id: &str, name: &str, hide_spawn_menu: bool| {
        let search = format!("{id} {name}").to_lowercase();
        Prototype { id: id.into(), name: name.into(), hide_spawn_menu, search, ..Default::default() }
    };
    session.set_prototypes(vec![
        prototype("MarkerWall", "wall marker", true),
        prototype("Table", "table", false),
        prototype("WallReinforced", "reinforced wall", false),
        prototype("WallSolid", "solid wall", false),
    ]);

    // No words is everything that can be placed, in the list's order.
    assert_eq!(session.placeable_matching(&[]), [1, 2, 3]);
    assert_eq!(session.placeable_matching(&["wall"]), [2, 3]);
    // Every word has to match, in either case, and the id counts as well as the name.
    assert_eq!(session.placeable_matching(&["wall", "Reinforced"]), [2]);
    assert_eq!(session.placeable_matching(&["wallsolid"]), [3]);
    assert_eq!(session.placeable_matching(&["wall", "table"]), Vec::<usize>::new());
}
