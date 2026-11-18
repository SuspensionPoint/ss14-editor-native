//! Device links: the mirror the session keeps of them, and the link tool.

use std::sync::mpsc::Sender;

use super::*;
use crate::links::{EntityPorts, LinkMirror, LinksAnswer, Ports};
use crate::model::{EngineTiming, Link, SceneJson};
use crate::session::Timing;

/// A session the engine has opened the test scene in, with the chair linked to the wall by
/// one link and the wall's device list holding the chair.
fn opened() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    let links: LinksAnswer = serde_json::from_str(&format!(
        r#"{{"links":[{{"source":{CHAIR},"sourcePort":"Pressed","sink":{WALL},"sinkPort":"Toggle"}}],"lists":[{{"list":{WALL},"device":{CHAIR}}}]}}"#
    ))
    .unwrap();
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(links) }).ok().expect("a session to send to");
    session.pump(VIEW);
    (session, answers, asked)
}

/// The engine answers an edit, an undo or a redo with a delta that removes these uids and
/// says these entities' links are now these, as the kernel writes `linked`.
fn changed(session: &mut Session, answers: &Sender<FromEngine>, what: &'static str, removed: &[i32], linked: &str) {
    let delta = format!(
        r#"{{"tiles":[],"entities":[],"removed":{removed:?},"gridsChanged":[],"created":[],
            "history":{{"canUndo":true,"canRedo":false,"revision":1,"undoLabel":null,"redoLabel":null}},"linked":[{linked}]}}"#
    );
    let delta = Some(Box::new(serde_json::from_str(&delta).unwrap()));
    answers.send(FromEngine::Changed { list: None, delta, what, seq: 0, asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump(VIEW);
}

fn link(source: i32, source_port: &str, sink: i32, sink_port: &str) -> Link {
    Link { source, source_port: source_port.into(), sink, sink_port: sink_port.into() }
}

/// The links from the chair and the devices on the wall's list, as the mirror has them.
fn mirrored(session: &Session) -> (Vec<Link>, Vec<i32>) {
    let links = session.doc.as_ref().unwrap().links.as_ref().expect("a mirror of the links");
    (links.links_from(CHAIR).to_vec(), links.devices(WALL).to_vec())
}

/// As `opened`, with the link tool in hand, the chair and the wall drawn where they stand, and
/// prototypes that make the chair a source of links and the wall a sink that keeps a device
/// list and has links of its own. The engine's static ports say Pressed links to Toggle.
fn linking() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, asked) = opened();
    let with = |id: &str, components: &[&str]| Prototype { id: id.into(), name: id.to_lowercase(), components: components.iter().map(|&name| Arc::from(name)).collect(), ..Default::default() };
    session.set_prototypes(vec![with("Chair", &["DeviceLinkSource"]), with("Crowbar", &[]), with("WallSolid", &["DeviceLinkSink", "DeviceList", "DeviceLinkSource"])]);
    session.ports = Ports::from_json(
        serde_json::from_str(r#"{"sourcePorts":[{"id":"Pressed","name":"n","description":"d","defaults":["Toggle"]}],"sinkPorts":[],"prototypes":[]}"#).unwrap(),
    );
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0), (WALL, (3.5, 1.5), 0)]);
    session.set_tool(Tool::Link);
    asked.try_iter().count();
    (session, answers, asked)
}

