use super::*;

fn gases() -> Vec<String> {
    ["Oxygen", "Nitrogen", "CarbonDioxide", "Plasma", "Tritium", "WaterVapor"].map(String::from).to_vec()
}

/// A 2500 litre mix at a temperature, of moles in the order of [`gases`].
fn mix(kelvin: f32, moles: &[f32]) -> Mix {
    Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles: moles.to_vec() }
}

/// Station air of the game's own numbers, at a temperature and a share of one atmosphere.
fn air(kelvin: f32, atmospheres: f32) -> Mix {
    // Atmospherics.MolesCellStandard, at the pressure asked for.
    let moles = ONE_ATMOSPHERE * 2500.0 / (kelvin * GAS_CONSTANT) * atmospheres;
    mix(kelvin, &[moles * 0.21, moles * 0.79])
}

#[test]
fn the_game_s_station_air_is_one_atmosphere_and_normal() {
    // The mix Pursuit's file stores.
    let station = mix(293.15, &[21.824879, 82.10312]);
    assert!((station.pressure() - ONE_ATMOSPHERE).abs() < 0.1, "{}", station.pressure());
    assert!((station.celsius() - 20.0).abs() < 0.01);
    assert_eq!(breathable(&station, &gases()), Breath::Normal);
}

#[test]
fn each_breathable_category_has_its_mixes() {
    let gases = gases();
    let verdict = |mix: Mix| breathable(&mix, &gases);
    // Cork's map atmosphere, 15 °C, is normal air.
    assert_eq!(verdict(air(288.15, 1.0)), Breath::Normal);
    // Below freezing and above freezer air: cold but breathable.
    assert_eq!(verdict(air(T0C - 10.0, 1.0)), Breath::Cold);
    // Freezer air itself, and anything colder, is another mix.
    assert_eq!(verdict(air(FREEZER, 1.0)), Breath::Other);
    // Too hot for a body.
    assert_eq!(verdict(air(HEAT_DAMAGE + 1.0, 1.0)), Breath::Other);
    // Thin: half an atmosphere of air.
    assert_eq!(verdict(air(293.15, 0.5)), Breath::Thin);
    // Pressure that the game warns of, and more.
    assert_eq!(verdict(air(293.15, 4.0)), Breath::HighPressure);
    // Vacuum: nothing at all, the map's own, and air too thin to count.
    assert_eq!(verdict(mix(SPACE_TEMPERATURE, &[])), Breath::Vacuum);
    assert_eq!(verdict(air(293.15, 0.1)), Breath::Vacuum);
    // Nitrogen alone, too little oxygen, too much oxygen, plasma.
    assert_eq!(verdict(mix(293.15, &[0.0, 103.9])), Breath::Other);
    assert_eq!(verdict(mix(293.15, &[10.0, 93.9])), Breath::Other);
    assert_eq!(verdict(mix(293.15, &[60.0, 43.9])), Breath::Other);
    assert_eq!(verdict(mix(293.15, &[21.8, 81.0, 0.0, 1.0])), Breath::Other);
    // A trace under the alarms' smallest danger is still air, and water vapour is left out.
    assert_eq!(verdict(mix(293.15, &[21.8, 82.0, 0.1])), Breath::Normal);
    assert_eq!(verdict(mix(293.15, &[21.8, 72.0, 0.0, 0.0, 0.0, 10.0])), Breath::Normal);
}

#[test]
fn the_categories_have_six_colours_apart_and_none_red_or_green() {
    let colours: HashSet<[u8; 3]> = Breath::ALL.iter().map(|breath| breath.colour()).collect();
    assert_eq!(colours.len(), 6);
    assert_eq!(Breath::Normal.colour(), [86, 180, 233]);
    assert_eq!(Breath::Vacuum.colour(), [20, 20, 28]);
    // No colour is mostly red or mostly green.
    assert!(colours.iter().all(|&[red, green, blue]| !(red > 200 && green < 100 && blue < 100) && !(green > 180 && red < 120 && blue < 140)));
}

