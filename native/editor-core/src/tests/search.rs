use super::*;

const GRID: i32 = 2;
const MOVED_GRID: i32 = 3;

/// An entity as the kernel writes one.
fn entity(uid: i32, yaml_uid: i32, prototype: &str, parent: i32, at: (f32, f32), contained: bool, name: Option<&str>) -> String {
    let name = name.map_or("null".to_owned(), |name| format!("\"{name}\""));
    format!(
        r#"{{"uid":{uid},"yamlUid":{yaml_uid},"prototype":"{prototype}","parent":{parent},"x":{},"y":{},"rotation":0,"anchored":false,"contained":{contained},"name":{name}}}"#,
        at.0, at.1
    )
}

/// Two grids, the second ten cells right of the origin. On the first a fire extinguisher
/// cabinet on cell (2, 3), a locker on cell (5, 1) with an extinguisher in it, and a
/// chair someone named. On the second, a second extinguisher on cell (1, 1), made since
/// the map was opened. A crate floats on the map itself.
fn doc() -> Doc {
    let grid = |uid: i32, x: f32| format!(r#"{{"uid":{uid},"name":"g","x":{x},"y":0,"rotation":0,"tileIds":["Space"],"tiles":[]}}"#);
    let entities = [
        entity(30, 300, "FireExtinguisher", 21, (0.0, 0.0), true, None),
        entity(20, 200, "ExtinguisherCabinetFilled", GRID, (2.5, 3.5), false, None),
        entity(21, 201, "LockerFilled", GRID, (5.5, 1.5), false, None),
        entity(40, 0, "FireExtinguisher", MOVED_GRID, (1.5, 1.5), false, None),
        entity(22, 202, "Chair", GRID, (0.5, 0.5), false, Some("Captain's chair")),
        entity(50, 500, "CrateGeneric", 1, (-4.5, 7.25), false, None),
    ];
    let scene = format!(
        r#"{{"document":{{"id":1,"source":"t.yml","category":"Map","mapUid":1}},"grids":[{},{}],"entities":[{}],
            "history":{{"canUndo":false,"canRedo":false,"revision":0,"undoLabel":null,"redoLabel":null}}}}"#,
        grid(GRID, 0.0),
        grid(MOVED_GRID, 10.0),
        entities.join(",")
    );
    Doc::from_scene(serde_json::from_str(&scene).unwrap(), None)
}

/// The prototypes' names, as the engine tells them.
fn name_of(id: &str) -> Option<&'static str> {
    match id {
        "FireExtinguisher" => Some("fire extinguisher"),
        "ExtinguisherCabinetFilled" => Some("extinguisher cabinet"),
        "LockerFilled" => Some("locker"),
        "Chair" => Some("chair"),
        _ => None,
    }
}

fn uids(found: &[Found]) -> Vec<i32> {
    found.iter().map(|found| found.uid).collect()
}

fn search(query: &str) -> Vec<Found> {
    find_entities(&doc(), query, name_of)
}

#[test]
fn a_search_matches_the_id_or_the_name_in_either_case_in_the_order_of_the_uids() {
    // By the id, and by the prototype's name, which has a space the id lacks.
    assert_eq!(uids(&search("Extinguisher")), [20, 30, 40]);
    assert_eq!(uids(&search("FIRE EXT")), [30, 40]);
    assert_eq!(uids(&search("cabinet")), [20]);
    // The entity's own name counts, and the prototype's still does.
    assert_eq!(uids(&search("captain")), [22]);
    assert_eq!(uids(&search("chair")), [22]);
    // Space around the query is not part of it.
    assert_eq!(uids(&search("  locker ")), [21]);
}

#[test]
fn a_search_finds_by_uid_in_the_engine_or_in_the_file() {
    assert_eq!(uids(&search("21")), [21]);
    assert_eq!(uids(&search("#201")), [21]);
    // The number of a new entity in the file is 0, which finds nothing.
    assert!(search("0").is_empty());
    // A number is matched whole, and not as a part of a uid.
    assert!(search("2").is_empty());
}

#[test]
fn an_empty_search_or_one_that_matches_nothing_finds_nothing() {
    assert!(search("").is_empty() && search("   ").is_empty());
    assert!(search("airlock").is_empty());
}

#[test]
fn a_result_says_what_it_is_and_where_to_jump() {
    let found = search("extinguisher");
    let cabinet = &found[0];
    assert_eq!((cabinet.prototype.as_str(), cabinet.name.as_str(), cabinet.yaml_uid), ("ExtinguisherCabinetFilled", "extinguisher cabinet", 200));
    assert_eq!((cabinet.grid, cabinet.cell, cabinet.world), (GRID, (2, 3), (2.5, 3.5)));
    // The one in the locker is where the locker is.
    assert_eq!((found[1].grid, found[1].cell, found[1].world), (GRID, (5, 1), (5.5, 1.5)));
    // The one on the second grid is in that grid's cells, and on the map ten cells right.
    assert_eq!((found[2].grid, found[2].cell, found[2].world), (MOVED_GRID, (1, 1), (11.5, 1.5)));
}

#[test]
fn a_result_off_every_grid_is_on_the_map_and_one_with_no_name_shows_its_id() {
    let found = search("crate");
    assert_eq!(uids(&found), [50]);
    assert_eq!((found[0].name.as_str(), found[0].grid, found[0].cell, found[0].world), ("CrateGeneric", 0, (-5, 7), (-4.5, 7.25)));
}

#[test]
fn a_search_gives_at_most_two_hundred() {
    let mut doc = doc();
    let template = doc.entities[&40].clone();
    for uid in 1000..1300 {
        doc.entities.insert(uid, crate::model::Entity { uid, ..template.clone() });
    }
    let found = find_entities(&doc, "extinguisher", name_of);
    assert_eq!(found.len(), MOST_FOUND);
    // The first two hundred by uid.
    assert_eq!((found[0].uid, found[199].uid), (20, 1196));
}
