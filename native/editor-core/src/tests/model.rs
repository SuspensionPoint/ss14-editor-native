use super::*;

const GRID: i32 = 2;
const LOCKER: i32 = 11;
const CROWBAR: i32 = 12;
const BACKPACK: i32 = 40;

const HISTORY: &str = r#"{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}"#;

/// A decal as the kernel writes one.
fn decal(id: i32, prototype: &str, colour: &str) -> String {
    format!(r#"{{"id":{id},"prototype":"{prototype}","x":1,"y":2,"color":{colour},"angle":0,"zIndex":3,"cleanable":false}}"#)
}

/// A grid with no tiles as the kernel writes one, with these decals.
fn grid(uid: i32, decals: &[String]) -> String {
    format!(r#"{{"uid":{uid},"yamlUid":{uid},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space"],"tiles":[],"decals":[{}]}}"#, decals.join(","))
}

/// A grid as the kernel writes one, with these decals and one tile of steel on cell (0, 0).
fn floored_grid(uid: i32, decals: &[String]) -> String {
    grid(uid, decals).replace(r#""tileIds":["Space"],"tiles":[]"#, r#""tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0]"#)
}

/// One grid with a red arrow and a patch of dirt on it. A locker and a backpack stand on the
/// grid. A crowbar is in the locker and five pens are in the backpack.
fn doc() -> Doc {
    let entity = |uid: i32, prototype: &str, parent: i32, contained: bool| {
        format!(r#"{{"uid":{uid},"yamlUid":{uid},"prototype":"{prototype}","parent":{parent},"x":0.5,"y":0.5,"rotation":0,"anchored":false,"contained":{contained},"name":null}}"#)
    };
    let dirt = r#"{"id":9,"prototype":"Dirt","x":4.25,"y":-0.5,"color":null,"angle":-1.5707963267948966,"zIndex":0,"cleanable":true}"#;
    let entities = [
        entity(44, "Pen", BACKPACK, true),
        entity(LOCKER, "LockerBotanist", GRID, false),
        entity(41, "Pen", BACKPACK, true),
        entity(45, "Pen", BACKPACK, true),
        entity(CROWBAR, "Crowbar", LOCKER, true),
        entity(BACKPACK, "ClothingBackpack", GRID, false),
        entity(43, "Pen", BACKPACK, true),
        entity(42, "Pen", BACKPACK, true),
    ];
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"t.yml","category":"Map","formatVersion":7,"engineVersion":"290.0.0","mapUid":1,"entityCount":8,"loadMilliseconds":0,"log":[]}},
            "grids":[{}],"entities":[{}],"history":{HISTORY}}}"#,
        grid(GRID, &[decal(7, "Arrows", r##""#FF0000FF""##), dirt.to_owned()]),
        entities.join(",")
    );
    Doc::from_scene(serde_json::from_str(&scene).unwrap(), None)
}

/// What the kernel says a command changed, with these grids and these changes to decals.
fn delta(grids: &[String], decals: &[String]) -> Delta {
    delta_removing(&[], grids, decals)
}

/// The same, with these entities gone as well.
fn delta_removing(removed: &[i32], grids: &[String], decals: &[String]) -> Delta {
    let delta = format!(
        r#"{{"document":1,"tiles":[],"entities":[],"removed":{removed:?},"gridsChanged":[{}],"history":{HISTORY},"created":[],"decals":[{}],"createdDecals":[],"details":[],"log":[]}}"#,
        grids.join(","),
        decals.join(",")
    );
    serde_json::from_str(&delta).unwrap()
}

/// A change to one decal as the kernel writes it. No decal says it was removed.
fn change(grid: i32, id: i32, decal: Option<String>) -> String {
    format!(r#"{{"grid":{grid},"id":{id},"decal":{}}}"#, decal.unwrap_or_else(|| "null".into()))
}

/// The ids of a grid's decals, in order.
fn decal_ids(doc: &Doc, grid: i32) -> Vec<i32> {
    let mut ids: Vec<i32> = doc.grid(grid).unwrap().decals.keys().copied().collect();
    ids.sort_unstable();
    ids
}

fn uids(entities: Vec<&Entity>) -> Vec<i32> {
    entities.iter().map(|entity| entity.uid).collect()
}

#[test]
fn a_grid_keeps_its_decals() {
    let doc = doc();
    let decals = &doc.grid(GRID).unwrap().decals;

    assert_eq!(decal_ids(&doc, GRID), [7, 9]);
    let arrow = &decals[&7];
    assert_eq!((arrow.prototype.as_str(), arrow.z_index, arrow.color.as_deref()), ("Arrows", 3, Some("#FF0000FF")));
    // A decal with no colour of its own, off the middle of a tile and turned a quarter.
    let dirt = Decal { id: 9, prototype: "Dirt".into(), x: 4.25, y: -0.5, color: None, angle: -std::f64::consts::FRAC_PI_2, z_index: 0, cleanable: true };
    assert_eq!(decals[&9], dirt);
}

#[test]
fn a_delta_adds_replaces_and_removes_decals() {
    let mut doc = doc();
    doc.apply(delta(&[], &[
        change(GRID, 8, Some(decal(8, "WarnLineN", "null"))),
        change(GRID, 7, Some(decal(7, "Arrows", r##""#00FF00FF""##))),
        // A grid the mirror does not have: nothing comes of either.
        change(99, 1, Some(decal(1, "Arrows", "null"))),
        change(99, 2, None),
    ]));

    assert_eq!(decal_ids(&doc, GRID), [7, 8, 9]);
    let decals = &doc.grid(GRID).unwrap().decals;
    assert_eq!((decals[&8].prototype.as_str(), decals[&7].color.as_deref()), ("WarnLineN", Some("#00FF00FF")));
    assert_eq!(doc.grids.len(), 1);

    // Taking away one that is there, and one that is not.
    doc.apply(delta(&[], &[change(GRID, 7, None), change(GRID, 70, None)]));
    assert_eq!(decal_ids(&doc, GRID), [8, 9]);
}

#[test]
fn a_grid_that_arrives_in_a_delta_brings_its_decals() {
    let mut doc = doc();
    doc.apply(delta(&[grid(5, &[decal(1, "WarnLineN", "null")])], &[]));
    assert_eq!((decal_ids(&doc, 5), decal_ids(&doc, GRID)), (vec![1], vec![7, 9]));

    // A grid that is known arrives whole, so the decals it had and does not bring are gone.
    doc.apply(delta(&[grid(GRID, &[decal(3, "Arrows", "null")])], &[]));
    assert_eq!((decal_ids(&doc, 5), decal_ids(&doc, GRID)), (vec![1], vec![3]));
}

#[test]
fn a_removed_grid_takes_its_tiles_and_decals_with_it() {
    let mut doc = doc();
    doc.apply(delta(&[floored_grid(5, &[decal(1, "WarnLineN", "null")])], &[]));
    assert_eq!((doc.tile_at(5, (0, 0)), decal_ids(&doc, 5)), ("FloorSteel", vec![1]));

    // The grid goes, and the locker on the other grid with it.
    doc.apply(delta_removing(&[5, LOCKER], &[], &[]));
    assert!(doc.grid(5).is_none() && doc.grid_with_tile_at((0.5, 0.5)).is_none());
    assert_eq!(doc.tile_at(5, (0, 0)), SPACE);
    assert!(!doc.entities.contains_key(&LOCKER));
    // The grid that stays is as it was.
    assert_eq!((doc.grids.len(), decal_ids(&doc, GRID)), (1, vec![7, 9]));

    // It comes back, as an undo brings it, with what the engine says it has now.
    doc.apply(delta(&[floored_grid(5, &[decal(2, "Arrows", "null")])], &[]));
    assert_eq!((doc.tile_at(5, (0, 0)), decal_ids(&doc, 5)), ("FloorSteel", vec![2]));
}

#[test]
fn a_delta_may_remove_what_the_mirror_never_knew() {
    // The engine can make and destroy an entity within one edit.
    let mut doc = doc();
    doc.apply(delta_removing(&[999, LOCKER], &[], &[]));

    assert_eq!((doc.entities.len(), doc.entities.contains_key(&LOCKER)), (7, false));
    assert_eq!((doc.grids.len(), decal_ids(&doc, GRID)), (1, vec![7, 9]));
}

#[test]
fn a_grid_a_delta_both_removes_and_brings_is_kept_as_brought() {
    let mut doc = doc();
    doc.apply(delta_removing(&[GRID], &[grid(GRID, &[decal(3, "Arrows", "null")])], &[]));

    assert_eq!(decal_ids(&doc, GRID), [3]);
}

/// The locker, the backpack, the crowbar and the five pens, as the engine lists them when
/// the grid goes: what each holds before it, and the grid last.
const ON_THE_GRID: [i32; 9] = [CROWBAR, LOCKER, 41, 42, 43, 44, 45, BACKPACK, GRID];

#[test]
fn a_removed_grid_goes_with_all_on_it_in_any_order() {
    let mut ordered = doc();
    assert_eq!(ordered.entities_on(GRID), 8);
    ordered.apply(delta_removing(&ON_THE_GRID, &[], &[]));

    // The mirror does not count on the order: the holders first comes to the same.
    let mut reversed = doc();
    let mut backwards = ON_THE_GRID;
    backwards.reverse();
    reversed.apply(delta_removing(&backwards, &[], &[]));

    for doc in [&ordered, &reversed] {
        assert!(doc.entities.is_empty() && doc.grids.is_empty());
        assert_eq!((doc.active_grid, doc.entities_on(GRID)), (0, 0));
    }
}

#[test]
fn undoing_a_grid_removal_brings_it_back_whole() {
    let mut doc = doc();
    doc.apply(delta_removing(&ON_THE_GRID, &[], &[]));

    // The undo: the grid in gridsChanged, and every entity on it in entities.
    let entity = |uid: i32, prototype: &str, parent: i32, contained: bool| {
        format!(r#"{{"uid":{uid},"yamlUid":{uid},"prototype":"{prototype}","parent":{parent},"x":0.5,"y":0.5,"rotation":0,"anchored":false,"contained":{contained},"name":null}}"#)
    };
    let entities = [entity(LOCKER, "LockerBotanist", GRID, false), entity(CROWBAR, "Crowbar", LOCKER, true)];
    let undo = format!(
        r#"{{"tiles":[],"entities":[{}],"removed":[],"gridsChanged":[{}],"history":{HISTORY},"created":[],"linked":[]}}"#,
        entities.join(","),
        floored_grid(GRID, &[decal(7, "Arrows", "null")])
    );
    doc.apply(serde_json::from_str(&undo).unwrap());

    assert_eq!((doc.tile_at(GRID, (0, 0)), decal_ids(&doc, GRID)), ("FloorSteel", vec![7]));
    assert_eq!(uids(doc.contents(LOCKER)), [CROWBAR]);
    assert_eq!((doc.entities_on(GRID), doc.place_of(CROWBAR).map(|place| place.0)), (2, Some(GRID)));
    // The grid is the tile tools' grid again, as the only one there is.
    assert_eq!(doc.active_grid, GRID);
}

#[test]
fn a_removed_grid_hands_the_tile_tools_to_the_largest_left() {
    let mut doc = doc();
    doc.apply(delta(&[floored_grid(5, &[])], &[]));
    assert_eq!(doc.active_grid, GRID);

    doc.apply(delta_removing(&ON_THE_GRID, &[], &[]));
    assert_eq!(doc.active_grid, 5);
}

#[test]
fn a_grid_centre_is_the_middle_of_its_tiles_or_its_origin() {
    let mut doc = doc();
    // No tiles: its origin.
    assert_eq!(doc.grid_centre(GRID), Some((0.0, 0.0)));
    let json = grid(5, &[]).replace(r#""x":0,"y":0,"rotation":0,"tileIds":["Space"],"tiles":[]"#, r#""x":10,"y":-4,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0,3,1,1,0,0]"#);
    doc.apply(delta(&[json], &[]));
    assert_eq!(doc.grid_centre(5), Some((12.0, -3.0)));
    assert_eq!(doc.grid_centre(99), None);
}

#[test]
fn a_delta_s_links_are_read() {
    let json = format!(
        r#"{{"tiles":[],"entities":[],"removed":[],"gridsChanged":[],"history":{HISTORY},"created":[],
            "linked":[{{"uid":18879,"links":[{{"source":18879,"sourcePort":"Pressed","sink":18832,"sinkPort":"Toggle"}}],"devices":[]}},{{"uid":18700,"links":[],"devices":[18650]}}]}}"#
    );
    let delta: Delta = serde_json::from_str(&json).unwrap();
    let link = Link { source: 18879, source_port: "Pressed".into(), sink: 18832, sink_port: "Toggle".into() };
    assert_eq!(delta.linked, [EntityLinks { uid: 18879, links: vec![link], devices: Vec::new() }, EntityLinks { uid: 18700, links: Vec::new(), devices: vec![18650] }]);
}

#[test]
fn a_scene_and_a_delta_from_an_engine_that_tells_no_decals_still_read() {
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"t.yml","category":"Map","mapUid":1}},
            "grids":[{{"uid":{GRID},"name":"grid","x":0,"y":0,"rotation":0,"tileIds":["Space","FloorSteel"],"tiles":[0,0,1,0,0]}}],"entities":[],"history":{HISTORY}}}"#
    );
    let mut doc = Doc::from_scene(serde_json::from_str(&scene).unwrap(), None);
    // A tile change with no variant and no turn, as an older engine sends it.
    let delta = format!(
        r#"{{"tiles":[{{"grid":{GRID},"x":1,"y":0,"tile":"FloorWood"}}],"entities":[],"removed":[],"gridsChanged":[],"history":{HISTORY},"created":[]}}"#
    );
    doc.apply(serde_json::from_str(&delta).unwrap());

    assert!(decal_ids(&doc, GRID).is_empty());
    let tiles = &doc.grid(GRID).unwrap().tiles;
    assert_eq!((doc.tile_at(GRID, (0, 0)), tiles[&(0, 0)].variant, tiles[&(0, 0)].rotation_mirroring), ("FloorSteel", 0, 0));
    assert_eq!((doc.tile_at(GRID, (1, 0)), tiles[&(1, 0)].variant, tiles[&(1, 0)].rotation_mirroring), ("FloorWood", 0, 0));
}

#[test]
fn a_grid_keeps_each_tile_s_variant_and_turn() {
    // Steel on (0, 0) in its third variant, mirrored and turned. Wood on (1, 0) plain.
    let json = grid(GRID, &[]).replace(r#""tileIds":["Space"],"tiles":[]"#, r#""tileIds":["Space","FloorSteel","FloorWood"],"tiles":[0,0,1,2,5,1,0,2,0,0]"#);
    let mut doc = doc();
    doc.apply(delta(&[json], &[]));

    let (steel, wood) = (doc.tile_index("FloorSteel"), doc.tile_index("FloorWood"));
    let tiles = &doc.grid(GRID).unwrap().tiles;
    assert_eq!(tiles[&(0, 0)], GridTile { id: steel, variant: 2, rotation_mirroring: 5 });
    assert_eq!(tiles[&(1, 0)], GridTile { id: wood, variant: 0, rotation_mirroring: 0 });
    assert_eq!((doc.tile_at(GRID, (0, 0)), doc.tile_at(GRID, (1, 0))), ("FloorSteel", "FloorWood"));
}

#[test]
fn a_tile_change_brings_its_variant_and_turn() {
    let mut doc = doc();
    let delta = format!(
        r#"{{"tiles":[{{"grid":{GRID},"x":3,"y":-1,"tile":"FloorSteel","variant":3,"rotationMirroring":6}},
                      {{"grid":{GRID},"x":4,"y":-1,"tile":"FloorSteel","variant":1,"rotationMirroring":0}}],
            "entities":[],"removed":[],"gridsChanged":[],"history":{HISTORY},"created":[]}}"#
    );
    doc.apply(serde_json::from_str(&delta).unwrap());

    let steel = doc.tile_index("FloorSteel");
    let tiles = &doc.grid(GRID).unwrap().tiles;
    assert_eq!(tiles[&(3, -1)], GridTile { id: steel, variant: 3, rotation_mirroring: 6 });
    assert_eq!(tiles[&(4, -1)], GridTile { id: steel, variant: 1, rotation_mirroring: 0 });

    // Space takes the tile away, whatever its variant and turn.
    let space = format!(r#"{{"tiles":[{{"grid":{GRID},"x":3,"y":-1,"tile":"Space","variant":0,"rotationMirroring":0}}],"entities":[],"removed":[],"gridsChanged":[],"history":{HISTORY},"created":[]}}"#);
    doc.apply(serde_json::from_str(&space).unwrap());
    assert_eq!((doc.tile_at(GRID, (3, -1)), doc.grid(GRID).unwrap().tiles.len()), (SPACE, 1));
}