/// What was asked since the last call: the uids of each `ports.of`, and the edits with their numbers.
fn questions(from: &Receiver<ToEngine>) -> (Vec<Vec<i32>>, Vec<(u64, &'static str, Vec<Value>)>) {
    let (mut ports, mut edits) = (Vec::new(), Vec::new());
    for command in from.try_iter() {
        match command {
            ToEngine::PortsOf { uids } => ports.push(uids),
            ToEngine::Edit { ops, label, stroke: None, seq, .. } => edits.push((seq, label, ops)),
            ToEngine::Edit { label, .. } => panic!("{label} has a stroke"),
            _ => {}
        }
    }
    (ports, edits)
}

/// The engine tells the chair's ports and the wall's.
fn tell_ports(session: &mut Session, answers: &Sender<FromEngine>) {
    let names = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
    let ports = vec![
        EntityPorts { uid: CHAIR, sources: names(&["Status", "Pressed"]), range: 30.0, ..Default::default() },
        EntityPorts { uid: WALL, sinks: names(&["Open", "Toggle"]), has_device_list: true, ..Default::default() },
    ];
    answers.send(FromEngine::PortsOf { uids: vec![CHAIR, WALL], ports: Ok(ports) }).ok().expect("a session to send to");
    session.pump(VIEW);
}

#[test]
fn a_press_on_a_source_takes_it_and_asks_its_ports() {
    let (mut session, _answers, asked) = linking();
    click(&mut session, (1.5, 1.5));
    assert_eq!((session.link_source(), &session.selection[..]), (Some((CHAIR, LinkKind::Links)), &[CHAIR][..]));
    assert_eq!(questions(&asked).0, [vec![CHAIR]]);
    assert!(session.status_line().starts_with("Linking from chair"), "{}", session.status_line());
}

#[test]
fn a_link_is_made_with_the_ports_chosen_as_one_step() {
    let (mut session, answers, asked) = linking();
    click(&mut session, (1.5, 1.5));
    click(&mut session, (3.5, 1.5));
    let (ports, edits) = questions(&asked);
    assert_eq!((ports.last().unwrap(), edits.len()), (&vec![CHAIR, WALL], 0));
    assert!(session.link_chooser().unwrap().asking);

    tell_ports(&mut session, &answers);
    let chooser = session.link_chooser().unwrap();
    // The game's defaults first: Pressed links to Toggle.
    assert_eq!((chooser.source_ports, chooser.sink_ports), (vec!["Pressed".to_owned(), "Status".to_owned()], vec!["Toggle".to_owned(), "Open".to_owned()]));
    assert_eq!((chooser.source_port.as_deref(), chooser.sink_port.as_deref(), chooser.asking), (Some("Pressed"), Some("Toggle"), false));
    // The pair is linked already, by the link the engine listed.
    assert_eq!(chooser.existing, [link(CHAIR, "Pressed", WALL, "Toggle")]);

    // Another port of the source: the sink's goes back to the first offered for it.
    session.choose_source_port("Status");
    session.make_link();
    let (_, edits) = questions(&asked);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].1, "Link");
    assert_eq!(edits[0].2, [json!({ "kind": "link", "uid": CHAIR, "port": "Status", "target": WALL, "targetPort": "Open" })]);

    // An unlink of the link there is.
    session.unlink(&link(CHAIR, "Pressed", WALL, "Toggle"));
    let (_, edits) = questions(&asked);
    assert_eq!(edits[0].1, "Unlink");
    assert_eq!(edits[0].2, [json!({ "kind": "unlink", "uid": CHAIR, "port": "Pressed", "target": WALL, "targetPort": "Toggle" })]);
}

#[test]
fn a_refusal_is_shown_whole_and_leaves_the_mirror_as_it_was() {
    let (mut session, answers, asked) = linking();
    click(&mut session, (1.5, 1.5));
    click(&mut session, (3.5, 1.5));
    tell_ports(&mut session, &answers);
    session.make_link();
    let (_, edits) = questions(&asked);
    assert!(session.link_chooser().unwrap().waiting);
    let before = mirrored(&session);

    let error = "The two are 42 tiles apart, past the source's range of 30.\nPorts: Pressed, Status".to_owned();
    answers.send(FromEngine::Refused { seq: edits[0].0, what: "Link", error: error.clone() }).ok().expect("a session to send to");
    session.pump(VIEW);
    let chooser = session.link_chooser().unwrap();
    assert_eq!((chooser.error, chooser.waiting), (Some(error), false));
    assert_eq!(mirrored(&session), before);
    assert!(session.status_line().starts_with("Link: the engine refused: The two are 42 tiles apart"), "{}", session.status_line());

    // The next change takes the refusal away.
    session.make_link();
    assert_eq!(session.link_chooser().unwrap().error, None);
}

