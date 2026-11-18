//! What is in hand, and what asks the palettes to show it.

use super::*;

/// A session whose engine knows a chair, with a suffix, and a wall with no name.
fn knowing() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    let chair = Prototype { id: "Chair".into(), name: "chair".into(), suffix: Some("Wood".into()), ..Default::default() };
    let wall = Prototype { id: "WallSolid".into(), ..Default::default() };
    session.set_prototypes(vec![chair, wall]);
    (session, engine)
}

#[test]
fn what_is_in_hand_follows_the_palettes() {
    let (mut session, _engine) = knowing();
    assert_eq!(session.in_hand(), Some(InHand::Tile("FloorSteel".into())));

    session.choose_entity(0);
    let held = session.in_hand().unwrap();
    assert_eq!(held, InHand::Entity { index: 0, name: "chair (Wood)".into(), id: "Chair".into() });
    assert_eq!((held.name(), held.kind()), ("chair (Wood)", "Entity"));
    // One with no name goes by its id.
    session.choose_entity(1);
    assert_eq!(session.in_hand().map(|held| held.name().to_owned()), Some("WallSolid".into()));

    session.choose_decal("Arrows");
    assert_eq!(session.in_hand(), Some(InHand::Decal("Arrows".into())));
    // A prototype the engine does not know is nothing in hand.
    session.choose_entity(7);
    assert_eq!(session.in_hand(), None);
}

#[test]
fn the_tree_is_made_once_both_the_categories_and_the_prototypes_are_known() {
    let (mut session, _engine) = Session::offline(&scene());
    let categories: HashMap<String, String> = [("Chair".to_owned(), "Furniture".to_owned())].into();
    session.categories_arrived(categories);
    assert_eq!(session.entity_tree(), None);

    // The engine's list comes after the fork's files have been read, and makes the tree.
    session.set_prototypes(knowing().0.prototypes().to_vec());
    let tree = session.entity_tree().expect("a tree");
    let paths: Vec<(&str, &[usize])> = tree.categories.iter().map(|category| (category.path.as_str(), &category.entries[..])).collect();
    assert_eq!(paths, [("Furniture", &[0][..]), ("Other", &[1][..])]);
}

#[test]
fn the_entity_search_goes_over_the_tree_once_it_is_made() {
    let (mut session, _engine) = Session::offline(&scene());
    let prototype = |id: &str, name: &str, suffix: Option<&str>| {
        let search = format!("{id} {name}").to_lowercase();
        Prototype { id: id.into(), name: name.into(), suffix: suffix.map(Into::into), search, ..Default::default() }
    };
    session.set_prototypes(vec![prototype("Table", "table", None), prototype("ChairWood", "chair", Some("Old"))]);
    // Before the fork's files are read: what can be placed, by id and name, in the engine's order.
    assert_eq!(session.entity_search(&["CHAIR"]), [1]);
    assert_eq!(session.entity_search(&["old"]), [] as [usize; 0]);
    assert_eq!(session.entity_search(&[]), [0, 1]);

    // Then the tree's search, which reads the suffix too and goes by name.
    let categories: HashMap<String, String> = [("ChairWood".to_owned(), "Furniture".to_owned())].into();
    session.categories_arrived(categories);
    assert_eq!(session.entity_search(&["old"]), [1]);
    assert_eq!(session.entity_search(&[]), [1, 0]);
}

#[test]
fn an_entity_s_category_is_the_palette_s() {
    let (mut session, _engine) = knowing();
    assert_eq!(session.palette_category("Chair"), None);
    let categories: HashMap<String, String> = [("Chair".to_owned(), "Structures/Furniture".to_owned())].into();
    session.categories_arrived(categories);
    assert_eq!(session.palette_category("Chair"), Some("Structures/Furniture"));
    // One whose file was not read is where the tree puts it.
    assert_eq!(session.palette_category("WallSolid"), Some("Other"));
}

#[test]
fn only_a_pick_asks_the_palettes_to_show_what_it_chose() {
    let (mut session, _engine) = knowing();
    session.choose_tile("FloorWood");
    session.choose_entity(0);
    assert!(!session.take_reveal());

    session.set_tool(Tool::Pick);
    click(&mut session, (0.5, 4.5));
    assert_eq!(session.chosen_tile(), Some("FloorSteel"));
    // Once for each pick.
    assert!(session.take_reveal());
    assert!(!session.take_reveal());

    // A pick of nothing asks nothing.
    session.set_tool(Tool::Pick);
    click(&mut session, (8.5, 8.5));
    assert!(!session.take_reveal());
}
