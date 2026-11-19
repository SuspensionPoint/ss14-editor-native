//! The toolbar's tools and options on a session: keys, targets, shapes, what Select selects
//! and how, and where the options are kept.

use super::decals::decals_on_map;
use super::*;
use crate::forks::settings::Settings;
use crate::toolbox::{BarTool, SelectMode, SelectTargets, Shape, Target, ToolOptions};

/// Presses a key and lets it go.
fn tap(session: &mut Session, key: char) {
    assert!(session.key_down(key, false), "{key}");
    session.key_up(key);
}

#[test]
fn each_key_takes_its_toolbar_tool() {
    let (mut session, _engine) = Session::offline(&scene());
    for (key, bar, tool) in [
        ('B', BarTool::Paint, Tool::Paint),
        ('E', BarTool::Erase, Tool::Erase),
        ('G', BarTool::Fill, Tool::Fill),
        ('I', BarTool::Pick, Tool::Pick),
        ('O', BarTool::Room, Tool::Room),
        ('H', BarTool::Pan, Tool::Pan),
        ('D', BarTool::Link, Tool::Link),
        ('K', BarTool::Cables, Tool::Cable),
        ('J', BarTool::Pipes, Tool::Pipe),
        ('S', BarTool::Select, Tool::Area),
        ('V', BarTool::Select, Tool::Select),
    ] {
        session.set_tool(Tool::Pan);
        tap(&mut session, key);
        assert_eq!((session.bar_tool(), session.tool()), (bar, tool), "{key}");
    }
    // A key no tool has is not taken.
    assert!(!session.key_down('Q', false));
}

#[test]
fn l_c_and_r_set_paint_s_shape_which_paint_keeps() {
    let (mut session, _engine) = Session::offline(&scene());
    for (key, shape, tool) in [('L', Shape::Line, Tool::Line), ('C', Shape::Circle, Tool::Circle), ('R', Shape::Rectangle, Tool::Rectangle)] {
        session.set_tool(Tool::Pan);
        tap(&mut session, key);
        assert_eq!((session.bar_tool(), session.shape(), session.tool()), (BarTool::Paint, Some(shape), tool), "{key}");
        // Paint taken again later keeps the shape.
        session.take_bar_tool(BarTool::Fill);
        tap(&mut session, 'B');
        assert_eq!(session.tool(), tool, "{key}");
    }
    // The options bar sets it the same way.
    session.set_shape(Shape::Point);
    assert_eq!(session.tool(), Tool::Paint);
}

#[test]
fn p_places_the_last_entity_and_b_goes_back_to_the_last_tile() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.choose_tile("FloorWood");
    assert_eq!((session.tool(), session.target()), (Tool::Paint, Target::Tiles));

    tap(&mut session, 'P');
    assert_eq!((session.bar_tool(), session.shape(), session.target(), session.tool()), (BarTool::Paint, Some(Shape::Point), Target::Entities, Tool::Place));
    assert_eq!(session.palette(), &Palette::Entity(0));
    // The palette is told to show it.
    assert!(session.take_reveal());
    click(&mut session, (2.5, 3.5));
    assert_eq!(labels(&edits(&engine)), ["Place entity"]);

    // The target set to tiles takes the tile back, and Paint at a point paints it.
    session.set_target(Target::Tiles);
    assert_eq!((session.tool(), session.palette()), (Tool::Paint, &Palette::Tile("FloorWood".into())));
}

#[test]
fn the_target_follows_the_palette() {
    let (mut session, _engine) = decals_on_map(&[]);
    choose(&mut session, "SnapgridCenter");
    assert_eq!(session.target(), Target::Entities);
    session.choose_decal("Arrows");
    assert_eq!((session.target(), session.tool()), (Target::Decals, Tool::Paint));
    session.choose_tile("FloorWood");
    assert_eq!(session.target(), Target::Tiles);
    // The eraser keeps its place and works on what was chosen.
    session.take_bar_tool(BarTool::Erase);
    session.choose_decal("Grass");
    assert_eq!((session.target(), session.tool()), (Target::Decals, Tool::Erase));
}