#[test]
fn a_view_colours_a_mix_by_its_own_measure() {
    let station = air(293.15, 1.0);
    assert_eq!(colour(AirView::Pressure, &station, &gases()), PRESSURE_SCALE.colour(station.pressure()));
    assert_eq!(colour(AirView::Temperature, &station, &gases()), cividis(0.6));
    assert_eq!(colour(AirView::Breathable, &station, &gases()), Breath::Normal.colour());
    // The scales cover the floor alike, with no mark.
    let scaled = look(AirView::Pressure, &station, &gases());
    assert_eq!((scaled.alpha, scaled.mark, scaled.icon), (SCALE_ALPHA, Mark::None, Icon::Tick));
    assert_eq!(look(AirView::Breathable, &air(T0C - 10.0, 1.0), &gases()).mark, Mark::Snowflake);
}

#[test]
fn every_view_s_legend_names_its_colours_and_the_tiles_with_no_air() {
    let breathable = legend(AirView::Breathable, None);
    assert_eq!(breathable.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), ["Normal air", "Cold air", "Too thin", "Too much pressure", "Vacuum", "Other mix"]);
    assert_eq!(breathable.rows[1].sample, Sample::Breath(Breath::Cold));
    assert!(breathable.scale.is_none() && breathable.note.is_none());
    assert_eq!(breathable.rows[0].meaning, "Oxygen and nitrogen, 85 to 385 kPa, 0 to 52 °C");
    // The Breathable view shows leaks, and the scales do not.
    let specials = |view: AirView| legend(view, None).special.iter().map(|row| (row.label.clone(), row.sample)).collect::<Vec<_>>();
    assert_eq!(specials(AirView::Breathable), [("Floor with no air".to_owned(), Sample::NoAir), ("Wall".to_owned(), Sample::Wall), ("Leaking to space".to_owned(), Sample::Leak)]);
    for view in [AirView::Pressure, AirView::Temperature] {
        assert_eq!(specials(view), specials(AirView::Breathable)[..2]);
    }
    let pressure = legend(AirView::Pressure, None);
    assert_eq!(pressure.title, "Air: pressure (kPa)");
    assert!(pressure.note.unwrap().starts_with("Darker means less air, brighter means more"));
    assert_eq!(pressure.scale.unwrap().stops.iter().map(|stop| stop.1).collect::<Vec<_>>(), ["0", "20", "85", "101", "385", "550+"]);
    assert_eq!(legend(AirView::Temperature, None).scale.unwrap().stops.iter().map(|stop| stop.1).collect::<Vec<_>>(), ["-270", "-38", "0", "20", "52", "120+"]);
}

/// An answer for grid 5 with what blocks air on each tile, drawn in text with the top row
/// first: `.` floor, `#` wall, `D` closed airlock, `_` space or lattice, ` ` no tile.
fn walled(rows: &[&str], map_is_space: bool) -> AirMirror {
    let mut cells = Vec::new();
    for (row, line) in rows.iter().enumerate() {
        for (x, letter) in line.chars().enumerate() {
            let flags = match letter {
                '.' => 0,
                '#' => 31,
                'D' => 15,
                '_' => 32,
                _ => continue,
            };
            cells.extend([x as i32, (rows.len() - 1 - row) as i32, flags]);
        }
    }
    let mut told = answer(5, 293.15);
    told.grids[0] = GridAirJson { grid: 5, cells, blocked: vec![0, 0], ..Default::default() };
    told.map_is_space = map_is_space;
    AirMirror::from_answer(told)
}

/// A room of four tiles and one of six apart by an airlock, the larger open at its top,
/// and a tile beside a lattice, which is the outside.
const ROOMS: [&str; 5] = ["#####_##", "#..D...#", "#..#...#", "########", "####._##"];