#[test]
fn a_list_takes_devices_on_and_off() {
    let (mut session, answers, asked) = linking();
    // The wall keeps a list, which comes before its links.
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.link_source(), Some((WALL, LinkKind::List)));
    // The chair is on its list already: a press offers to take it off, and adds nothing.
    click(&mut session, (1.5, 1.5));
    let chooser = session.link_chooser().unwrap();
    assert!(chooser.listed && chooser.kind == LinkKind::List);
    assert!(questions(&asked).1.is_empty());
    // A secondary press takes it off.
    session.pointer_down(at(&session, (1.5, 1.5)), VIEW, Button::Secondary);
    let (_, edits) = questions(&asked);
    assert_eq!((edits[0].1, &edits[0].2), ("Remove from device list", &vec![json!({ "kind": "list.remove", "uid": WALL, "target": CHAIR })]));

    // Once the engine says it is off, a press puts it on again.
    changed(&mut session, &answers, "Remove from device list", &[], &format!(r#"{{"uid":{WALL},"links":[],"devices":[]}}"#));
    click(&mut session, (1.5, 1.5));
    let (_, edits) = questions(&asked);
    assert_eq!((edits[0].1, &edits[0].2), ("Add to device list", &vec![json!({ "kind": "list.add", "uid": WALL, "target": CHAIR })]));
}

#[test]
fn shift_takes_a_source_for_its_links_where_it_keeps_a_list_too() {
    let (mut session, _answers, _asked) = linking();
    session.held.shift = true;
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.link_source(), Some((WALL, LinkKind::Links)));
}

#[test]
fn the_tool_outlines_what_could_be_a_source_and_then_what_the_source_is_linked_to() {
    let (mut session, _answers, _asked) = linking();
    let styles = |session: &Session| session.outlines().iter().map(|outline| (outline.style, outline.corners[0])).collect::<Vec<_>>();
    assert_eq!(styles(&session), [(OutlineStyle::Linkable, (1.0, 1.0)), (OutlineStyle::Linkable, (3.0, 1.0))]);

    click(&mut session, (1.5, 1.5));
    assert_eq!(styles(&session), [(OutlineStyle::LinkSource, (1.0, 1.0)), (OutlineStyle::Linked, (3.0, 1.0))]);
    // The dashed line goes from the chair to the cursor, orange for links.
    session.pointer_move(at(&session, (2.5, 4.5)), VIEW);
    assert_eq!(session.link_line(), Some(LinkLine { from: (1.5, 1.5), to: (2.5, 4.5), kind: LinkKind::Links }));
}

