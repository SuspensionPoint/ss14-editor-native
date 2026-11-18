use std::sync::Arc;

use super::*;

fn link(source: i32, source_port: &str, sink: i32, sink_port: &str) -> Link {
    Link { source, source_port: source_port.into(), sink, sink_port: sink_port.into() }
}

fn linked(uid: i32, links: Vec<Link>, devices: Vec<i32>) -> EntityLinks {
    EntityLinks { uid, links, devices }
}

/// A button linked to two doors, and an air alarm with a vent and a scrubber on its list.
fn mirror() -> LinkMirror {
    let answer: LinksAnswer = serde_json::from_str(
        r#"{"links":[{"source":20,"sourcePort":"Pressed","sink":21,"sinkPort":"Toggle"},{"source":20,"sourcePort":"Pressed","sink":22,"sinkPort":"Toggle"}],
            "lists":[{"list":30,"device":31},{"list":30,"device":32}]}"#,
    )
    .unwrap();
    LinkMirror::from_answer(answer)
}

#[test]
fn the_mirror_reads_the_engine_s_answer() {
    let mirror = mirror();
    assert_eq!(mirror.links_from(20), [link(20, "Pressed", 21, "Toggle"), link(20, "Pressed", 22, "Toggle")]);
    assert_eq!(mirror.links_to(22), [&link(20, "Pressed", 22, "Toggle")]);
    assert_eq!((mirror.devices(30), mirror.lists_with(32)), (&[31, 32][..], vec![30]));
    assert!(mirror.links_from(21).is_empty() && mirror.devices(20).is_empty());
    assert_eq!((mirror.all_links().len(), mirror.all_lists().len()), (2, 1));
}

#[test]
fn a_delta_s_linked_entry_replaces_that_entity_s_links_and_list_whole() {
    let mut mirror = mirror();
    // The button loses one door and gains another port to the other.
    mirror.apply(&[], &[linked(20, vec![link(20, "Pressed", 22, "Toggle"), link(20, "Pressed", 22, "Open")], Vec::new())]);
    assert_eq!(mirror.links_from(20), [link(20, "Pressed", 22, "Toggle"), link(20, "Pressed", 22, "Open")]);
    assert!(mirror.links_to(21).is_empty());
    // The alarm's list is untouched, since its entry says nothing of it.
    assert_eq!(mirror.devices(30), [31, 32]);

    // An empty entry means none left.
    mirror.apply(&[], &[linked(20, Vec::new(), Vec::new()), linked(30, Vec::new(), vec![32])]);
    assert!(mirror.links_from(20).is_empty() && mirror.all_links().is_empty());
    assert_eq!(mirror.devices(30), [32]);
}

#[test]
fn a_removed_source_or_list_takes_its_own_with_it() {
    let mut mirror = mirror();
    mirror.apply(&[20, 30], &[]);
    assert_eq!(mirror, LinkMirror::default());
}

#[test]
fn a_removed_sink_goes_by_the_sources_the_engine_names() {
    let mut mirror = mirror();
    // Door 21 and the vent go. The engine takes them off the button and the alarm and names both.
    mirror.apply(&[21, 31], &[linked(20, vec![link(20, "Pressed", 22, "Toggle")], Vec::new()), linked(30, Vec::new(), vec![32])]);
    assert_eq!(mirror.links_from(20), [link(20, "Pressed", 22, "Toggle")]);
    assert_eq!(mirror.devices(30), [32]);

    // The undo brings them back, in the same way.
    mirror.apply(&[], &[linked(20, vec![link(20, "Pressed", 21, "Toggle"), link(20, "Pressed", 22, "Toggle")], Vec::new()), linked(30, Vec::new(), vec![31, 32])]);
    assert_eq!(mirror, self::mirror());
}

#[test]
fn the_game_s_defaults_come_first() {
    let ports = Ports::from_json(
        serde_json::from_str(
            r#"{"sourcePorts":[{"id":"Pressed","name":"n","description":"d","defaults":["Toggle","Trigger"]},{"id":"Status","name":"n","description":"d","defaults":null},
                               {"id":"Timer","name":"n","description":"d","defaults":["Trigger"]}],
                "sinkPorts":[{"id":"Toggle","name":"n","description":"d","defaults":null}],
                "prototypes":[{"id":"SignalButton","sources":["Pressed"],"sinks":[]},{"id":"Airlock","sources":[],"sinks":["Open","Toggle"]}]}"#,
        )
        .unwrap(),
    );
    let names = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
    assert!(ports.is_source("SignalButton") && !ports.is_source("Airlock") && !ports.is_source("Chair"));

    // A door has Open and Toggle. Pressed names Toggle among its defaults, Status names none.
    let sinks = names(&["Open", "Close", "Toggle"]);
    assert_eq!(source_ports_ordered(&ports, &names(&["Status", "Timer", "Pressed"]), &sinks), names(&["Pressed", "Status", "Timer"]));
    assert_eq!(sink_ports_ordered(&ports, Some("Pressed"), &sinks), names(&["Toggle", "Open", "Close"]));
    // With no defaults, the engine's order.
    assert_eq!(sink_ports_ordered(&ports, Some("Status"), &sinks), sinks);
    assert_eq!(sink_ports_ordered(&ports, None, &sinks), sinks);
}

const GRID: i32 = 2;
const ALARM: i32 = 50;

