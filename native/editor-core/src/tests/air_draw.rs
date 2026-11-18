use super::*;
use crate::air::{AirAnswer, GridAirJson, CIVIDIS, GAS_CONSTANT, ONE_ATMOSPHERE, PRESSURE_SCALE, TEMPERATURE_SCALE};
use crate::drawlist::FLOATS_PER_QUAD;
use std::collections::HashSet;

fn gases() -> Vec<String> {
    ["Oxygen", "Nitrogen", "CarbonDioxide", "Plasma", "Tritium", "WaterVapor"].map(String::from).to_vec()
}

/// Station air at a temperature and a share of one atmosphere.
fn air_at(kelvin: f32, atmospheres: f32) -> Mix {
    let moles = ONE_ATMOSPHERE * 2500.0 / (kelvin * GAS_CONSTANT) * atmospheres;
    Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles: vec![moles * 0.21, moles * 0.79] }
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", colour[0], colour[1], colour[2])
}

#[test]
fn each_kind_has_its_colour_and_a_mark_of_its_own() {
    let looks: Vec<(String, f32, Mark, &str)> = Breath::ALL.iter().map(|breath| breath.look()).map(|look| (hex(look.colour), look.alpha, look.mark, look.icon.glyph())).collect();
    assert_eq!(
        looks,
        [
            ("#56B4E9".to_owned(), 0.42, Mark::None, "✓"),
            ("#0072B2".to_owned(), 0.75, Mark::Snowflake, "❄"),
            ("#F0E442".to_owned(), 0.7, Mark::Dots, "!"),
            ("#E69F00".to_owned(), 0.75, Mark::Bars, "▲"),
            ("#14141C".to_owned(), 0.85, Mark::Stars, "✕"),
            ("#CC79A7".to_owned(), 0.75, Mark::Waves, "☁"),
        ]
    );
    assert_eq!((hex(NO_AIR.colour), NO_AIR.mark), ("#F0E442".to_owned(), Mark::Tape));
    // Every kind with a problem has a mark none of the others has, and an icon.
    let marks: HashSet<Mark> = Breath::ALL.iter().map(|breath| breath.look().mark).chain([NO_AIR.mark]).collect();
    assert_eq!(marks.len(), 7);
    // Dark ink on yellow, light on the rest.
    assert_eq!(Breath::Thin.look().ink(), [40, 40, 20, 191]);
    assert_eq!(Breath::Cold.look().ink(), [255, 255, 255, 204]);
}

