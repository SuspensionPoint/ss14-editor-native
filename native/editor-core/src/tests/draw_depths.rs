use super::*;

/// Starlight's DrawDepth enum, as its Content.Shared/DrawDepth/DrawDepth.cs numbers it.
const STARLIGHT: &[(&str, i32)] = &[
    ("LowFloors", -22),
    ("ThickPipe", -21),
    ("ThickWire", -20),
    ("ThinPipeAlt4", -19),
    ("ThinPipeAlt3", -18),
    ("ThinPipeAlt2", -17),
    ("ThinPipeAlt1", -16),
    ("ThinPipe", -15),
    ("ThinWire", -14),
    ("BelowFloor", -13),
    ("FloorTiles", -12),
    ("FloorObjects", -11),
    ("Puddles", -10),
    ("HighFloorObjects", -5),
    ("DeadMobs", -4),
    ("SmallMobs", -3),
    ("Walls", -2),
    ("WallTops", -1),
    ("Objects", 0),
    ("SmallObjects", 1),
    ("WallMountedItems", 2),
    ("LargeObjects", 3),
    ("Items", 4),
    ("BelowMobs", 5),
    ("Mobs", 6),
    ("OverMobs", 7),
    ("Doors", 8),
    ("BlastDoors", 9),
    ("Overdoors", 10),
    ("Gasses", 11),
    ("Effects", 12),
    ("Ghosts", 13),
    ("Overlays", 14),
    ("CyberspaceOverlays", 15),
    ("CyberspaceObjects", 100),
];

/// Space Wizards' DrawDepth enum on current master, where walls went down among the floors'
/// neighbours and gaps were left for what a t-ray scanner shows.
const SPACE_WIZARDS: &[(&str, i32)] = &[
    ("LowFloors", -22),
    ("ThickPipe", -21),
    ("ThickWire", -20),
    ("ThinPipeAlt2", -19),
    ("ThinPipeAlt1", -18),
    ("ThinPipe", -17),
    ("ThinWire", -16),
    ("BelowFloor", -15),
    ("FloorTiles", -14),
    ("FloorObjects", -13),
    ("Puddles", -12),
    ("HighFloorObjects", -11),
    ("DeadMobs", -10),
    ("SmallMobs", -9),
    ("Walls", -8),
    ("WallTops", -1),
    ("Objects", 0),
    ("SmallObjects", 1),
    ("WallMountedItems", 2),
    ("LargeObjects", 3),
    ("Items", 4),
    ("BelowMobs", 5),
    ("Mobs", 6),
    ("OverMobs", 7),
    ("Doors", 8),
    ("BlastDoors", 9),
    ("Overdoors", 10),
    ("Gasses", 17),
    ("Effects", 18),
    ("Ghosts", 19),
    ("Overlays", 20),
];

fn depths(names: &[(&str, i32)]) -> Vec<DrawDepth> {
    names.iter().map(|&(name, value)| DrawDepth { name: name.into(), value }).collect()
}

/// The ranges the editor had before it read the fork's names, taken from Starlight.
fn starlight_ranges(depth: i32) -> u32 {
    match depth {
        i32::MIN..=-13 => group::SUBFLOOR,
        -12..=-5 => group::FLOOR_OBJECTS,
        -4..=-1 => group::STRUCTURES,
        8..=10 => group::DOORS,
        _ => group::OBJECTS,
    }
}

#[test]
fn starlight_s_names_give_the_ranges_it_always_had() {
    let groups = DepthGroups::from_depths(&depths(STARLIGHT));
    for depth in (-300..=300).chain([i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX]) {
        assert_eq!(groups.group(depth), starlight_ranges(depth), "depth {depth}");
        assert_eq!(DepthGroups::default().group(depth), starlight_ranges(depth), "depth {depth} with no names told");
    }
}

#[test]
fn space_wizards_walls_are_structures_and_not_floor_objects() {
    let groups = DepthGroups::from_depths(&depths(SPACE_WIZARDS));
    let by_name = |name: &str| groups.group(SPACE_WIZARDS.iter().find(|(named, _)| *named == name).unwrap().1);
    for name in ["LowFloors", "ThickPipe", "ThinWire", "BelowFloor"] {
        assert_eq!(by_name(name), group::SUBFLOOR, "{name}");
    }
    for name in ["FloorTiles", "FloorObjects", "Puddles", "HighFloorObjects"] {
        assert_eq!(by_name(name), group::FLOOR_OBJECTS, "{name}");
    }
    for name in ["DeadMobs", "SmallMobs", "Walls", "WallTops"] {
        assert_eq!(by_name(name), group::STRUCTURES, "{name}");
    }
    for name in ["Doors", "BlastDoors", "Overdoors"] {
        assert_eq!(by_name(name), group::DOORS, "{name}");
    }
    for name in ["Objects", "Items", "Mobs", "OverMobs", "Gasses", "Overlays"] {
        assert_eq!(by_name(name), group::OBJECTS, "{name}");
    }
    // Below every name, and in the gap between the walls and their tops.
    assert_eq!(groups.group(-40), group::SUBFLOOR);
    assert_eq!((-7..=-2).map(|depth| groups.group(depth)).collect::<Vec<_>>(), [group::STRUCTURES; 6]);
    // The gap above the doors lies between two layers, so it is Objects, as is all above.
    assert_eq!((11..=16).map(|depth| groups.group(depth)).collect::<Vec<_>>(), [group::OBJECTS; 6]);
    assert_eq!(groups.group(500), group::OBJECTS);
}

#[test]
fn a_name_the_rule_does_not_know_falls_back_by_where_it_is() {
    let groups = DepthGroups::from_depths(&depths(&[
        ("DeepPlating", -30),
        ("ThinCableTray", -25),
        ("BelowFloor", -20),
        ("FloorTiles", -19),
        ("Walls", -5),
        ("Signs", -4),
        ("Objects", 0),
    ]));
    // Under BelowFloor it lies under the floor. Above it, an unknown name is an object.
    assert_eq!(groups.group(-30), group::SUBFLOOR);
    assert_eq!(groups.group(-25), group::SUBFLOOR);
    assert_eq!(groups.group(-4), group::OBJECTS);
    // The gap between a floor and a wall is neither's.
    assert_eq!(groups.group(-10), group::OBJECTS);
    assert_eq!(groups.group(-5), group::STRUCTURES);
}

#[test]
fn two_names_on_one_number_take_the_one_the_rule_knows() {
    let groups = DepthGroups::from_depths(&depths(&[("BelowFloor", -10), ("Fixtures", -2), ("Walls", -2), ("Objects", 0)]));
    assert_eq!(groups.group(-2), group::STRUCTURES);
    let groups = DepthGroups::from_depths(&depths(&[("BelowFloor", -10), ("Walls", -2), ("Fixtures", -2), ("Objects", 0)]));
    assert_eq!(groups.group(-2), group::STRUCTURES);
}

#[test]
fn no_names_told_leaves_starlight_s() {
    assert_eq!(DepthGroups::from_depths(&[]), DepthGroups::default());
}

#[test]
fn the_engine_s_answer_reads_as_names_and_numbers() {
    let read: Vec<DrawDepth> = serde_json::from_str(r#"[{"name":"Walls","value":-8},{"name":"WallTops","value":-1}]"#).unwrap();
    assert_eq!(read, depths(&[("Walls", -8), ("WallTops", -1)]));
}
