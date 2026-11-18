use std::sync::Arc;

use super::*;
use crate::air::{AirAnswer, AirMirror, GridAirJson, Mix};
use crate::links::{LinksAnswer, ListEntry};
use crate::model::Link;

const GRID: i32 = 2;

/// An entity as the kernel writes one, standing on the grid in the middle of a cell.
fn entity(uid: i32, prototype: &str, cell: (i32, i32)) -> String {
    format!(
        r#"{{"uid":{uid},"yamlUid":{uid},"prototype":"{prototype}","parent":{GRID},"x":{}.5,"y":{}.5,"rotation":0,"anchored":true,"contained":false,"name":null}}"#,
        cell.0, cell.1
    )
}

/// A grid of steel floor three cells by three, with plating on (0, 0) and lattice on
/// (1, 0). Cell (5, 5) is space. On it these entities and decals.
fn doc(entities: &[String], decals: &[String]) -> Doc {
    let mut tiles = vec!["0,0,2,0,0".to_owned(), "1,0,3,0,0".to_owned()];
    tiles.extend((0..9).filter(|i| i / 3 > 0).map(|i| format!("{},{},1,0,0", i % 3, i / 3)));
    tiles.push("2,0,1,0,0".to_owned());
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"t.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"g","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel","Plating","Lattice"],"tiles":[{}],"decals":[{}]}}],
            "entities":[{}],"history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#,
        tiles.join(","),
        decals.join(","),
        entities.join(",")
    );
    Doc::from_scene(serde_json::from_str(&scene).unwrap(), None)
}