#[test]
fn the_rooms_view_colours_rooms_and_leaves_walls_and_the_outside() {
    let mirror = walled(&ROOMS, true);
    let doc = crate::model::Doc::from_scene(scene(), None);
    // Grid 5 is not in the scene: the rooms of grid 2 are drawn instead.
    let mut mirror_on_a = mirror.clone();
    mirror_on_a.grids.insert(2, mirror.grids[&5].clone());
    let quads = crate::air_draw::quads(&doc.grids, &mirror_on_a, AirView::Rooms, 1.0);
    let floats = crate::drawlist::FLOATS_PER_QUAD;
    // Four sealed and six open: a square each. The outside is left. The open room's edge
    // of ten sides has two dashes on each, and its gap an arrow of two pieces.
    assert_eq!(quads.len() / floats, 10 + 10 * 2 + 2);
    let colours: HashSet<[u32; 3]> = quads.chunks_exact(floats).map(|quad| [quad[8], quad[9], quad[10]].map(|channel| (channel * 255.0).round() as u32)).collect();
    assert!(colours.contains(&LEAK.map(u32::from)));
    // Both rooms in their soft colours, the open one too.
    let rooms: Vec<_> = mirror.grids[&5].rooms.rooms.iter().filter(|room| !room.is_outside()).collect();
    assert_eq!(rooms.len(), 2);
    assert!(rooms.iter().all(|room| colours.contains(&ROOM_COLOURS[room.colour].map(u32::from))));

    // The leaks are those of the open room, and not the outside's.
    assert_eq!(leaks(&mirror).collect::<Vec<_>>(), [(5, (5, 3))]);
    assert_eq!(room_counts(&mirror), (1, 1));
}

#[test]
fn the_rooms_view_tells_the_room_under_the_mouse_and_its_legend_counts_them() {
    let mirror = walled(&ROOMS, true);
    // Its floors store no air, which a second line says.
    assert_eq!(room_hover(&mirror, 5, (1, 3)).unwrap(), ["Room 1, 4 tiles, sealed", "Air: 4 of 4 tiles store no air"]);
    assert_eq!(room_hover(&mirror, 5, (5, 2)).unwrap(), ["Room 3, 6 tiles, open to space through 1 tile"]);
    // A wall, a closed airlock, and a cell of no grid.
    assert_eq!(room_hover(&mirror, 5, (0, 0)).unwrap()[0], "Wall");
    assert_eq!(room_hover(&mirror, 5, (3, 3)), None);
    assert_eq!(room_hover(&mirror, 9, (1, 3)), None);

    let shown = legend(AirView::Rooms, Some(&mirror));
    assert_eq!(shown.title, "Air: rooms");
    assert_eq!(shown.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), ["Sealed room (1 room)", "Leaking to space (1 room)"]);
    assert!(shown.rows[1].sample == Sample::Leak && shown.scale.is_none());
    // On a map whose atmosphere is air.
    let cork = walled(&ROOMS, false);
    assert_eq!(legend(AirView::Rooms, Some(&cork)).rows[1].label, "Leaking to the map's atmosphere (1 room)");
    assert_eq!(room_hover(&cork, 5, (5, 2)).unwrap(), ["Room 3, 6 tiles, open to the map's atmosphere through 1 tile"]);
    // With no air read yet, none.
    assert_eq!(legend(AirView::Rooms, None).rows[0].label, "Sealed room (0 rooms)");

    // The outside: a walkway of ten tiles under lattice.
    let walkway = walled(&["__________", "..........", "##########"], true);
    assert_eq!(room_hover(&walkway, 5, (3, 1)).unwrap(), ["Outside", "Open to space on 10 of its 10 tiles"]);
    assert_eq!((room_counts(&walkway), leaks(&walkway).count()), ((0, 0), 0));
}