/// A grid of steel from the rows of a picture, top row first, each character a cell from
/// x = 0: `.` floor, space for space, `#` a wall, `D` an airlock, `F` a firelock, `V` a vent,
/// `S` a scrubber, `A` the air alarm, `R` a fire alarm. Each thing stands on steel.
fn room(rows: &[&str]) -> Doc {
    let height = rows.len() as i32;
    let mut tiles = Vec::new();
    let mut entities = Vec::new();
    let mut uid = 100;
    for (row, line) in rows.iter().enumerate() {
        let y = height - 1 - row as i32;
        for (x, mark) in line.chars().enumerate() {
            if mark == ' ' {
                continue;
            }
            tiles.push(format!("{x},{y},1,0,0"));
            let prototype = match mark {
                '#' => "WallSolid",
                'D' => "AirlockGlass",
                'F' => "Firelock",
                'V' => "GasVentPump",
                'S' => "GasVentScrubber",
                'A' => "AirAlarm",
                'R' => "FireAlarm",
                _ => continue,
            };
            let id = if matches!(mark, 'A' | 'R') { ALARM } else { uid };
            uid += 1;
            entities.push(format!(
                r#"{{"uid":{id},"yamlUid":{id},"prototype":"{prototype}","parent":{GRID},"x":{x}.5,"y":{y}.5,"rotation":0,"anchored":true,"contained":false,"name":null}}"#
            ));
        }
    }
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"t.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"g","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[{}]}}],
            "entities":[{}],"history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#,
        tiles.join(","),
        entities.join(",")
    );
    Doc::from_scene(serde_json::from_str(&scene).unwrap(), None)
}

fn prototypes() -> Vec<Prototype> {
    let with = |id: &str, components: &[&str]| Prototype { id: id.into(), components: components.iter().map(|&name| Arc::from(name)).collect(), ..Default::default() };
    vec![with("AirAlarm", &["DeviceList"]), with("FireAlarm", &["DeviceList"]), with("Table", &["Occluder"]), with("Chair", &[])]
}

/// The prototypes of the devices Auto-link Room finds, in the order of their uids.
fn found(doc: &Doc, links: &LinkMirror) -> Result<Vec<String>, String> {
    let prototypes = prototypes();
    let uids = room_devices(doc, links, ALARM, |id| prototypes.iter().find(|prototype| prototype.id == id))?;
    Ok(uids.iter().map(|uid| doc.entities[uid].prototype.clone()).collect())
}

#[test]
fn an_air_alarm_takes_the_vents_of_its_room_and_not_those_behind_a_wall_or_a_door() {
    let doc = room(&[
        "#########",
        "#..V.#.V#",
        "#.S..D..#",
        "#...A#..#",
        "#########",
    ]);
    // The vent is on a row above the scrubber, so it has the lower uid.
    assert_eq!(found(&doc, &LinkMirror::default()), Ok(vec!["GasVentPump".to_owned(), "GasVentScrubber".to_owned()]));
}

#[test]
fn an_alarm_on_the_wall_finds_the_room_beside_it() {
    // The alarm hangs on the wall itself, and the room is below it.
    let mut doc = room(&[
        "#####",
        "#.V.#",
        "#.S.#",
        "#####",
    ]);
    let alarm = format!(r#"{{"uid":{ALARM},"yamlUid":1,"prototype":"AirAlarm","parent":{GRID},"x":2.5,"y":3.5,"rotation":0,"anchored":true,"contained":false,"name":null}}"#);
    doc.entities.insert(ALARM, serde_json::from_str(&alarm).unwrap());
    assert_eq!(found(&doc, &LinkMirror::default()).map(|found| found.len()), Ok(2));
}

#[test]
fn an_open_room_reaches_no_farther_than_eight_steps_and_stops_at_space() {
    // A long hall: the vent ten steps away is out of reach, the one seven away is in.
    let doc = room(&["A......V..V", "           "]);
    let prototypes = prototypes();
    let uids = room_devices(&doc, &LinkMirror::default(), ALARM, |id| prototypes.iter().find(|prototype| prototype.id == id)).unwrap();
    assert_eq!(uids.iter().map(|uid| doc.entities[uid].x).collect::<Vec<_>>(), [7.5]);
    // Space round the hall is its edge, and nothing past it is looked at.
    let room = flood_room(&doc, GRID, (0, 1), |_| None);
    assert!(room.boundary.contains(&(0, 0)) && room.boundary.contains(&(-1, 1)));
    assert_eq!(room.tiles.len(), 9);
}

#[test]
fn a_fire_alarm_takes_the_firelocks_round_its_room() {
    let doc = room(&[
        "###F###",
        "#.....#",
        "F..R..F",
        "#.....#",
        "#######",
        "F      ",
    ]);
    // The three in the room's walls, and not the one far off.
    assert_eq!(found(&doc, &LinkMirror::default()).map(|found| found.len()), Ok(3));
}

#[test]
fn devices_on_the_list_already_are_left_out_and_other_entities_are_refused() {
    let doc = room(&["#####", "#AVS#", "#####"]);
    let vent = doc.entities.values().find(|entity| entity.prototype == "GasVentPump").unwrap().uid;
    let mirror = LinkMirror::from_answer(LinksAnswer { links: Vec::new(), lists: vec![ListEntry { list: ALARM, device: vent }] });
    assert_eq!(found(&doc, &mirror), Ok(vec!["GasVentScrubber".to_owned()]));

    let mut doc = doc;
    doc.entities.get_mut(&ALARM).unwrap().prototype = "Chair".into();
    assert_eq!(found(&doc, &mirror), Err("Chair isn't an air alarm or a fire alarm".into()));
    // An alarm whose prototype keeps no list, and with nothing on one.
    doc.entities.get_mut(&ALARM).unwrap().prototype = "AirAlarmBare".into();
    assert_eq!(found(&doc, &LinkMirror::default()), Err("AirAlarmBare has no device list".into()));
}