#[test]
fn a_container_knows_what_it_holds() {
    assert_eq!(uids(doc().contents(LOCKER)), [CROWBAR]);
}

#[test]
fn contents_are_the_contained_children_only_in_the_order_of_their_uids() {
    let doc = doc();

    assert_eq!(uids(doc.contents(BACKPACK)), [41, 42, 43, 44, 45]);
    // The locker and the backpack have the grid for a parent and are not inside it.
    assert!(doc.contents(GRID).is_empty());
    assert!(doc.contents(CROWBAR).is_empty());
    assert!(doc.contents(99).is_empty());
}

#[test]
fn a_prototype_carries_its_tags_and_components() {
    let prototype: Prototype = serde_json::from_str(
        r#"{"id":"WallSolid","name":"solid wall","description":"Keeps the air in and the greytide out.","suffix":null,"parents":["BaseWall"],
            "categories":["Structures"],"abstract":false,"hideSpawnMenu":false,"placementMode":"SnapgridCenter",
            "components":["Occluder","Sprite","Tag"],"tags":["Wall"]}"#,
    )
    .unwrap();

    assert!(prototype.tags.iter().any(|tag| &**tag == "Wall") && prototype.has("Occluder"));
    assert!(!prototype.has("Wall") && !prototype.has("Occlude"));
    assert_eq!((prototype.description.as_str(), &prototype.categories[..]), ("Keeps the air in and the greytide out.", &[Arc::<str>::from("Structures")][..]));
}