#[test]
fn the_hover_box_tells_pressure_temperature_and_each_gas() {
    let gases = gases();
    let station = mix(293.15, &[21.824879, 82.10312]);
    assert_eq!(hover_text(TileAir::Stored(&station), &gases).unwrap(), ["Normal air", "101.3 kPa, 20.0 °C", "Nitrogen 79.0%", "Oxygen 21.0%"]);
    // Led by its kind's icon.
    assert_eq!(hover_icon(TileAir::Stored(&station), &gases), Some((Icon::Tick, [86, 180, 233])));

    // The map's own: Bagel's vacuum, and Cork's air.
    let vacuum = Mix { immutable: true, ..mix(SPACE_TEMPERATURE, &[]) };
    assert_eq!(hover_text(TileAir::Stored(&vacuum), &gases).unwrap(), ["Map atmosphere: vacuum".to_owned(), format!("0.0 kPa, {:.1} °C", SPACE_TEMPERATURE - T0C), "No gas".to_owned()]);
    assert_eq!(hover_icon(TileAir::Stored(&vacuum), &gases), Some((Icon::Cross, [187, 187, 187])));
    let outside = Mix { immutable: true, ..air(288.15, 1.0) };
    assert_eq!(hover_text(TileAir::Stored(&outside), &gases).unwrap()[0], "Map atmosphere: normal air");

    // Gases in their spoken names, and a trace.
    let damp = mix(293.15, &[21.0, 70.0, 0.05, 0.0, 0.0, 12.0]);
    assert_eq!(&hover_text(TileAir::Stored(&damp), &gases).unwrap()[2..], ["Nitrogen 67.9%", "Oxygen 20.4%", "Water vapor 11.6%", "Carbon dioxide <0.1%"]);

    assert_eq!(hover_text(TileAir::Wall, &gases).unwrap()[0], "Wall");
    assert_eq!(hover_text(TileAir::Unfilled, &gases).unwrap(), ["Floor with no air", "The game turns it to vacuum"]);
    assert_eq!((hover_icon(TileAir::Unfilled, &gases), hover_icon(TileAir::Wall, &gases)), (Some((Icon::Question, [240, 228, 66])), None));
    assert_eq!(hover_text(TileAir::Nothing, &gases), None);
}

/// An answer for one grid: a mix on two tiles, a wall and a floor with none.
fn answer(grid: i32, kelvin: f32) -> AirAnswer {
    AirAnswer {
        gases: gases(),
        grids: vec![GridAirJson { grid, mixes: vec![air(kelvin, 1.0)], tiles: vec![0, 0, 0, 1, 0, 0], blocked: vec![2, 0], unfilled: vec![3, 0], ..Default::default() }],
        map_is_space: true,
    }
}

#[test]
fn the_mirror_tells_each_tile_and_takes_in_grids_read_again() {
    let mut mirror = AirMirror::from_answer(answer(5, 293.15));
    assert_eq!(mirror.revision, 1);
    assert!(matches!(mirror.at(5, (1, 0)), TileAir::Stored(mix) if mix.temperature == 293.15));
    assert_eq!((mirror.at(5, (2, 0)), mirror.at(5, (3, 0)), mirror.at(5, (4, 0)), mirror.at(6, (0, 0))), (TileAir::Wall, TileAir::Unfilled, TileAir::Nothing, TileAir::Nothing));

    // Another grid read beside it keeps the first.
    mirror.update(Some(&[7]), answer(7, 250.0));
    assert_eq!((mirror.grids.len(), mirror.revision), (2, 2));

    // Grid 5 read again after an edit, and grid 7 asked about and not answered: it is gone.
    mirror.update(Some(&[5, 7]), answer(5, 280.0));
    assert!(matches!(mirror.at(5, (0, 0)), TileAir::Stored(mix) if mix.temperature == 280.0));
    assert_eq!(mirror.grids.keys().copied().collect::<Vec<_>>(), [5]);
    assert_eq!(mirror.revision, 3);
}

