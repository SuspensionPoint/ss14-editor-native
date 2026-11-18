use super::tools::tests::scene;
use super::*;

/// An edit of a stroke, by its number, which the engine may pass over for a later one of
/// the stroke or may not.
fn edit(stroke: u64, coalesce: bool, seq: u64) -> ToEngine {
    ToEngine::Edit { ops: Vec::new(), label: "Move", stroke: Some(stroke), coalesce, seq, asked: Instant::now() }
}

/// The number of the edit or the undo an inbox hands on next.
fn next(inbox: &mut Inbox) -> Option<u64> {
    match inbox.next()? {
        ToEngine::Edit { seq, .. } | ToEngine::Undo { seq, .. } => Some(seq),
        _ => panic!("neither an edit nor an undo"),
    }
}

#[test]
fn of_edits_waiting_with_one_stroke_only_the_last_is_sent() {
    let (to_engine, commands) = mpsc::channel();
    let handled = AtomicU64::new(0);
    let mut inbox = Inbox { commands, ahead: VecDeque::new(), handled: &handled };
    let send = |command: ToEngine| to_engine.send(command).ok().expect("an inbox to send to");

    // One place of a move with nothing behind it goes to the engine as it is.
    send(edit(1, true, 1));
    assert_eq!(next(&mut inbox), Some(1));
    // Of two more places of that move waiting, the first is passed over.
    send(edit(1, true, 2));
    send(edit(1, true, 3));
    assert_eq!(next(&mut inbox), Some(3));

    // A place of another move is not passed over for it, nor is an edit that sets
    // something of its own, as each of a stroke of the brush does.
    for command in [edit(2, true, 4), edit(2, true, 5), edit(3, true, 6), edit(4, false, 7), edit(4, false, 8)] {
        send(command);
    }
    // Whatever else is asked for between two places of a move keeps them apart, and
    // nothing changes places.
    for command in [edit(5, true, 9), ToEngine::Undo { seq: 10, asked: Instant::now() }, edit(5, true, 11), edit(5, true, 12)] {
        send(command);
    }
    drop(to_engine);
    let sent: Vec<u64> = std::iter::from_fn(|| next(&mut inbox)).collect();
    assert_eq!(sent, [5, 6, 7, 8, 9, 10, 12]);
}

#[test]
fn the_count_of_what_was_handled_reaches_what_was_sent() {
    let (mut session, commands) = Session::offline(&scene());
    // Three places of one move, an undo and a save. With the map the engine's thread opens
    // without being asked, that is six.
    let stroke = session.stroke();
    for x in [2.0, 2.5, 3.0] {
        session.edit(vec![json!({ "kind": "move", "uid": 10, "x": x, "y": 1.5 })], "Move", Some(stroke), true);
    }
    session.undo();
    session.save_as("map.yml");
    assert_eq!(session.sent, 6);
    assert!(!session.idle());

    // The session lets go of its end, so that the inbox ends when it has handed on all
    // there is.
    let (nowhere, _) = mpsc::channel();
    drop(std::mem::replace(&mut session.to_engine, nowhere));
    let handled = session.handled.clone();
    let mut inbox = Inbox { commands, ahead: VecDeque::from([ToEngine::Open { path: None }]), handled: &handled };
    let mut turns = 0;
    while let Some(_command) = inbox.next() {
        // As the engine's loop counts each of its turns.
        let _finished = Finished(&handled);
        turns += 1;
    }

    // Two places of the move were passed over. They never had a turn of the loop, and are
    // counted all the same.
    assert_eq!((turns, handled.load(Ordering::Acquire)), (4, 6));
    assert!(session.idle());
}

#[test]
fn the_painter_takes_backdrops_in_order_and_one_under_the_mouse_first() {
    let ask = |name: &str, whole: bool| PaintAsk { name: name.into(), whole };
    let mut queue = VecDeque::new();
    for name in ["Default", "Amber", "Water"] {
        queue_ask(&mut queue, ask(name, false));
    }
    // Asked again, nothing moves.
    queue_ask(&mut queue, ask("Amber", false));
    // The mouse over Water: it goes first, whole, and only once.
    queue_ask(&mut queue, ask("Water", true));
    assert_eq!(Vec::from(queue.clone()), [ask("Water", true), ask("Default", false), ask("Amber", false)]);
    // Over one not asked for yet: first again.
    queue_ask(&mut queue, ask("Kettle", true));
    assert_eq!(queue.iter().map(|ask| ask.name.as_str()).collect::<Vec<_>>(), ["Kettle", "Water", "Default", "Amber"]);
}

#[test]
fn one_component_is_set_or_taken_away_and_the_rest_are_kept() {
    let components = vec![json!({ "type": "Map" }), json!({ "type": "Parallax", "parallax": "Water" }), json!({ "type": "Gravity" })];
    let set = with_component(components.clone(), "Parallax", Some(json!({ "type": "Parallax", "parallax": "Kettle" })));
    assert_eq!(set, [json!({ "type": "Map" }), json!({ "type": "Gravity" }), json!({ "type": "Parallax", "parallax": "Kettle" })]);
    assert_eq!(with_component(components, "Parallax", None), [json!({ "type": "Map" }), json!({ "type": "Gravity" })]);
    // A map with none takes one.
    assert_eq!(with_component(vec![json!({ "type": "Map" })], "Parallax", Some(json!({ "type": "Parallax" }))), [json!({ "type": "Map" }), json!({ "type": "Parallax" })]);
}

