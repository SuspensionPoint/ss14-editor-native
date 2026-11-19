use super::*;

/// Every session tool, for checks that go through them all.
const EVERY: [Tool; 15] = [
    Tool::Pan,
    Tool::Paint,
    Tool::Erase,
    Tool::Pick,
    Tool::Fill,
    Tool::Rectangle,
    Tool::Line,
    Tool::Circle,
    Tool::Room,
    Tool::Area,
    Tool::Select,
    Tool::Place,
    Tool::Link,
    Tool::Cable,
    Tool::Pipe,
];

#[test]
fn no_two_toolbar_tools_share_a_key_and_every_key_is_handed_on() {
    let mut keys: Vec<char> = BarTool::ALL.iter().map(|tool| tool.key()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), BarTool::ALL.len());
    for key in keys {
        assert!(TOOL_KEYS.contains(&key), "{key}");
    }
    // Every key handed on does something, and no other letter does.
    for key in 'A'..='Z' {
        assert_eq!(key_action(key).is_some(), TOOL_KEYS.contains(&key), "{key}");
    }
}

#[test]
fn the_keys_mappers_knew_keep_what_they_did() {
    assert_eq!(key_action('b'), Some(KeyAction::Take(BarTool::Paint)));
    assert_eq!(key_action('E'), Some(KeyAction::Take(BarTool::Erase)));
    assert_eq!(key_action('G'), Some(KeyAction::Take(BarTool::Fill)));
    assert_eq!(key_action('I'), Some(KeyAction::Take(BarTool::Pick)));
    assert_eq!(key_action('O'), Some(KeyAction::Take(BarTool::Room)));
    assert_eq!(key_action('H'), Some(KeyAction::Take(BarTool::Pan)));
    assert_eq!(key_action('D'), Some(KeyAction::Take(BarTool::Link)));
    assert_eq!(key_action('K'), Some(KeyAction::Take(BarTool::Cables)));
    assert_eq!(key_action('J'), Some(KeyAction::Take(BarTool::Pipes)));
    assert_eq!(key_action('V'), Some(KeyAction::SelectThings));
    assert_eq!(key_action('S'), Some(KeyAction::SelectRegion));
    assert_eq!(key_action('P'), Some(KeyAction::PlaceEntities));
    assert_eq!(key_action('R'), Some(KeyAction::PaintShape(Shape::Rectangle)));
    assert_eq!(key_action('L'), Some(KeyAction::PaintShape(Shape::Line)));
    assert_eq!(key_action('C'), Some(KeyAction::PaintShape(Shape::Circle)));
}

#[test]
fn the_toolbar_has_three_groups_in_order() {
    let groups: Vec<usize> = BarTool::ALL.iter().map(|tool| tool.group()).collect();
    assert_eq!(groups, [0, 0, 0, 1, 1, 1, 1, 2, 2, 2]);
}

#[test]
fn paint_at_a_point_places_entities_and_paints_tiles_and_decals() {
    let mut toolbox = Toolbox::default();
    for (target, tool) in [(Target::Tiles, Tool::Paint), (Target::Entities, Tool::Place), (Target::Decals, Tool::Paint)] {
        toolbox.target = target;
        assert_eq!(toolbox.tool_for(BarTool::Paint), tool, "{target:?}");
    }
}

#[test]
fn paint_in_a_shape_is_the_shape_tool_whatever_the_target() {
    let mut toolbox = Toolbox::default();
    for (shape, tool) in [(Shape::Line, Tool::Line), (Shape::Rectangle, Tool::Rectangle), (Shape::Circle, Tool::Circle)] {
        toolbox.set_shape(BarTool::Paint, shape);
        for target in Target::ALL {
            toolbox.target = target;
            assert_eq!(toolbox.tool_for(BarTool::Paint), tool, "{shape:?} {target:?}");
        }
    }
}

#[test]
fn erase_is_the_eraser_in_every_shape() {
    let mut toolbox = Toolbox::default();
    for shape in Shape::ALL {
        assert!(toolbox.set_shape(BarTool::Erase, shape));
        assert_eq!(toolbox.tool_for(BarTool::Erase), Tool::Erase);
        assert_eq!(toolbox.shape(BarTool::Erase), Some(shape));
    }
    // Paint kept its own.
    assert_eq!(toolbox.shape(BarTool::Paint), Some(Shape::Point));
    // A tool without a shape takes none.
    assert!(!toolbox.set_shape(BarTool::Fill, Shape::Line));
    assert_eq!(toolbox.shape(BarTool::Fill), None);
}