#[test]
fn linking_ends_on_nothing_with_escape_with_another_tool_and_when_the_source_goes() {
    let (mut session, answers, _asked) = linking();
    click(&mut session, (1.5, 1.5));
    // A press on empty floor ends it and deselects, as in the web editor.
    click(&mut session, (0.5, 4.5));
    assert_eq!((session.link_source(), session.selection.is_empty()), (None, true));

    click(&mut session, (1.5, 1.5));
    session.escape();
    assert_eq!(session.link_source(), None);
    // Escape with linking ended clears the selection as it always does.
    session.escape();
    assert!(session.selection.is_empty());

    click(&mut session, (1.5, 1.5));
    session.set_tool(Tool::Select);
    session.set_tool(Tool::Link);
    assert_eq!(session.link_source(), None);

    click(&mut session, (1.5, 1.5));
    changed(&mut session, &answers, "Delete", &[CHAIR], &format!(r#"{{"uid":{WALL},"links":[],"devices":[]}}"#));
    assert_eq!((session.link_source(), session.link_line()), (None, None));
}

/// The lines of the connections view: each one's colour, its alpha and its width in pixels,
/// rounded to hundredths.
fn lines(session: &mut Session) -> Vec<([f32; 3], f32, f32)> {
    let pixels = 32.0 * session.camera.zoom;
    let round = |value: f32| (value * 100.0).round() / 100.0;
    let over = session.overlay_quads().1;
    over.chunks_exact(FLOATS_PER_QUAD)
        .map(|quad| {
            let width = ((quad[0] - quad[6]).powi(2) + (quad[1] - quad[7]).powi(2)).sqrt() * pixels;
            ([round(quad[8]), round(quad[9]), round(quad[10])], round(quad[11]), round(width))
        })
        .collect()
}

#[test]
fn the_connections_view_draws_each_link_and_list_entry_as_a_line() {
    let (mut session, _answers, _asked) = opened();
    session.set_tool(Tool::Select);
    assert!(lines(&mut session).is_empty());

    session.show_connections = true;
    let (orange, cyan) = ([1.0, 0.53, 0.0], [0.0, 0.8, 0.8]);
    // The chair's link to the wall, orange, and the wall's list entry of the chair, cyan.
    assert_eq!(lines(&mut session), [(orange, 0.4, 1.5), (cyan, 0.4, 1.5)]);
    // The link runs from the middle of the chair along to the middle of the wall.
    let over = session.overlay_quads().1;
    assert_eq!((over[0], over[2]), (1.5, 3.5));

    // Selected, the lines that touch the chair are bright and thick.
    session.selection = vec![CHAIR];
    assert_eq!(lines(&mut session), [([1.0, 0.67, 0.0], 1.0, 3.0), ([0.0, 1.0, 1.0], 1.0, 3.0)]);
    // With another entity selected, they are faint.
    session.selection = vec![CROWBAR];
    assert_eq!(lines(&mut session), [(orange, 0.15, 1.0), (cyan, 0.15, 1.0)]);

    // Zoomed out past four pixels to a tile, nothing is drawn.
    session.camera.zoom = 0.1;
    assert!(lines(&mut session).is_empty());
}

#[test]
fn the_selection_panel_lists_an_entity_s_links_from_either_end() {
    let (session, _answers, _asked) = linking();
    let chair = session.entity_link_rows(CHAIR).unwrap();
    assert_eq!(chair, EntityLinkRows { outgoing: vec![link(CHAIR, "Pressed", WALL, "Toggle")], lists: vec![WALL], ..Default::default() });
    let wall = session.entity_link_rows(WALL).unwrap();
    assert_eq!(wall, EntityLinkRows { incoming: vec![link(CHAIR, "Pressed", WALL, "Toggle")], devices: vec![CHAIR], ..Default::default() });
    assert_eq!(session.entity_label(WALL), "wallsolid");

    // Without a mirror there is nothing to list.
    let (session, _asked) = Session::offline(&scene());
    assert_eq!(session.entity_link_rows(CHAIR), None);
}

const ALARM: i32 = 50;

/// A floor five cells by three, bounded by space, with an air alarm on (0, 1), a vent on
/// (2, 1), a scrubber on (4, 1) and a vent beyond the space on another grid. The alarm is
/// selected, and its list is empty.
fn alarmed() -> (Session, Receiver<ToEngine>) {
    let tiles: Vec<String> = (0..15).map(|i| format!("{},{},1,0,0", i % 5, i / 5)).collect();
    let entity = |uid: i32, prototype: &str, parent: i32, cell: (i32, i32)| {
        format!(r#"{{"uid":{uid},"yamlUid":{uid},"prototype":"{prototype}","parent":{parent},"x":{}.5,"y":{}.5,"rotation":0,"anchored":true,"contained":false,"name":null}}"#, cell.0, cell.1)
    };
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"test.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[{}]}},
                     {{"uid":3,"name":"other","x":20,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0]}}],
            "entities":[{},{},{},{}],
            "history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#,
        tiles.join(","),
        entity(ALARM, "AirAlarm", GRID, (0, 1)),
        entity(51, "GasVentPump", GRID, (2, 1)),
        entity(52, "GasVentScrubber", GRID, (4, 1)),
        entity(53, "GasVentPump", 3, (0, 0)),
    );
    let (mut session, asked) = Session::offline(&scene);
    session.doc.as_mut().unwrap().links = Some(Default::default());
    session.set_prototypes(vec![Prototype { id: "AirAlarm".into(), components: vec![Arc::from("DeviceList")], ..Default::default() }]);
    session.selection = vec![ALARM];
    (session, asked)
}