#[test]
fn a_tile_says_whether_it_can_be_turned_and_an_older_engine_says_nothing() {
    let listed: Vec<TileJson> = serde_json::from_str(
        r#"[{"id":"FloorSteel","variants":4,"sprite":"/Textures/Tiles/steel.png","isSubFloor":false,"baseTurf":"Plating","allowsRotation":false},
            {"id":"FloorSteelSlatsContinuous","variants":4,"sprite":"/Textures/Tiles/steel_slats_continuous.png","isSubFloor":false,"baseTurf":"Plating","allowsRotation":true},
            {"id":"FloorWood","variants":7,"sprite":"/Textures/Tiles/wood.png","isSubFloor":false,"baseTurf":"Plating"}]"#,
    )
    .unwrap();
    let tiles = tiles_at_start(listed);

    // The third is as an engine from before the kernel said it lists a tile: not to be turned.
    let turned: Vec<(&str, bool)> = tiles.iter().map(|tile| (tile.id.as_str(), tile.allows_rotation)).collect();
    assert_eq!(turned, [("FloorSteel", false), ("FloorSteelSlatsContinuous", true), ("FloorWood", false)]);
    assert_eq!(tiles[1].sprite.as_deref(), Some("/Textures/Tiles/steel_slats_continuous.png"));
}

#[test]
fn only_an_engine_that_does_not_know_decals_starts_without_them() {
    assert_eq!(decals_at_start(Err("Unknown engine command 'decals'.".into())), Ok(Vec::new()));

    let arrows = vec![DecalPrototype { id: "Arrows".into(), ..Default::default() }];
    assert_eq!(decals_at_start(Ok(arrows.clone())), Ok(arrows));

    // An answer that does not read as the contract says is a mismatch, and is not hidden.
    let unread = "the engine's answer to 'decals' could not be read: missing field `showMenu`".to_owned();
    assert_eq!(decals_at_start(Err(unread.clone())), Err(unread));
}

#[test]
fn a_hidden_or_abstract_prototype_is_kept_and_is_not_placeable() {
    let listed = vec![
        Prototype { id: "BaseTable".into(), is_abstract: true, ..Default::default() },
        Prototype { id: "MarkerHidden".into(), name: "Marker".into(), hide_spawn_menu: true, ..Default::default() },
        Prototype { id: "Table".into(), name: "table".into(), ..Default::default() },
    ];
    let kept = prototypes_at_start(listed);

    // All three, in the order the engine listed them.
    let placeable: Vec<(&str, bool)> = kept.iter().map(|prototype| (prototype.id.as_str(), prototype.placeable())).collect();
    assert_eq!(placeable, [("BaseTable", false), ("MarkerHidden", false), ("Table", true)]);
    // Each can be searched for, by its id and its name in lower case.
    let search: Vec<&str> = kept.iter().map(|prototype| prototype.search.as_str()).collect();
    assert_eq!(search, ["basetable ", "markerhidden marker", "table table"]);
}

#[test]
fn prototypes_share_the_names_of_their_components() {
    let listed: Vec<Prototype> = serde_json::from_str(
        r#"[{"id":"Chair","name":"chair","suffix":null,"categories":["Furniture"],"abstract":false,"hideSpawnMenu":false,"placementMode":"PlaceFree",
             "components":["Sprite","Strap","Tag"],"tags":["Seat","Structure"]},
            {"id":"Table","name":"table","suffix":null,"categories":["Furniture"],"abstract":false,"hideSpawnMenu":false,"placementMode":"SnapgridCenter",
             "components":["Climbable","Sprite","Tag"],"tags":["Structure","Table"]}]"#,
    )
    .unwrap();
    // As they are read, each prototype has copies of its own.
    assert!(!Arc::ptr_eq(&listed[0].components[0], &listed[1].components[1]));
    let read = listed.clone();

    let shared = prototypes_at_start(listed);
    let (chair, table) = (&shared[0], &shared[1]);

    // Sprite and Tag, the tag Structure and the category are one copy each, held by both.
    assert!(Arc::ptr_eq(&chair.components[0], &table.components[1]) && Arc::ptr_eq(&chair.components[2], &table.components[2]));
    assert!(Arc::ptr_eq(&chair.tags[1], &table.tags[0]) && Arc::ptr_eq(&chair.categories[0], &table.categories[0]));
    // The names themselves are as they were read.
    let names = |prototype: &Prototype| (prototype.categories.clone(), prototype.components.clone(), prototype.tags.clone());
    assert_eq!((names(chair), names(table)), (names(&read[0]), names(&read[1])));
    assert!(table.has("Climbable") && !chair.has("Climbable"));
}