#[test]
fn a_floor_with_no_air_under_an_open_door_shows_its_neighbours_air() {
    // A row: air at 290 K, an open door with none, air at 300 K with half the moles, a
    // closed door with none, a wall. Then a floor with none and nothing on it.
    let mut told = answer(3, 290.0);
    let mut thin = air(300.0, 0.5);
    thin.moles = vec![10.0, 40.0];
    told.grids[0].mixes = vec![mix(290.0, &[20.0, 80.0]), thin];
    told.grids[0].tiles = vec![0, 0, 0, 2, 0, 1];
    told.grids[0].blocked = vec![4, 0];
    told.grids[0].unfilled = vec![1, 0, 3, 0, 5, 0];
    told.grids[0].cells = vec![0, 0, 0, 1, 0, 64, 2, 0, 0, 3, 0, 15 | 64, 4, 0, 31 | 64, 5, 0, 0];
    let mirror = AirMirror::from_answer(told);

    // The open door takes the mean of both sides, moles and temperature each.
    let TileAir::Filled(door) = mirror.at(3, (1, 0)) else { panic!("the open door is not filled: {:?}", mirror.at(3, (1, 0))) };
    assert_eq!((door.moles.clone(), door.temperature, door.volume, door.immutable), (vec![15.0, 60.0], 295.0, 2500.0, false));
    // The closed door shares air with neither side and the floor is no door: both turn to vacuum.
    assert_eq!((mirror.at(3, (3, 0)), mirror.at(3, (5, 0))), (TileAir::Unfilled, TileAir::Unfilled));
    // 75 moles at 295 K is thin air.
    assert_eq!(hover_text(TileAir::Filled(door), &gases()).unwrap()[..2], ["Too thin", "Stores no air. The game gives it its neighbours' air"]);

    // Drawn in the colour of that air, and not taped: normal air stored, thin air stored and
    // filled with three dots each, and two taped floors with four stripes each.
    let doc = crate::model::Doc::from_scene(scene(), None);
    assert_eq!(crate::air_draw::quads(&doc.grids, &mirror, AirView::Breathable, 1.0).len() / crate::drawlist::FLOATS_PER_QUAD, 1 + 2 * 4 + 2 * 5);
}