#[test]
fn a_target_with_nothing_in_hand_empties_the_palette_and_a_press_asks_for_one() {
    let (mut session, engine) = decals_on_map(&[]);
    session.choose_tile("FloorWood");
    session.set_target(Target::Decals);
    assert_eq!((session.palette(), session.tool()), (&Palette::None, Tool::Paint));
    click(&mut session, (2.5, 3.5));
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Choose a decal to place");
    session.set_target(Target::Entities);
    click(&mut session, (2.5, 3.5));
    assert_eq!(session.status_line(), "Choose an entity to place");
}

#[test]
fn fill_and_room_work_on_tiles_and_take_the_last_tile_back() {
    let (mut session, _engine) = Session::offline(&scene());
    session.choose_tile("FloorWood");
    choose(&mut session, "SnapgridCenter");
    session.take_bar_tool(BarTool::Fill);
    assert_eq!((session.target(), session.palette()), (Target::Tiles, &Palette::Tile("FloorWood".into())));
    // Fill refuses another target.
    session.set_target(Target::Entities);
    assert_eq!((session.target(), session.tool()), (Target::Tiles, Tool::Fill));

    choose(&mut session, "SnapgridCenter");
    session.take_bar_tool(BarTool::Room);
    assert_eq!(session.chosen_tile(), Some("FloorWood"));
}

#[test]
fn erase_in_a_rectangle_takes_the_tiles_away_as_one_step() {
    let (mut session, engine) = Session::offline(&scene());
    session.take_bar_tool(BarTool::Erase);
    session.set_shape(Shape::Rectangle);
    assert_eq!((session.tool(), session.shape()), (Tool::Erase, Some(Shape::Rectangle)));
    drag(&mut session, (0, 4), (1, 5));

    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Erase tiles"]);
    assert_eq!(edits[0].1, None);
    assert!(edits[0].2.iter().all(|op| op["kind"] == "tile" && op["tile"] == SPACE));
    assert_eq!(cells(&edits[0].2), [(0, 4), (0, 5), (1, 4), (1, 5)]);
    // Paint kept its own shape.
    session.take_bar_tool(BarTool::Paint);
    assert_eq!(session.shape(), Some(Shape::Point));
}

#[test]
fn erase_in_a_line_or_circle_with_an_entity_chosen_deletes_that_prototype() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_prototypes(vec![Prototype { id: "Chair".into(), name: "chair".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.take_bar_tool(BarTool::Erase);
    session.choose_entity(0);
    session.set_shape(Shape::Line);
    drag(&mut session, (0, 1), (2, 1));
    let edits = edits(&engine);
    assert_eq!(labels(&edits), ["Erase entities"]);
    assert_eq!(edits[0].2, [json!({ "kind": "delete", "uid": CHAIR })]);

    // A circle that misses the chair asks for nothing that changes anything.
    session.set_shape(Shape::Circle);
    drag(&mut session, (4, 4), (4, 4));
    assert!(asked(&engine).iter().all(|ops| ops.is_empty()));
}

#[test]
fn select_with_tiles_on_is_the_region_and_off_is_entities_and_decals() {
    let (mut session, _engine) = selecting();
    assert_eq!((session.bar_tool(), session.select_targets()), (BarTool::Select, SelectTargets::default()));
    session.toggle_select_target(Target::Tiles);
    assert_eq!(session.tool(), Tool::Area);
    drag(&mut session, (0, 0), (1, 1));
    assert_eq!(session.region_size(), 4);
    session.toggle_select_target(Target::Tiles);
    // The region goes with its tool.
    assert_eq!((session.tool(), session.region_size()), (Tool::Select, 0));
}

#[test]
fn select_leaves_out_a_target_that_is_off() {
    let (mut session, _engine) = decals_on_map(&[(1, "Arrows", (1.0, 1.0))]);
    draw(&mut session, &[(CHAIR, (1.5, 1.5), 0)]);
    session.set_tool(Tool::Select);
    session.toggle_select_target(Target::Entities);
    // The chair is passed over for the decal under it.
    click(&mut session, (1.5, 1.5));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[][..], &[(GRID, 1)][..]));

    // A box takes only the entities with decals off.
    session.toggle_select_target(Target::Entities);
    session.toggle_select_target(Target::Decals);
    drag(&mut session, (0, 0), (2, 2));
    assert_eq!((&session.selection[..], &session.selected_decals[..]), (&[CHAIR][..], &[][..]));

    // The last one on stays on.
    session.toggle_select_target(Target::Entities);
    assert!(session.select_targets().entities);
    assert_eq!(session.status_line(), "Select needs at least one target");
}