#[test]
fn auto_link_room_puts_the_room_s_devices_on_the_alarm_s_list_in_one_step() {
    let (mut session, asked) = alarmed();
    assert!(session.entity_link_rows(ALARM).unwrap().alarm);
    session.auto_link_room();
    let (_, edits) = questions(&asked);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].1, "Auto-link room");
    assert_eq!(edits[0].2, [json!({ "kind": "list.add", "uid": ALARM, "target": 51 }), json!({ "kind": "list.add", "uid": ALARM, "target": 52 })]);
    assert_eq!(session.status_line(), "Linking 2 devices");

    // Once both are on the list there is nothing new, and nothing is asked.
    let entries = [crate::links::ListEntry { list: ALARM, device: 51 }, crate::links::ListEntry { list: ALARM, device: 52 }];
    session.doc.as_mut().unwrap().links = Some(LinkMirror::from_answer(LinksAnswer { links: Vec::new(), lists: entries.to_vec() }));
    session.auto_link_room();
    assert!(questions(&asked).1.is_empty());
    assert_eq!(session.status_line(), "No new devices found");

    // On a vent it says why not.
    session.selection = vec![51];
    assert!(!session.entity_link_rows(51).unwrap().alarm);
    session.auto_link_room();
    assert_eq!(session.status_line(), "Not linked: GasVentPump isn't an air alarm or a fire alarm");
}

#[test]
fn the_links_the_engine_listed_on_open_are_mirrored() {
    let (session, _answers, _asked) = opened();
    assert_eq!(mirrored(&session), (vec![link(CHAIR, "Pressed", WALL, "Toggle")], vec![CHAIR]));
}