/// A decal as the kernel writes one, its corner on a cell's.
fn decal(id: i32, prototype: &str, cell: (i32, i32)) -> String {
    format!(r#"{{"id":{id},"prototype":"{prototype}","x":{},"y":{},"color":null,"angle":0,"zIndex":0,"cleanable":false}}"#, cell.0, cell.1)
}

/// The fork's prototypes, as far as the rules ask: walls and a diagonal wall by their tags.
fn prototypes() -> Vec<Prototype> {
    let tagged = |id: &str, tags: &[&str]| Prototype { id: id.into(), tags: tags.iter().map(|&tag| Arc::from(tag)).collect(), ..Default::default() };
    vec![
        tagged("WallSolid", &["Wall"]),
        tagged("WallSolidDiagonal", &["Wall", "Diagonal"]),
        tagged("AsteroidRock", &["Wall"]),
        tagged("Table", &[]),
        tagged("AirlockGlass", &[]),
        // Two alarms that keep device lists, and one that keeps none.
        Prototype { id: "AirAlarm".into(), components: vec![Arc::from("DeviceList")], ..Default::default() },
        Prototype { id: "FireAlarm".into(), components: vec![Arc::from("DeviceList")], ..Default::default() },
        Prototype { id: "AirAlarmFrame".into(), ..Default::default() },
        // Disposal tubes with the fork's exits, a router and a bin.
        tube("DisposalPipe", &[0, 4], &[]),
        tube("DisposalBend", &[0, 6], &[]),
        tube("DisposalTrunk", &[0], &["DisposalEntry"]),
        tube("DisposalJunction", &[0, 6, 4], &[]),
        tube("DisposalRouter", &[6, 0, 4], &["DisposalRouter"]),
        Prototype { id: "DisposalUnit".into(), components: vec![Arc::from("DisposalUnit")], ..Default::default() },
        Prototype { id: "ConveyorBelt".into(), components: vec![Arc::from("Conveyor")], ..Default::default() },
    ]
}

fn tube(id: &str, exits: &[u8], components: &[&str]) -> Prototype {
    let mut names: Vec<Arc<str>> = components.iter().map(|&name| Arc::from(name)).collect();
    names.push(Arc::from("DisposalTube"));
    Prototype { id: id.into(), components: names, tube_exits: Some(exits.to_vec()), ..Default::default() }
}

/// An entity on a cell turned by this many quarter turns.
fn turned_entity(uid: i32, prototype: &str, cell: (i32, i32), quarters: i32) -> String {
    entity(uid, prototype, cell).replace(r#""rotation":0"#, &format!(r#""rotation":{}"#, quarters as f64 * std::f64::consts::FRAC_PI_2))
}

fn disposals(doc: &Doc) -> Vec<(i32, String)> {
    check(doc).into_iter().filter(|finding| finding.rule == Rule::OpenDisposals).map(|finding| (finding.entity.unwrap(), finding.message)).collect()
}

#[test]
fn a_disposal_line_from_bin_to_bin_has_no_open_end() {
    // A bin on a trunk at the top, a pipe, and a trunk turned upside down under a bin.
    let line = vec![
        entity(30, "DisposalUnit", (0, 2)),
        entity(31, "DisposalTrunk", (0, 2)),
        entity(32, "DisposalPipe", (0, 1)),
        turned_entity(33, "DisposalTrunk", (0, 0), 2),
        entity(34, "DisposalUnit", (0, 0)),
    ];
    assert_eq!(disposals(&doc(&line, &[])), []);

    // With the lower bin gone, trash comes down to its trunk and is dumped.
    let no_bin = &line[..4];
    // It is on plating, which a crowbar does not lift.
    assert_eq!(disposals(&doc(no_bin, &[])), [(33, "DisposalTrunk at (0, 0) has no disposal unit on it, so trash that reaches it gets dumped there.".to_owned())]);

    // On a conveyor it is a station's outlet to the recycler.
    let outlet: Vec<String> = no_bin.iter().cloned().chain([entity(35, "ConveyorBelt", (0, 0))]).collect();
    assert_eq!(disposals(&doc(&outlet, &[])), []);

    // With the trunk gone as well, the pipe is open below.
    let no_trunk = &line[..3];
    assert_eq!(disposals(&doc(no_trunk, &[])), [(32, "DisposalPipe at (0, 1) is open to the south, so trash leaving that way gets dumped there and tears up the floor.".to_owned())]);
}

#[test]
fn a_junction_s_middle_side_may_be_open_and_a_router_s_may_not() {
    let junction = disposals(&doc(&[entity(40, "DisposalJunction", (1, 1))], &[]));
    assert_eq!(junction, [(40, "DisposalJunction at (1, 1) is open to the south and north, so trash leaving that way gets dumped there and tears up the floor.".to_owned())]);
    let router = disposals(&doc(&[entity(41, "DisposalRouter", (1, 1))], &[]));
    assert_eq!(router, [(41, "DisposalRouter at (1, 1) is open to the west, south and north, so trash leaving that way gets dumped there and tears up the floor.".to_owned())]);
}

#[test]
fn turned_tubes_join_by_their_turned_exits() {
    // A bend turned a quarter has exits east and south; a pipe turned a quarter, east and west.
    let found = disposals(&doc(&[turned_entity(50, "DisposalBend", (1, 1), 1), turned_entity(51, "DisposalPipe", (2, 1), 1)], &[]));
    assert_eq!(
        found,
        [
            (50, "DisposalBend at (1, 1) is open to the south, so trash leaving that way gets dumped there and tears up the floor.".to_owned()),
            (51, "DisposalPipe at (2, 1) is open to the east, so trash leaving that way gets dumped there and tears up the floor.".to_owned()),
        ]
    );
}

#[test]
fn an_open_end_over_space_throws_trash_out() {
    let found = disposals(&doc(&[entity(60, "DisposalTrunk", (5, 5))], &[]));
    assert_eq!(found, [(60, "DisposalTrunk at (5, 5) is open to the south, so trash leaving that way gets thrown out into space.".to_owned())]);
}

/// The doc with these links and list entries, as the engine listed them.
fn linked(mut doc: Doc, links: &[(i32, i32)], lists: &[(i32, i32)]) -> Doc {
    let links = links.iter().map(|&(source, sink)| Link { source, source_port: "Pressed".into(), sink, sink_port: "Toggle".into() }).collect();
    let lists = lists.iter().map(|&(list, device)| ListEntry { list, device }).collect();
    doc.links = Some(LinkMirror::from_answer(LinksAnswer { links, lists }));
    doc
}

/// A button, a door, an air alarm with a vent and a fire alarm with a firelock, all linked.
fn devices() -> Vec<String> {
    vec![
        entity(20, "SignalButton", (0, 1)),
        entity(21, "AirlockGlass", (1, 1)),
        entity(22, "AirAlarm", (2, 1)),
        entity(23, "GasVentPump", (2, 2)),
        entity(24, "FireAlarm", (0, 2)),
        entity(25, "Firelock", (1, 2)),
    ]
}

#[test]
fn a_map_with_every_link_whole_has_no_findings_of_links() {
    let doc = linked(doc(&devices(), &[]), &[(20, 21)], &[(22, 23), (24, 25)]);
    assert!(check(&doc).is_empty(), "{:?}", check(&doc));
}

#[test]
fn a_link_or_a_list_entry_of_nothing_is_an_error() {
    // The button links to 99, the air alarm lists 98, and a list that is gone, 97, lists the vent.
    let doc = linked(doc(&devices(), &[]), &[(20, 21), (20, 99)], &[(22, 23), (22, 98), (24, 25), (97, 23)]);
    let findings = check(&doc);
    assert_eq!(
        places(&findings),
        [(Rule::DanglingLink, (0, 1), Some(20), None), (Rule::DanglingLink, (2, 1), Some(22), None), (Rule::DanglingLink, (2, 2), Some(23), None)]
    );
    assert_eq!(findings[0].message, "SignalButton (UID 20) points to UID 99 in DeviceLinkSource, but no entity has that UID.");
    assert_eq!(findings[1].message, "AirAlarm (UID 22) points to UID 98 in DeviceList, but no entity has that UID.");
    assert_eq!(findings[2].message, "GasVentPump (UID 23) points to UID 97 in DeviceNetwork, but no entity has that UID.");
    assert_eq!(Rule::DanglingLink.severity(), Severity::Error);
}

#[test]
fn an_air_alarm_with_no_devices_is_found() {
    let mut entities = devices();
    // One that keeps no list is no fault.
    entities.push(entity(26, "AirAlarmFrame", (2, 0)));
    let doc = linked(doc(&entities, &[]), &[(20, 21)], &[(24, 25)]);
    let findings = check(&doc);
    assert_eq!(places(&findings), [(Rule::UnlinkedAirAlarm, (2, 1), Some(22), None)]);
    assert_eq!(findings[0].message, "AirAlarm at (2, 1) has no linked devices.");
}

#[test]
fn a_fire_alarm_with_no_devices_is_found() {
    let doc = linked(doc(&devices(), &[]), &[(20, 21)], &[(22, 23)]);
    let findings = check(&doc);
    assert_eq!(places(&findings), [(Rule::UnlinkedFireAlarm, (0, 2), Some(24), None)]);
    assert_eq!(findings[0].message, "FireAlarm at (0, 2) has no linked devices.");
}

#[test]
fn the_rules_of_links_wait_for_the_engine_s_list_of_them() {
    // No list of links: an alarm with nothing on its list is not found.
    let doc = doc(&devices(), &[]);
    assert!(doc.links.is_none() && check(&doc).is_empty());
}

fn check(doc: &Doc) -> Vec<Finding> {
    let prototypes = prototypes();
    let known = HashSet::from(["WarnLineN", "Arrows"]);
    // Steel floor is lifted by a crowbar, plating and lattice are not.
    validate(doc, |id| prototypes.iter().find(|prototype| prototype.id == id), &known, &HashSet::from(["FloorSteel"]))
}

fn places(findings: &[Finding]) -> Vec<(Rule, (i32, i32), Option<i32>, Option<i32>)> {
    findings.iter().map(|finding| (finding.rule, finding.cell, finding.entity, finding.decal)).collect()
}

#[test]
fn a_clean_map_has_no_findings() {
    let doc = doc(
        &[
            // A wall on plating and one on lattice, a diagonal wall and a rock on steel.
            entity(10, "WallSolid", (0, 0)),
            entity(11, "WallSolid", (1, 0)),
            entity(12, "WallSolidDiagonal", (1, 1)),
            entity(13, "AsteroidRock", (2, 2)),
            // A door on steel, and a table on space, which is no fault.
            entity(14, "AirlockGlass", (0, 1)),
            entity(15, "Table", (5, 5)),
        ],
        &[decal(1, "WarnLineN", (0, 1)), decal(2, "Arrows", (2, 2))],
    );
    assert!(check(&doc).is_empty(), "{:?}", check(&doc));
    assert!(grouped(&check(&doc)).is_empty());
}

#[test]
fn a_wall_on_a_floor_is_found_and_one_on_space_is_not() {
    let doc = doc(&[entity(10, "WallSolid", (1, 2)), entity(11, "WallSolid", (5, 5))], &[]);
    let findings = check(&doc);
    assert_eq!(places(&findings), [(Rule::FloorUnderWall, (1, 2), Some(10), None)]);
    assert_eq!(findings[0].message, "Floor tile (FloorSteel) under wall at (1, 2). Walls should be on Plating.");
    assert_eq!(findings[0].grid, GRID);
}

#[test]
fn a_wall_of_a_prototype_the_engine_does_not_know_is_no_wall() {
    let doc = doc(&[entity(10, "WallMystery", (1, 2))], &[]);
    assert!(check(&doc).is_empty());
}

#[test]
fn a_door_on_space_or_lattice_is_found() {
    let doc = doc(&[entity(10, "AirlockGlass", (5, 5)), entity(11, "Firelock", (1, 0)), entity(12, "AirlockGlass", (2, 1))], &[]);
    let findings = check(&doc);
    assert_eq!(places(&findings), [(Rule::DoorWithoutFloor, (5, 5), Some(10), None), (Rule::DoorWithoutFloor, (1, 0), Some(11), None)]);
    assert_eq!(findings[0].message, "Door (AirlockGlass) has no floor tile at (5, 5).");
}

#[test]
fn what_is_inside_something_is_not_checked() {
    let mut doc = doc(&[entity(10, "WallSolid", (1, 2))], &[]);
    doc.entities.get_mut(&10).unwrap().contained = true;
    assert!(check(&doc).is_empty());
}

#[test]
fn a_decal_of_no_known_prototype_is_found_and_one_that_names_none() {
    let doc = doc(&[], &[decal(4, "Gone", (1, 1)), decal(2, "Arrows", (0, 0)), decal(3, "", (2, 2))]);
    let findings = check(&doc);
    assert_eq!(places(&findings), [(Rule::UnknownDecal, (2, 2), None, Some(3)), (Rule::UnknownDecal, (1, 1), None, Some(4))]);
    assert_eq!(findings[0].message, "Decal 3 at (2, 2) has no prototype.");
    assert_eq!(findings[1].message, "Decal 4 (Gone) at (1, 1) isn't a decal this fork has.");
}

/// The doc with the engine's air of its grid: what blocks air on each tile and the air it
/// stores, drawn in text with the top row first. `.` floor of station air, `v` floor of
/// vacuum, `c` floor of cold air, `n` floor with no air, `#` wall, `W` wall that stores
/// air, `_` space or lattice, ` ` no tile.
fn aired(mut doc: Doc, rows: &[&str], map_is_space: bool) -> Doc {
    let (mut cells, mut tiles) = (Vec::new(), Vec::new());
    for (row, line) in rows.iter().enumerate() {
        for (x, letter) in line.chars().enumerate() {
            let (flags, mix) = match letter {
                '.' => (0, Some(0)),
                'v' => (0, Some(1)),
                'c' => (0, Some(2)),
                'n' => (0, None),
                '#' => (31, None),
                'W' => (31, Some(0)),
                '_' => (32, None),
                _ => continue,
            };
            let cell = [x as i32 + 10, (rows.len() - 1 - row) as i32];
            cells.extend([cell[0], cell[1], flags]);
            if let Some(mix) = mix {
                tiles.extend([cell[0], cell[1], mix]);
            }
        }
    }
    let mix = |kelvin: f32, moles: Vec<f32>| Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles };
    let mixes = vec![mix(293.15, vec![21.824879, 82.10312]), mix(293.15, vec![0.0, 0.0]), mix(250.0, vec![25.6, 96.3])];
    let gases = vec!["Oxygen".to_owned(), "Nitrogen".to_owned()];
    let answer = AirAnswer { gases, grids: vec![GridAirJson { grid: GRID, cells, tiles, mixes, ..Default::default() }], map_is_space };
    doc.air = Some(AirMirror::from_answer(answer));
    doc
}

#[test]
fn a_room_open_to_space_is_found_at_its_first_leak() {
    // A room of six tiles with a gap above and lattice at its left, beside a room of three
    // tiles open to space, then a sealed room, and a walkway outside along a hull.
    let rows = ["#_######", "#...#..._", "_...####", "#####", "####", "#..#", "####", "", "__________", "..........", "##########"];
    let station = aired(doc(&[], &[]), &rows, true);
    let findings = check(&station);
    assert_eq!(places(&findings), [(Rule::OpenRoom, (11, 8), None, None)]);
    // The walkway is room 1 and the sealed room 2.
    assert_eq!(findings[0].message, "Room 3, 6 tiles, open to space through 2 tiles. The first leak is at (11, 8).");
    assert_eq!(findings[0].grid, GRID);
    assert_eq!(Rule::OpenRoom.severity(), Severity::Warning);

    // On a map whose atmosphere is air, it is said so.
    let cork = aired(doc(&[], &[]), &rows, false);
    assert_eq!(check(&cork)[0].message, "Room 3, 6 tiles, open to the map's atmosphere through 2 tiles. The first leak is at (11, 8).");
    // Before the engine has read the air, nothing.
    assert!(check(&doc(&[], &[])).is_empty());
}

#[test]
fn the_air_of_sealed_rooms_and_walls_is_checked() {
    // A room with a floor of no air and a vacuum tile, a room with a cold tile beside it,
    // a wall between them that stores air, and a closet below with no air at all.
    let rows = ["##########", "#...v.#..#", "#n....#.c#", "#######W##", "#nn#", "####"];
    let station = aired(doc(&[], &[]), &rows, true);
    let findings = check(&station);
    assert_eq!(
        places(&findings),
        [
            (Rule::UnfilledFloor, (11, 1), None, None),
            (Rule::UnfilledFloor, (11, 3), None, None),
            (Rule::UnevenAir, (14, 4), None, None),
            (Rule::UnevenAir, (18, 3), None, None),
            (Rule::AirInWall, (17, 2), None, None),
        ]
    );
    let messages: Vec<&str> = findings.iter().map(|finding| finding.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "Room 1 (2 tiles) has no air at all, so the game will start it as vacuum. Run Fix grid atmos to fill it. Its first tile is at (11, 1).",
            "Room 2: 1 of 10 tiles stores no air. When the round starts the game makes them vacuum, and the room's air spreads thin into them. Run Fix grid atmos to fill them. The first one is at (11, 3).",
            "Room 2: 1 of 10 tiles differs (vacuum). The first one is at (14, 4).",
            "Room 3: 1 of 4 tiles differs (colder). The first one is at (18, 3).",
            "1 tile fully blocked by a wall still stores air. The game clears it when the round starts. The first one is at (17, 2).",
        ]
    );
    assert_eq!(findings[0].grid, GRID);
    assert_eq!((Rule::UnfilledFloor.severity(), Rule::UnevenAir.severity(), Rule::AirInWall.severity()), (Severity::Warning, Severity::Warning, Severity::Notice));
    let groups: Vec<(Rule, usize)> = grouped(&findings).iter().map(|(rule, found)| (*rule, found.len())).collect();
    assert_eq!(groups, [(Rule::UnfilledFloor, 2), (Rule::UnevenAir, 2), (Rule::AirInWall, 1)]);

    // A room open to space is left to its own rule, whatever air it stores.
    let open = aired(doc(&[], &[]), &["##_###", "#.v.n#", "######"], true);
    assert_eq!(places(&check(&open)), [(Rule::OpenRoom, (12, 1), None, None)]);
}