#[test]
fn the_add_and_subtract_modes_do_what_shift_and_ctrl_do() {
    let (mut session, _engine) = selecting();
    session.set_select_mode(SelectMode::Add);
    click(&mut session, (1.5, 1.5));
    click(&mut session, (3.5, 1.5));
    assert_eq!(session.selection, [CHAIR, WALL]);
    session.set_select_mode(SelectMode::Subtract);
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL]);
    // Shift goes over the mode for a press, as Ctrl does.
    session.held.shift = true;
    click(&mut session, (1.5, 1.5));
    assert_eq!(session.selection, [WALL, CHAIR]);
    session.held.shift = false;

    // In a region, Add adds a box.
    session.set_select_mode(SelectMode::Add);
    session.toggle_select_target(Target::Tiles);
    drag(&mut session, (0, 0), (0, 0));
    drag(&mut session, (2, 2), (2, 2));
    assert_eq!(session.region_size(), 2);
}

#[test]
fn free_places_where_the_cursor_is_and_shift_snaps_for_a_press() {
    let (mut session, engine) = Session::offline(&scene());
    choose(&mut session, "SnapgridCenter");
    session.set_entity_free(true);
    click(&mut session, (2.25, 3.75));
    session.held.shift = true;
    click(&mut session, (4.25, 3.75));
    let placed = asked(&engine);
    let at = |ops: &[Value]| (ops[0]["x"].as_f64().unwrap(), ops[0]["y"].as_f64().unwrap());
    assert_eq!([at(&placed[0]), at(&placed[1])], [(2.25, 3.75), (4.5, 3.5)]);
}

#[test]
fn a_row_of_entities_has_no_gaps_and_skips_cells_that_hold_one() {
    let (mut session, engine) = Session::offline(&scene());
    session.set_prototypes(vec![Prototype { id: "Chair".into(), name: "chair".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() }]);
    session.choose_entity(0);
    assert_eq!(session.tool(), Tool::Place);
    // One jump of the cursor from (0, 1) to (3, 1), over the chair on (1, 1).
    drag(&mut session, (0, 1), (3, 1));
    let placed: Vec<(f64, f64)> = asked(&engine).iter().flatten().map(|op| (op["x"].as_f64().unwrap(), op["y"].as_f64().unwrap())).collect();
    assert_eq!(placed, [(0.5, 1.5), (2.5, 1.5), (3.5, 1.5)]);
}

#[test]
fn the_options_are_kept_in_the_settings_file() {
    let folder = std::env::temp_dir().join(format!("editor-core-toolbar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let file = folder.join("settings.json");
    // What else the file holds stays.
    let mut settings = Settings::default();
    settings.animate_sprites = true;
    settings.save(&file).unwrap();

    let (mut session, _engine) = Session::offline(&scene());
    session.tooling = Tooling::load(file.clone());
    tap(&mut session, 'C');
    session.take_bar_tool(BarTool::Erase);
    session.set_shape(Shape::Line);
    session.take_bar_tool(BarTool::Select);
    session.toggle_select_target(Target::Decals);
    session.set_entity_free(true);
    // The mode is not kept.
    session.set_select_mode(SelectMode::Subtract);

    let read = Settings::load(&file);
    assert!(read.animate_sprites);
    let kept = ToolOptions { paint_shape: Shape::Circle, erase_shape: Shape::Line, select: SelectTargets { entities: true, decals: false, tiles: false }, entity_free: true };
    assert_eq!(read.tools, kept);

    // A session reads them back.
    let (mut again, _again_engine) = Session::offline(&scene());
    again.tooling = Tooling::load(file);
    again.take_bar_tool(BarTool::Paint);
    assert_eq!((again.tool(), again.select_mode()), (Tool::Circle, SelectMode::Replace));
    assert_eq!(again.toolbox().options, kept);
    let _ = std::fs::remove_dir_all(&folder);
}
