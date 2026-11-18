use super::*;

fn tile(id: &str) -> Tile {
    Tile { id: id.into(), sprite: None, allows_rotation: false, can_crowbar: false }
}

fn prototype(id: &str, name: &str) -> Prototype {
    Prototype { id: id.into(), name: name.into(), ..Default::default() }
}

#[test]
fn the_common_tiles_come_first_in_the_web_editor_s_order() {
    let tiles: Vec<Tile> = ["FloorWood", "Lattice", "FloorAsteroid", "Plating", "FloorBlue", "FloorSteel"].into_iter().map(tile).collect();
    let ids: Vec<&str> = tile_order(&tiles).into_iter().map(|index| tiles[index].id.as_str()).collect();
    // Those the fork lacks are passed over.
    assert_eq!(ids, ["Plating", "FloorSteel", "FloorWood", "Lattice", "FloorAsteroid", "FloorBlue"]);
    assert!(tile_order(&[]).is_empty());
}

#[test]
fn a_category_is_the_folder_a_prototype_is_written_in() {
    assert_eq!(category_of("/Prototypes/Entities/Structures/Power/apc.yml"), "Structures/Power");
    assert_eq!(category_of("Prototypes/Entities/Structures/Doors/Airlocks/base.yml"), "Structures/Doors/Airlocks");
    // A fork's own folder is passed over.
    assert_eq!(category_of("Prototypes\\_Starlight\\Entities\\Markers\\spawners.yml"), "Markers");
    assert_eq!(category_of("Prototypes/Catalog/Fills/Lockers/heads.yml"), "Catalog/Fills/Lockers");
    assert_eq!(category_of("Prototypes/_ST/Catalog/Fills/crates.yml"), "Catalog/Fills");
    // A file straight in Entities, one in a fork folder without the `_`, and any other is Other.
    assert_eq!(category_of("Prototypes/Entities/base.yml"), "Other");
    assert_eq!(category_of("Prototypes/DeltaV/Entities/Mobs/cat.yml"), "Other");
    assert_eq!(category_of("Prototypes/tiles.yml"), "Other");
    assert_eq!(category_of("Maps/cork.yml"), "Other");
}

#[test]
fn the_ids_of_a_file_s_entities_are_read_from_its_items() {
    let text = "\
# A comment
- type: entity
  parent: BaseWallmount
  id: BaseAPC # the base
  components:
  - type: Sprite
id: NotThis
- id: APCBasic
  type: entity
  name: APC
- type: tile
  id: FloorSteel
-   type: entity
- type: entity
  id: \"Quoted\"
";
    assert_eq!(entity_ids(text), ["BaseAPC", "APCBasic", "Quoted"]);
    assert!(entity_ids("").is_empty());
}

#[test]
fn the_scan_finds_each_id_s_category_from_its_file() {
    let fork = std::env::temp_dir().join(format!("editor-core-palette-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fork);
    let power = fork.join("Resources/Prototypes/Entities/Structures/Power");
    let other = fork.join("Resources/Prototypes/Recipes");
    std::fs::create_dir_all(&power).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(power.join("apc.yml"), "- type: entity\n  id: APC\n").unwrap();
    std::fs::write(other.join("odd.yml"), "- type: entity\n  id: Odd\n").unwrap();

    let found = scan_categories(&fork);
    assert_eq!(found.get("APC").map(String::as_str), Some("Structures/Power"));
    // A file outside Entities and Catalog is not read.
    assert_eq!(found.get("Odd"), None);
    let _ = std::fs::remove_dir_all(&fork);
}

#[test]
fn the_tree_puts_the_common_categories_first_and_each_by_name() {
    let mut hidden = prototype("MarkerHidden", "hidden");
    hidden.hide_spawn_menu = true;
    let prototypes = vec![prototype("Zeta", "zeta"), prototype("APC", "APC"), prototype("Airlock", "airlock"), prototype("Alpha", "Alpha"), hidden, prototype("Loose", "loose")];
    let categories: HashMap<String, String> = [("Zeta", "Markers"), ("APC", "Structures/Power"), ("Airlock", "Structures/Doors"), ("Alpha", "Markers"), ("MarkerHidden", "Markers"), ("Loose", "Objects/Misc")]
        .into_iter()
        .map(|(id, category)| (id.to_owned(), category.to_owned()))
        .collect();
    let mut tree = EntityTree::build(&prototypes, &categories);
    let paths: Vec<&str> = tree.categories.iter().map(|category| category.path.as_str()).collect();
    assert_eq!(paths, ["Structures/Doors", "Structures/Power", "Markers", "Objects/Misc"]);
    // By name, in either case, and only what can be placed.
    assert_eq!(tree.categories[2].entries, [3, 0]);
    assert_eq!(tree.categories[1].short_name(), "Power");
    assert_eq!(tree.category_holding(1), Some(1));
    assert_eq!(tree.category_holding(4), None);

    // A prototype whose file was not read is Other.
    tree = EntityTree::build(&prototypes, &HashMap::new());
    assert_eq!(tree.categories.len(), 1);
    assert_eq!((tree.categories[0].path.as_str(), tree.categories[0].entries.len()), ("Other", 5));
}

#[test]
fn the_rows_open_only_the_open_categories_and_the_search_is_one_list() {
    let mut chair = prototype("Chair", "chair");
    chair.suffix = Some("Wooden".into());
    let prototypes = vec![chair, prototype("ChairOffice", "office chair"), prototype("Table", "table")];
    let categories: HashMap<String, String> = [("Chair", "Furniture"), ("ChairOffice", "Furniture"), ("Table", "Tables")].into_iter().map(|(id, category)| (id.to_owned(), category.to_owned())).collect();
    let tree = EntityTree::build(&prototypes, &categories);

    assert_eq!(tree.rows(&HashSet::new()), [TreeRow::Category(0, false), TreeRow::Category(1, false)]);
    let open: HashSet<String> = ["Furniture".to_owned()].into();
    assert_eq!(tree.rows(&open), [TreeRow::Category(0, true), TreeRow::Entity(0), TreeRow::Entity(1), TreeRow::Category(1, false)]);

    // Every word in the id, the name or the suffix, across the categories, by name.
    assert_eq!(tree.search(&prototypes, &["chair"]), [0, 1]);
    assert_eq!(tree.search(&prototypes, &["WOODEN"]), [0]);
    assert_eq!(tree.search(&prototypes, &["office", "chair"]), [1]);
    assert_eq!(tree.search(&prototypes, &["chair", "table"]), Vec::<usize>::new());
}