#[test]
fn a_map_whose_links_the_engine_would_not_list_has_no_mirror() {
    let (to_engine, _asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    let links = Err("Unknown engine command 'links'.".to_owned());
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert!(session.doc.as_ref().unwrap().links.is_none());
    assert!(session.status_line().contains("couldn't list the links"), "{}", session.status_line());
}

#[test]
fn a_link_and_an_unlink_replace_the_source_s_links() {
    let (mut session, answers, _asked) = opened();
    let both = format!(
        r#"{{"uid":{CHAIR},"links":[{{"source":{CHAIR},"sourcePort":"Pressed","sink":{WALL},"sinkPort":"Toggle"}},{{"source":{CHAIR},"sourcePort":"Pressed","sink":{WALL},"sinkPort":"Open"}}],"devices":[]}}"#
    );
    changed(&mut session, &answers, "Link", &[], &both);
    assert_eq!(mirrored(&session).0, [link(CHAIR, "Pressed", WALL, "Toggle"), link(CHAIR, "Pressed", WALL, "Open")]);

    changed(&mut session, &answers, "Unlink", &[], &format!(r#"{{"uid":{CHAIR},"links":[],"devices":[]}}"#));
    assert_eq!(mirrored(&session), (Vec::new(), vec![CHAIR]));

    // The undo of the unlink brings the two back as the engine tells them.
    changed(&mut session, &answers, "Undo", &[], &both);
    assert_eq!(mirrored(&session).0.len(), 2);
}

#[test]
fn a_removed_source_takes_its_links_and_list_with_it() {
    let (mut session, answers, _asked) = opened();
    // The wall goes. It held the crowbar and kept a list. The chair's link to it is taken away
    // and the engine names the chair.
    changed(&mut session, &answers, "Delete", &[CROWBAR, WALL], &format!(r#"{{"uid":{CHAIR},"links":[],"devices":[]}}"#));
    assert_eq!(mirrored(&session), (Vec::new(), Vec::new()));
    assert!(session.doc.as_ref().unwrap().links.as_ref().unwrap().lists_with(CHAIR).is_empty());
}

#[test]
fn a_removed_sink_leaves_the_links_the_engine_names() {
    let (mut session, answers, _asked) = opened();
    // The chair, which is a source and on the wall's list, goes. The engine names the wall,
    // whose list lost it.
    changed(&mut session, &answers, "Delete", &[CHAIR], &format!(r#"{{"uid":{WALL},"links":[],"devices":[]}}"#));
    assert_eq!(mirrored(&session), (Vec::new(), Vec::new()));

    // Undo brings both back.
    let back = format!(
        r#"{{"uid":{CHAIR},"links":[{{"source":{CHAIR},"sourcePort":"Pressed","sink":{WALL},"sinkPort":"Toggle"}}],"devices":[]}},{{"uid":{WALL},"links":[],"devices":[{CHAIR}]}}"#
    );
    changed(&mut session, &answers, "Undo", &[], &back);
    assert_eq!(mirrored(&session), (vec![link(CHAIR, "Pressed", WALL, "Toggle")], vec![CHAIR]));
}

#[test]
fn ports_are_shown_by_their_english_names_with_the_ids_kept_and_by_their_ids_without_them() {
    let (mut session, _answers, _asked) = opened();
    session.ports = Ports::from_json(
        serde_json::from_str(
            r#"{"sourcePorts":[{"id":"Pressed","name":"signal-port-name-pressed","description":"d","defaults":["Toggle"]},{"id":"Status","name":"signal-port-name-status","description":"d","defaults":null}],
                "sinkPorts":[{"id":"DoorBolt","name":"signal-port-name-doorbolt","description":"d","defaults":null},{"id":"Pressed","name":"signal-port-name-pressed-sink","description":"d","defaults":null}],
                "prototypes":[]}"#,
        )
        .unwrap(),
    );
    // Before the fork's files are read, the ids.
    assert_eq!(session.port_name("Pressed", false), None);
    assert_eq!(session.link_ports_text("Pressed", "DoorBolt"), ("Pressed \u{2192} DoorBolt".to_owned(), None));

    let mut text = HashMap::new();
    crate::locale::parse_ftl("signal-port-name-pressed = Pressed\nsignal-port-name-doorbolt = Door bolt\nsignal-port-name-pressed-sink = Push\n", &mut text);
    session.locale = crate::locale::Locale::of(text);
    // A source port and a sink port of the same id have names of their own.
    assert_eq!((session.port_name("Pressed", false), session.port_name("Pressed", true)), (Some("Pressed"), Some("Push")));
    // A name the same as the id needs no note. One with no text, or a port of no key, is its id.
    assert_eq!(session.port_shown("Pressed", false), ("Pressed".to_owned(), None));
    assert_eq!(session.port_shown("DoorBolt", true), ("Door bolt".to_owned(), Some("DoorBolt".to_owned())));
    assert_eq!(session.port_shown("Status", false), ("Status".to_owned(), None));
    assert_eq!(session.port_shown("Unknown", true), ("Unknown".to_owned(), None));
    assert_eq!(session.link_ports_text("Pressed", "DoorBolt"), ("Pressed \u{2192} Door bolt".to_owned(), Some("Pressed \u{2192} DoorBolt".to_owned())));
}