#[test]
fn a_grid_of_floors_with_no_atmosphere_is_found_and_a_planet_s_ground_is_not() {
    let with = |atmosphere: bool, addable: bool| {
        let mut doc = doc(&[], &[]);
        let grid = GridAirJson { grid: GRID, atmosphere: Some(atmosphere), addable: Some(addable), ..Default::default() };
        doc.air = Some(AirMirror::from_answer(AirAnswer { gases: Vec::new(), grids: vec![grid], map_is_space: true }));
        doc
    };
    // Eight floors and lattice: found at the first floor of the lowest row, with the Fix of the air.
    let findings = check(&with(false, true));
    assert_eq!(places(&findings), [(Rule::NoAtmosphere, (0, 0), None, None)]);
    assert_eq!(
        findings[0].message,
        "This grid has no atmosphere, so its 8 floor tiles store no air. Fix adds one, the way the game's atmos:add does, and fills its floors with station air."
    );
    assert_eq!((findings[0].grid, findings[0].fixes_air(), Rule::NoAtmosphere.severity()), (GRID, Some(GRID), Severity::Warning));
    // A planet's ground cannot be given one, and a grid that has one is no fault.
    assert!(check(&with(false, false)).is_empty() && check(&with(true, false)).is_empty());
}

/// The air rules and open disposal ends on the fork's stations, through the engine published in
/// src/Editor.Native/bin/r2r (or the folder in EDITOR_ENGINE). It prints what they find.
/// Not run with the others: it starts the engine, which takes a minute and several GB.
/// `cargo test -p editor-core --release stations -- --ignored --nocapture`
#[test]
#[ignore]
fn stations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let folder = std::env::var_os("EDITOR_ENGINE").map(std::path::PathBuf::from).unwrap_or_else(|| root.join("src/Editor.Native/bin/r2r"));
    let engine = crate::engine::Engine::load(&folder).unwrap();
    let path = |inside: &str| root.join(inside).to_string_lossy().replace('\\', "/");
    engine.start(&path("upstream/starlight"), Some(&path("native/cache/kernel-resources.zip")));
    engine.wait_ready().unwrap();
    let prototypes: Vec<Prototype> = engine.call("prototypes", &serde_json::json!({})).unwrap();
    let prototype = |id: &str| prototypes.iter().find(|prototype| prototype.id == id);
    let tiles: Vec<serde_json::Value> = engine.call("tiles", &serde_json::json!({})).unwrap();
    let pryable: HashSet<&str> = tiles.iter().filter(|tile| tile["canCrowbar"] == true).filter_map(|tile| tile["id"].as_str()).collect();
    for map in ["Bagel", "Cork", "Starboard"] {
        let opened: serde_json::Value = engine.call("open", &serde_json::json!({ "path": path(&format!("upstream/starlight/Resources/Maps/_Starlight/Stations/{map}.yml")) })).unwrap();
        let mut doc = Doc::from_scene(serde_json::from_value(opened["scene"].clone()).unwrap(), None);
        let begun = std::time::Instant::now();
        let mut answer: AirAnswer = engine.call("air", &serde_json::json!({ "doc": doc.id })).unwrap();
        answer.grids.iter_mut().for_each(GridAirJson::find_rooms);
        let found_in = begun.elapsed();
        doc.air = Some(AirMirror::from_answer(answer));
        let mirror = doc.air.as_ref().unwrap();
        let sealed: usize = mirror.grids.values().map(|air| air.rooms.rooms.iter().filter(|room| !room.is_open()).count()).sum();
        println!("\n{map}: {} grids, {sealed} sealed rooms, read and checked in {found_in:?}", mirror.grids.len());
        let disposals = open_disposals(&doc, prototype, &pryable);
        for finding in open_rooms(&doc).iter().chain(&unfilled_floors(&doc)).chain(&uneven_air(&doc)).chain(&air_in_walls(&doc)).chain(&disposals) {
            println!("  {:?} on grid {}: {}", finding.rule, finding.grid, finding.message);
            // What stands on the cell, to judge it by.
            let standing = doc.entities.values().filter(|entity| entity.parent == finding.grid && (entity.x.floor() as i32, entity.y.floor() as i32) == finding.cell);
            println!("    {}: {}", doc.tile_at(finding.grid, finding.cell), standing.map(|entity| entity.prototype.as_str()).collect::<Vec<_>>().join(", "));
        }
        let _: Result<serde_json::Value, String> = engine.call("close", &serde_json::json!({ "doc": doc.id }));
    }
}

#[test]
fn findings_are_grouped_by_rule_in_the_order_of_the_rules() {
    let doc = doc(&[entity(11, "AirlockGlass", (5, 5)), entity(10, "WallSolid", (1, 2)), entity(12, "WallSolid", (2, 2))], &[decal(1, "Gone", (0, 1))]);
    let findings = check(&doc);
    let groups: Vec<(Rule, usize)> = grouped(&findings).iter().map(|(rule, found)| (*rule, found.len())).collect();
    assert_eq!(groups, [(Rule::FloorUnderWall, 2), (Rule::DoorWithoutFloor, 1), (Rule::UnknownDecal, 1)]);
    // Within a rule, by uid.
    assert_eq!(findings[0].entity, Some(10));
}