#[test]
fn a_prototype_from_an_engine_that_tells_less_still_reads() {
    let prototype: Prototype =
        serde_json::from_str(r#"{"id":"WallSolid","name":"solid wall","suffix":null,"abstract":false,"hideSpawnMenu":false,"placementMode":"SnapgridCenter"}"#).unwrap();

    let known = Prototype { id: "WallSolid".into(), name: "solid wall".into(), placement_mode: "SnapgridCenter".into(), ..Default::default() };
    assert_eq!(prototype, known);
    assert!(prototype.description.is_empty() && prototype.categories.is_empty() && prototype.components.is_empty() && prototype.tags.is_empty());
    assert!(!prototype.has("Occluder"));
}

#[test]
fn a_decal_prototype_reads_as_the_kernel_sends_it() {
    let decals: Vec<DecalPrototype> = serde_json::from_str(
        r#"[{"id":"Arrows","spritePath":"/Textures/Decals/markings.rsi","spriteState":"arrows","tags":["station","markings"],
             "showMenu":true,"snapCardinals":false,"defaultCustomColor":false,"defaultSnap":true,"defaultCleanable":false},
            {"id":"Bare","spritePath":null,"spriteState":null,"tags":[],
             "showMenu":false,"snapCardinals":true,"defaultCustomColor":true,"defaultSnap":false,"defaultCleanable":true}]"#,
    )
    .unwrap();

    let arrows = DecalPrototype {
        id: "Arrows".into(),
        sprite_path: Some("/Textures/Decals/markings.rsi".into()),
        sprite_state: Some("arrows".into()),
        // As the prototype has them: the first is the group a palette shows it under.
        tags: vec!["station".into(), "markings".into()],
        show_menu: true,
        snap_cardinals: false,
        default_custom_color: false,
        default_snap: true,
        default_cleanable: false,
    };
    let bare = DecalPrototype {
        id: "Bare".into(),
        sprite_path: None,
        sprite_state: None,
        tags: Vec::new(),
        show_menu: false,
        snap_cardinals: true,
        default_custom_color: true,
        default_snap: false,
        default_cleanable: true,
    };
    assert_eq!(decals, [arrows, bare]);
}

#[test]
fn a_decal_prototype_names_the_image_file_of_its_picture() {
    let picture = |path: Option<&str>, state: Option<&str>| DecalPrototype { sprite_path: path.map(str::to_owned), sprite_state: state.map(str::to_owned), ..Default::default() }.file();

    assert_eq!(picture(Some("/Textures/Decals/markings.rsi"), Some("arrows")).as_deref(), Some("/Textures/Decals/markings.rsi/arrows.png"));
    assert_eq!(picture(Some("/Textures/Decals/stain.png"), None).as_deref(), Some("/Textures/Decals/stain.png"));
    assert_eq!(picture(None, None), None);
}