#[test]
fn the_mirror_has_each_grid_s_rooms_and_whether_the_map_has_air() {
    // Two floors and a wall in a row, with nothing round them: one room, open.
    let mut told = answer(5, 293.15);
    told.grids[0].cells = vec![0, 0, 0, 1, 0, 0, 2, 0, 31];
    let mirror = AirMirror::from_answer(told);
    let rooms = &mirror.grids[&5].rooms;
    assert_eq!((rooms.rooms.len(), rooms.rooms[0].size, rooms.rooms[0].leaks.len()), (1, 2, 2));
    assert!(!mirror.map_has_air);
    // Rooms found on the engine's thread are kept as they are.
    let mut found = answer(5, 293.15);
    found.grids[0].rooms = Some(GridRooms::default());
    assert!(AirMirror::from_answer(found).grids[&5].rooms.rooms.is_empty());

    // Cork's map atmosphere is air. An engine that does not say is taken as space.
    let cork: AirAnswer = serde_json::from_str(r#"{"gases":[],"grids":[],"mapIsSpace":false}"#).unwrap();
    assert!(AirMirror::from_answer(cork).map_has_air);
    let older: AirAnswer = serde_json::from_str(r#"{"gases":[],"grids":[]}"#).unwrap();
    assert!(!AirMirror::from_answer(older).map_has_air);
}

fn scene() -> SceneJson {
    serde_json::from_str(
        r#"{"document":{"id":1,"source":"t.yml","category":"Map","mapUid":1},
            "grids":[{"uid":2,"name":"a","x":0,"y":0,"rotation":0,"tileIds":["Space"],"tiles":[]},
                     {"uid":3,"name":"b","x":10,"y":0,"rotation":0,"tileIds":["Space"],"tiles":[]}],
            "entities":[{"uid":10,"yamlUid":10,"prototype":"WallSolid","parent":2,"x":0.5,"y":0.5,"rotation":0,"anchored":true,"contained":false,"name":null},
                        {"uid":11,"yamlUid":11,"prototype":"Locker","parent":3,"x":0.5,"y":0.5,"rotation":0,"anchored":false,"contained":false,"name":null},
                        {"uid":12,"yamlUid":12,"prototype":"Crowbar","parent":11,"x":0,"y":0,"rotation":0,"anchored":false,"contained":true,"name":null}],
            "history":{"canUndo":false,"canRedo":false,"revision":0}}"#,
    )
    .unwrap()
}

/// A change of the engine's, with only what is given in it.
fn delta(changes: serde_json::Value) -> Delta {
    let mut whole = serde_json::json!({ "tiles": [], "entities": [], "removed": [], "gridsChanged": [], "created": [], "history": { "canUndo": true, "canRedo": false, "revision": 1 } });
    for (key, value) in changes.as_object().unwrap() {
        whole[key] = value.clone();
    }
    serde_json::from_value(whole).unwrap()
}

fn entity(uid: i32, prototype: &str, parent: i32) -> serde_json::Value {
    serde_json::json!({ "uid": uid, "yamlUid": uid, "prototype": prototype, "parent": parent, "x": 0.5, "y": 0.5, "rotation": 0, "anchored": false, "contained": false, "name": null })
}

#[test]
fn an_edit_s_grids_are_those_it_touched_before_and_after() {
    use serde_json::json;
    let mut grids = GridsOf::new(&scene());
    // A tile painted on the first grid.
    assert_eq!(grids.touched(&delta(json!({ "tiles": [{ "grid": 2, "x": 4, "y": 4, "tile": "Plating" }] }))), [2]);
    // The wall deleted: its grid, known though the wall is gone.
    assert_eq!(grids.touched(&delta(json!({ "removed": [10] }))), [2]);
    assert_eq!(grids.touched(&delta(json!({ "removed": [10] }))), Vec::<i32>::new());
    // The crowbar in the locker moved onto the first grid: both grids.
    assert_eq!(grids.touched(&delta(json!({ "entities": [entity(12, "Crowbar", 2)] }))), [2, 3]);
    // A wall placed on a grid made in the same change.
    let grid = json!({ "uid": 4, "name": "c", "x": 0, "y": 20, "rotation": 0, "tileIds": ["Space"], "tiles": [] });
    assert_eq!(grids.touched(&delta(json!({ "gridsChanged": [grid], "entities": [entity(13, "WallSolid", 4)] }))), [4]);
    // A grid removed with what was on it: named, so that its air goes too.
    assert_eq!(grids.touched(&delta(json!({ "removed": [11, 3] }))), [3]);
    assert_eq!(grids.grid_of(13), Some(4));
    assert_eq!(grids.grid_of(11), None);
}

#[test]
fn the_overlay_colours_tiles_with_air_tapes_floors_without_and_leaves_walls() {
    use crate::air_draw::quads;
    let doc = crate::model::Doc::from_scene(scene(), None);
    let mirror = AirMirror::from_answer(answer(3, 293.15));
    let quads_of = quads(&doc.grids, &mirror, AirView::Breathable, 1.0);
    let floats = crate::drawlist::FLOATS_PER_QUAD;
    // Two tiles of air, and the floor's yellow square with four black stripes.
    assert_eq!(quads_of.len() / floats, 2 + 5);
    let first = &quads_of[..floats];
    // On grid b, ten tiles to the right, in normal air's light sky blue.
    assert!([first[0], first[2]].contains(&10.0) || [first[0], first[2]].contains(&11.0), "{first:?}");
    assert_eq!(&first[8..12], &[86.0 / 255.0, 180.0 / 255.0, 233.0 / 255.0, 0.42]);
    assert_eq!(first[12], crate::drawlist::WHITE_TEXTURE as f32);
    // The tape's stripes are near black and nearly opaque.
    let last = &quads_of[quads_of.len() - floats..];
    assert_eq!(&last[8..12], &[20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0, 0.9]);
    // Nothing for a grid the mirror has no air of.
    assert!(quads(&doc.grids, &AirMirror::default(), AirView::Pressure, 1.0).is_empty());
}