#[test]
fn marks_fade_out_and_labels_hide_as_the_map_zooms_out() {
    assert_eq!([4.0, 8.0, 9.0, 10.0, 11.0, 12.0, 40.0].map(mark_strength), [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
    // Labels: none below ten pixels a tile, and only on rooms large enough on screen.
    assert!(!label_shown(1000, 9.0));
    assert!(label_shown(12, 10.0) && !label_shown(11, 10.0));
    assert!(label_shown(1, 35.0) && !label_shown(1, 34.0));
}

/// The rectangles' colours, as whole numbers, with their alphas in hundredths.
fn colours(quads: &[f32]) -> Vec<[u32; 4]> {
    quads.chunks_exact(FLOATS_PER_QUAD).map(|quad| [quad[8] * 255.0, quad[9] * 255.0, quad[10] * 255.0, quad[11] * 100.0].map(|value| value.round() as u32)).collect()
}

#[test]
fn a_tile_has_its_colour_and_its_mark_drawn_with_ink() {
    let world = |point: Point| point;
    let count = |look: &Look, marks: f32| {
        let mut out = Vec::new();
        tile(&world, (3, 4), look, marks, &mut Marks::new(), &mut out);
        out
    };
    // Normal air: the light tint and nothing more.
    assert_eq!(colours(&count(&Breath::Normal.look(), 1.0)), [[86, 180, 233, 42]]);
    // Cold: the square, then three white lines, faint at half strength.
    let cold = count(&Breath::Cold.look(), 0.5);
    assert_eq!(colours(&cold)[1..], [[255, 255, 255, 40]; 3]);
    // The mark is inside the tile, and the tile at its cell.
    assert!(cold.chunks_exact(FLOATS_PER_QUAD).all(|quad| quad[..8].chunks_exact(2).all(|corner| (3.0..=4.0).contains(&corner[0]) && (4.0..=5.0).contains(&corner[1]))));
    // Thin air's dots are dark.
    assert_eq!(colours(&count(&Breath::Thin.look(), 1.0))[1], [40, 40, 20, 75]);
    // Each mark's count of pieces, and none with marks faded out.
    let pieces = |breath: Breath| count(&breath.look(), 1.0).len() / FLOATS_PER_QUAD - 1;
    assert_eq!(Breath::ALL.map(pieces), [0, 3, 3, 3, 6, 24]);
    assert!(Breath::ALL.iter().all(|breath| count(&breath.look(), 0.0).len() == FLOATS_PER_QUAD));
    // The tape: yellow, with four black bands.
    assert_eq!(colours(&count(&NO_AIR, 1.0)), [[240, 228, 66, 90], [20, 20, 20, 90], [20, 20, 20, 90], [20, 20, 20, 90], [20, 20, 20, 90]]);
}

#[test]
fn the_scales_rise_evenly_from_dark_blue_to_bright_yellow() {
    // Each tick is evenly placed, and between two ticks is between.
    assert_eq!(PRESSURE_SCALE.stops.iter().map(|stop| PRESSURE_SCALE.place(stop.0)).collect::<Vec<_>>(), [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    assert!((PRESSURE_SCALE.place(235.0) - 0.7).abs() < 0.01);
    assert_eq!((PRESSURE_SCALE.place(-5.0), PRESSURE_SCALE.place(9000.0), PRESSURE_SCALE.place(f32::NAN)), (0.0, 1.0, 0.0));
    assert_eq!(PRESSURE_SCALE.colour(0.0), [0, 32, 77]);
    assert_eq!(PRESSURE_SCALE.colour(9000.0), [253, 231, 55]);
    assert_eq!(TEMPERATURE_SCALE.colour(-273.15), [0, 32, 77]);
    // The safe band is normal air's, from four tenths of the way to eight.
    assert_eq!((PRESSURE_SCALE.place(PRESSURE_SCALE.safe.0), PRESSURE_SCALE.place(PRESSURE_SCALE.safe.1)), (0.4, 0.8));
    assert_eq!((TEMPERATURE_SCALE.place(TEMPERATURE_SCALE.safe.0), TEMPERATURE_SCALE.place(TEMPERATURE_SCALE.safe.1)), (0.4, 0.8));
    // Brightness rises at every step.
    let brightness = |colour: [u8; 3]| 0.2126 * colour[0] as f32 + 0.7152 * colour[1] as f32 + 0.0722 * colour[2] as f32;
    let steps: Vec<f32> = (0..=20).map(|step| brightness(air::cividis(step as f32 / 20.0))).collect();
    assert!(steps.windows(2).all(|pair| pair[1] > pair[0]), "{steps:?}");
    assert_eq!(air::cividis(1.0 / 6.0), CIVIDIS[1]);
}

/// A mirror of grid 5 from rows drawn in text, the top row first: `.` floor of station
/// air, `c` cold air, `f` freezer air, `t` thin air, `p` plasma, `v` vacuum, `n` a floor with
/// no air, `#` wall, `_` space, ` ` no tile.
fn mirror(rows: &[&str]) -> AirMirror {
    let mixes = vec![
        air_at(293.15, 1.0),
        air_at(263.15, 1.0),
        air_at(234.8, 1.0),
        air_at(293.15, 0.45),
        Mix { temperature: 293.15, volume: 2500.0, immutable: false, moles: vec![0.0, 0.0, 0.0, 104.0] },
        Mix { temperature: 2.7, volume: 2500.0, immutable: false, moles: vec![] },
    ];
    let (mut tiles, mut cells, mut unfilled, mut blocked) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (row, line) in rows.iter().enumerate() {
        for (x, letter) in line.chars().enumerate() {
            let (x, y) = (x as i32, (rows.len() - 1 - row) as i32);
            let flags = match letter {
                '#' => 31,
                '_' => 32,
                ' ' => continue,
                _ => 0,
            };
            cells.extend([x, y, flags]);
            match letter {
                '#' => blocked.extend([x, y]),
                'n' => unfilled.extend([x, y]),
                '_' => {}
                _ => tiles.extend([x, y, ".cftpv".find(letter).unwrap() as i32]),
            }
        }
    }
    AirMirror::from_answer(AirAnswer { gases: gases(), grids: vec![GridAirJson { grid: 5, mixes, tiles, blocked, unfilled, cells, ..Default::default() }], map_is_space: true })
}

fn grid() -> Grid {
    let scene: crate::model::SceneJson = serde_json::from_str(
        r#"{"document":{"id":1,"source":"t.yml","category":"Map","mapUid":1},
            "grids":[{"uid":5,"name":"a","x":0,"y":0,"rotation":0,"tileIds":["Space"],"tiles":[]}],
            "entities":[],"history":{"canUndo":false,"canRedo":false,"revision":0}}"#,
    )
    .unwrap();
    crate::model::Doc::from_scene(scene, None).grids.remove(0)
}

/// The labels of a view, by their words, with the cell each is on.
fn texts(mirror: &AirMirror, view: AirView) -> Vec<(String, (i32, i32))> {
    let mut found: Vec<(String, (i32, i32))> = labels(&[grid()], mirror, view).into_iter().map(|label| (format!("{}{}", label.icon.map(|icon| format!("{} ", icon.glyph())).unwrap_or_default(), label.text), (label.at.0.floor() as i32, label.at.1.floor() as i32))).collect();
    found.sort();
    found
}

#[test]
fn a_problem_room_says_what_it_is_and_a_normal_room_says_nothing() {
    // Rooms in a row, each three tiles wide and walled.
    let rooms = mirror(&["#####################", "#...#ccc#ttt#ppp#vvv#", "#...#ccc#ttt#ppp#vvv#", "#####################"]);
    assert_eq!(
        texts(&rooms, AirView::Breathable),
        [
            ("! Thin air, 46 kPa".to_owned(), (10, 1)),
            ("☁ Plasma".to_owned(), (14, 1)),
            ("✕ Vacuum".to_owned(), (18, 1)),
            ("❄ Cold, -10 °C".to_owned(), (6, 1)),
        ]
    );
    // A room with part of a problem says how much, the worst first of two as large, and a
    // floor with no air in an otherwise normal room says so.
    let mixed = mirror(&["##########", "#cc.#ffnn#", "#...#..nn#", "##########"]);
    let said = texts(&mixed, AirView::Breathable);
    assert_eq!(said.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>(), ["? Some floor has no air", "❄ Cold, -10 °C (2 of 6 tiles)"]);
    let freezer = mirror(&["#####", "#ff.#", "#####"]);
    assert_eq!(texts(&freezer, AirView::Breathable)[0].0, "☁ Too cold, -38 °C (2 of 3 tiles)");
    assert_eq!(texts(&mirror(&["####", "#nn#", "####"]), AirView::Breathable)[0].0, "? No air stored");
    // A vacuum's circle is pale grey, as its near-black would not show.
    assert_eq!(labels(&[grid()], &rooms, AirView::Breathable).iter().find(|label| label.text == "Vacuum").unwrap().colour, [187, 187, 187]);
}

#[test]
fn a_room_open_to_space_is_outlined_with_arrows_out_and_labelled() {
    // A room of six with a gap at its top, beside a sealed room of thin air.
    let leaking = mirror(&["##_######", "#...#ttt#", "#...#ttt#", "#########"]);
    assert_eq!(texts(&leaking, AirView::Breathable), [("! Thin air, 46 kPa".to_owned(), (6, 1)), ("⇢ Leaking to space".to_owned(), (2, 1))]);
    assert_eq!(texts(&leaking, AirView::Rooms), [("⇢ Leaking to space".to_owned(), (2, 1))]);
    assert_eq!(texts(&leaking, AirView::Pressure), [("46 kPa".to_owned(), (6, 1))]);
    assert_eq!(texts(&leaking, AirView::Temperature), [("20 °C".to_owned(), (6, 1))]);

    let orange = [230, 159, 0, 100];
    let leaks = |view: AirView| colours(&quads(&[grid()], &leaking, view, 1.0)).iter().filter(|colour| **colour == orange).count();
    // The room's edge is ten sides of tiles with two dashes each, and its one gap has an
    // arrow of a shaft and a head.
    assert_eq!(leaks(AirView::Breathable), 10 * 2 + 2);
    assert_eq!(leaks(AirView::Rooms), 10 * 2 + 2);
    assert_eq!(leaks(AirView::Pressure), 0);
    // The arrow points up out of the gap, at (2, 2), into the space above it.
    let quads = quads(&[grid()], &leaking, AirView::Rooms, 1.0);
    let head = quads.chunks_exact(FLOATS_PER_QUAD).filter(|quad| colours(quad)[0] == orange).last().unwrap();
    assert_eq!((head[0], head[1]), (2.5, 2.5 + 24.0 / 22.0));
    // In the Rooms view both rooms take soft colours, and the outside none.
    let soft: HashSet<[u32; 4]> = colours(&quads).into_iter().filter(|colour| colour[3] == 45).collect();
    assert_eq!(soft.len(), 2);
}

#[test]
fn of_labels_that_would_cover_each_other_the_larger_room_s_is_kept() {
    let label = |text: &str, tiles: usize| AirLabel { at: (0.0, 0.0), icon: Some(Icon::Cross), text: text.to_owned(), colour: [0, 0, 0], tiles };
    assert_eq!(label_width(&label("Vacuum", 1)), 6.0 * 6.3 + 34.0);
    // Side by side and close, one above the other and close, and well apart.
    let placed = vec![((100.0, 100.0), label("Small", 4)), ((130.0, 105.0), label("Large", 40)), ((130.0, 130.0), label("Below", 2)), ((400.0, 100.0), label("Far", 1))];
    let kept: Vec<String> = uncrowded(placed).into_iter().map(|(_, label)| label.text).collect();
    assert_eq!(kept, ["Large", "Below", "Far"]);
}

#[test]
fn a_label_sits_on_the_tile_nearest_its_room_s_middle() {
    // An L: its middle is off the room, so the nearest tile of it is taken, the lower of two as near.
    assert_eq!(middle_tile(&[(0, 0), (1, 0), (2, 0), (0, 1), (0, 2)]), Some((0, 1)));
    assert_eq!(middle_tile(&[(0, 0), (1, 0), (2, 0)]), Some((1, 0)));
    assert_eq!(middle_tile(&[]), None);
}

#[test]
fn another_mix_is_named_by_its_odd_gas_its_oxygen_or_its_heat() {
    let gases = gases();
    let mix = |kelvin: f32, moles: &[f32]| Mix { temperature: kelvin, volume: 2500.0, immutable: false, moles: moles.to_vec() };
    assert_eq!(other_name(&mix(293.15, &[21.8, 81.0, 0.0, 1.0]), &gases), "Plasma");
    assert_eq!(other_name(&mix(293.15, &[21.8, 81.0, 0.0, 0.0, 1.0]), &gases), "Tritium");
    assert_eq!(other_name(&mix(293.15, &[0.0, 103.9]), &gases), "Too little oxygen");
    assert_eq!(other_name(&mix(293.15, &[60.0, 43.9]), &gases), "Too much oxygen");
    assert_eq!(other_name(&air_at(400.0, 1.0), &gases), "Too hot, 127 °C");
    assert_eq!(degrees(-0.3), "0 °C");
}

#[test]
fn a_legend_s_sample_is_the_tile_as_the_map_draws_it() {
    let pixel = |pixels: &[u8], x: u32, y: u32, size: u32| -> [u8; 4] { pixels[((y * size + x) * 4) as usize..][..4].try_into().unwrap() };
    let floor = sample(Sample::Floor, 8);
    assert_eq!((floor.len(), pixel(&floor, 3, 3, 8)), (8 * 8 * 4, [74, 77, 85, 255]));
    // The tape has both its yellow and its black.
    let tape = sample(Sample::NoAir, 22);
    let distinct: HashSet<[u8; 4]> = tape.chunks_exact(4).map(|pixel| pixel.try_into().unwrap()).collect();
    assert!(distinct.iter().any(|colour| colour[0] > 200 && colour[2] < 100) && distinct.iter().any(|colour| colour[0] < 50));
    // The snowflake's middle is light on the blue.
    let cold = sample(Sample::Breath(Breath::Cold), 22);
    assert!(pixel(&cold, 11, 11, 22)[0] > 120);
    assert!(pixel(&cold, 2, 20, 22)[2] > pixel(&cold, 2, 20, 22)[0]);
    // A leak's arrow is orange across the middle.
    assert_eq!(pixel(&sample(Sample::Leak, 22), 11, 11, 22), [230, 159, 0, 255]);
    // An icon: clear outside its circle, the circle's colour, and dark ink in the middle.
    let cross = icon_picture(Icon::Cross, [187, 187, 187], 28);
    assert_eq!((pixel(&cross, 0, 0, 28), pixel(&cross, 14, 2, 28), pixel(&cross, 14, 14, 28)), ([0, 0, 0, 0], [187, 187, 187, 255], [10, 10, 22, 255]));
    // Every icon is drawn, and no two alike.
    let all = [Icon::Tick, Icon::Snowflake, Icon::Bang, Icon::Triangle, Icon::Cross, Icon::Cloud, Icon::Question, Icon::Arrow];
    let pictures: HashSet<Vec<u8>> = all.iter().map(|&icon| icon_picture(icon, [255, 255, 255], 28)).collect();
    assert_eq!(pictures.len(), all.len());
}