#[test]
fn select_with_tiles_is_the_region_and_without_is_the_entity_select() {
    let mut toolbox = Toolbox::default();
    assert_eq!(toolbox.tool_for(BarTool::Select), Tool::Select);
    assert!(toolbox.options.select.toggle(Target::Tiles));
    assert_eq!(toolbox.tool_for(BarTool::Select), Tool::Area);
}

#[test]
fn the_last_select_target_stays_on() {
    let mut targets = SelectTargets::default();
    assert!(targets.toggle(Target::Entities));
    assert!(!targets.toggle(Target::Decals));
    assert_eq!(targets, SelectTargets { entities: false, decals: true, tiles: false });
    // Another on first lets it go.
    assert!(targets.toggle(Target::Tiles));
    assert!(targets.toggle(Target::Decals));
    assert_eq!(targets, SelectTargets { entities: false, decals: false, tiles: true });
    assert!(targets.has(Target::Tiles) && !targets.has(Target::Decals));
}

#[test]
fn the_keys_go_over_the_mode_ctrl_first() {
    for mode in SelectMode::ALL {
        assert_eq!(mode.with_keys(false, false), mode);
        assert_eq!(mode.with_keys(true, false), SelectMode::Add);
        assert_eq!(mode.with_keys(false, true), SelectMode::Subtract);
        assert_eq!(mode.with_keys(true, true), SelectMode::Subtract);
    }
}

#[test]
fn every_session_tool_stands_for_the_toolbar_tool_it_comes_back_as() {
    // A session tool taken some other way, noted, is what the toolbar tool stands for.
    for tool in EVERY {
        let mut toolbox = Toolbox::default();
        toolbox.note_tool(tool);
        let bar = BarTool::of(tool);
        assert_eq!(toolbox.tool_for(bar), tool, "{tool:?}");
    }
}

#[test]
fn noting_a_tool_keeps_what_it_says_and_nothing_more() {
    let mut toolbox = Toolbox::default();
    toolbox.note_tool(Tool::Circle);
    assert_eq!((toolbox.options.paint_shape, toolbox.target), (Shape::Circle, Target::Tiles));
    toolbox.note_tool(Tool::Place);
    assert_eq!((toolbox.options.paint_shape, toolbox.target), (Shape::Point, Target::Entities));
    toolbox.note_tool(Tool::Area);
    assert!(toolbox.options.select.tiles);
    // The select tool with neither entities nor decals on gets both back.
    toolbox.options.select = SelectTargets { entities: false, decals: false, tiles: true };
    toolbox.note_tool(Tool::Select);
    assert_eq!(toolbox.options.select, SelectTargets::default());
    // The others leave the options alone.
    let before = toolbox.clone();
    for tool in [Tool::Pan, Tool::Erase, Tool::Pick, Tool::Fill, Tool::Room, Tool::Link, Tool::Cable, Tool::Pipe] {
        toolbox.note_tool(tool);
        assert_eq!(toolbox, before, "{tool:?}");
    }
}

#[test]
fn each_target_gives_back_what_was_last_in_hand_for_it() {
    let mut toolbox = Toolbox::default();
    for target in Target::ALL {
        assert_eq!(toolbox.recall(target), Palette::None);
    }
    toolbox.remember(&Palette::Tile("FloorWood".into()));
    toolbox.remember(&Palette::Entity(3));
    toolbox.remember(&Palette::Decal("Arrows".into()));
    assert_eq!(toolbox.target, Target::Decals);
    toolbox.remember(&Palette::Tile("FloorSteel".into()));
    assert_eq!(toolbox.target, Target::Tiles);
    assert_eq!(toolbox.recall(Target::Tiles), Palette::Tile("FloorSteel".into()));
    assert_eq!(toolbox.recall(Target::Entities), Palette::Entity(3));
    assert_eq!(toolbox.recall(Target::Decals), Palette::Decal("Arrows".into()));
    // An empty palette leaves the target as it was.
    toolbox.remember(&Palette::None);
    assert_eq!(toolbox.target, Target::Tiles);
}

#[test]
fn the_options_read_back_from_their_settings_text() {
    let options = ToolOptions { paint_shape: Shape::Circle, erase_shape: Shape::Rectangle, select: SelectTargets { entities: false, decals: true, tiles: true }, entity_free: true };
    let text = serde_json::to_string(&options).unwrap();
    assert!(text.contains("\"circle\""), "{text}");
    assert_eq!(serde_json::from_str::<ToolOptions>(&text).unwrap(), options);
    // Missing fields are the defaults.
    assert_eq!(serde_json::from_str::<ToolOptions>("{}").unwrap(), ToolOptions::default());
}
