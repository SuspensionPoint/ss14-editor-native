//! Favourites: adding, removing, reordering, choosing, the number keys and keeping them per fork.

use super::*;
use crate::forks::settings::Settings;
use crate::forks::tests::folder;

fn tile(id: &str) -> Tile {
    Tile { id: id.into(), sprite: None, allows_rotation: false, can_crowbar: true }
}

/// A session whose engine knows two tiles, a chair and a wall, and one decal.
fn knowing() -> (Session, Receiver<ToEngine>) {
    let (mut session, engine) = Session::offline(&scene());
    session.tiles = vec![tile("FloorSteel"), tile("FloorWood")];
    let chair = Prototype { id: "Chair".into(), name: "chair".into(), suffix: Some("Wood".into()), ..Default::default() };
    let wall = Prototype { id: "WallSolid".into(), ..Default::default() };
    session.set_prototypes(vec![chair, wall]);
    session.decal_prototypes = vec![DecalPrototype { id: "Arrows".into(), ..Default::default() }];
    (session, engine)
}

fn ids(session: &Session) -> Vec<String> {
    session.favourites().into_iter().map(|row| row.favourite.id).collect()
}

#[test]
fn a_star_adds_and_takes_off_a_favourite_at_the_end() {
    let (mut session, _engine) = knowing();
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "WallSolid"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Tile, "FloorWood"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Decal, "Arrows"));
    assert_eq!(ids(&session), ["WallSolid", "FloorWood", "Arrows"]);
    assert!(session.is_favourite(&Favourite::new(FavouriteKind::Tile, "FloorWood")));
    // A tile and a decal of one id are two favourites.
    assert!(!session.is_favourite(&Favourite::new(FavouriteKind::Decal, "FloorWood")));

    session.toggle_favourite(Favourite::new(FavouriteKind::Tile, "FloorWood"));
    assert_eq!(ids(&session), ["WallSolid", "Arrows"]);
    assert_eq!(session.status_line(), "Removed FloorWood from favourites");
    session.remove_favourite(0);
    session.remove_favourite(5);
    assert_eq!(ids(&session), ["Arrows"]);
}

#[test]
fn what_is_in_hand_can_be_starred() {
    let (mut session, _engine) = knowing();
    assert_eq!(session.held_favourite(), Some(Favourite::new(FavouriteKind::Tile, "FloorSteel")));
    session.choose_entity(0);
    let held = session.held_favourite().unwrap();
    assert_eq!(held, Favourite::new(FavouriteKind::Entity, "Chair"));
    session.toggle_favourite(held);
    let rows = session.favourites();
    assert_eq!((rows[0].name.as_str(), rows[0].chosen, rows[0].key), ("chair (Wood)", true, Some(1)));
}

#[test]
fn favourites_move_up_and_down_within_the_list() {
    let (mut session, _engine) = knowing();
    for id in ["FloorSteel", "FloorWood"] {
        session.toggle_favourite(Favourite::new(FavouriteKind::Tile, id));
    }
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "Chair"));
    session.move_favourite(2, true);
    assert_eq!(ids(&session), ["FloorSteel", "Chair", "FloorWood"]);
    session.move_favourite(0, false);
    assert_eq!(ids(&session), ["Chair", "FloorSteel", "FloorWood"]);
    // Past either end nothing moves.
    session.move_favourite(0, true);
    session.move_favourite(2, false);
    assert_eq!(ids(&session), ["Chair", "FloorSteel", "FloorWood"]);
}

#[test]
fn choosing_a_favourite_chooses_it_as_its_own_palette_does() {
    let (mut session, engine) = knowing();
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "WallSolid"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Decal, "Arrows"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Tile, "FloorWood"));

    // An entity takes E-Place in hand and asks how it looks, as choose_entity does.
    session.set_tool(Tool::Fill);
    assert!(session.choose_favourite(0));
    assert_eq!((session.palette().clone(), session.tool()), (Palette::Entity(1), Tool::Place));
    assert!(engine.try_iter().any(|command| matches!(command, ToEngine::Previews { .. })));
    // A decal takes Paint from a tool that cannot brush it, and a tile keeps a tile tool.
    session.set_tool(Tool::Fill);
    assert!(session.choose_favourite(1));
    assert_eq!((session.palette().clone(), session.tool()), (Palette::Decal("Arrows".into()), Tool::Paint));
    session.set_tool(Tool::Rectangle);
    assert!(session.choose_favourite(2));
    assert_eq!((session.palette().clone(), session.tool()), (Palette::Tile("FloorWood".into()), Tool::Rectangle));
    assert!(!session.choose_favourite(3));
}

#[test]
fn a_favourite_the_fork_lacks_is_kept_and_not_chosen() {
    let (mut session, _engine) = Session::offline(&scene());
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "Gone"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "Chair"));
    // Before the engine has said what the fork has, nothing is missing and nothing is chosen.
    assert_eq!(session.favourites()[0].state, FavouriteState::Waiting);
    assert!(!session.choose_favourite(1));

    let (known, _known_engine) = knowing();
    session.tiles = known.tiles.clone();
    session.set_prototypes(known.prototypes().to_vec());
    let rows = session.favourites();
    assert_eq!((rows[0].state, rows[0].name.as_str()), (FavouriteState::Missing, "Gone"));
    assert_eq!(rows[1].state, FavouriteState::Here(0));
    let before = session.palette().clone();
    assert!(!session.choose_favourite(0));
    assert_eq!(session.palette(), &before);
    assert_eq!(session.status_line(), "Gone isn't in this fork");
    assert_eq!(ids(&session), ["Gone", "Chair"]);
}

#[test]
fn the_number_keys_choose_the_first_nine() {
    let (mut session, _engine) = knowing();
    for at in 0..10 {
        let id = if at == 4 { "FloorWood".to_owned() } else { format!("Missing{at}") };
        session.toggle_favourite(Favourite::new(FavouriteKind::Tile, id));
    }
    let keys: Vec<Option<usize>> = session.favourites().iter().map(|row| row.key).collect();
    assert_eq!(keys[..], [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6), Some(7), Some(8), Some(9), None]);

    assert!(session.favourite_key(5));
    assert_eq!(session.chosen_tile(), Some("FloorWood"));
    assert_eq!(session.status_line(), "Favourite 5: FloorWood");
    // One the fork lacks is not chosen, and 0 and 10 are no favourite's keys.
    assert!(session.favourite_key(1));
    assert_eq!(session.chosen_tile(), Some("FloorWood"));
    assert_eq!(session.status_line(), "Missing0 isn't in this fork");
    assert!(!session.favourite_key(0));
    assert!(!session.favourite_key(10));
    for at in (5..10).rev() {
        session.remove_favourite(at);
    }
    assert!(session.favourite_key(7));
    assert_eq!(session.status_line(), "No favourite 7");
}

#[test]
fn a_number_key_does_nothing_during_a_drag() {
    let (mut session, _engine) = knowing();
    session.toggle_favourite(Favourite::new(FavouriteKind::Tile, "FloorWood"));
    session.set_tool(Tool::Paint);
    session.pointer_down(over(&session, (0, 0)), VIEW, Button::Primary);
    assert!(!matches!(session.drag, Drag::None));
    assert!(!session.favourite_key(1));
    assert_eq!(session.chosen_tile(), Some("FloorSteel"));
    session.pointer_up(over(&session, (0, 0)), VIEW);
    assert!(session.favourite_key(1));
    assert_eq!(session.chosen_tile(), Some("FloorWood"));
}

#[test]
fn favourites_are_kept_in_the_settings_under_the_fork() {
    let file = folder("favourites").join("settings.json");
    // What else the file holds stays.
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), Some("p".into()), None);
    settings.save(&file).unwrap();

    let (mut session, _engine) = knowing();
    session.favourites = crate::session::favourites::Favourites::load(file.clone(), "C:/forks/starlight");
    session.toggle_favourite(Favourite::new(FavouriteKind::Entity, "WallSolid"));
    session.toggle_favourite(Favourite::new(FavouriteKind::Tile, "FloorWood"));
    session.move_favourite(1, true);
    let read = Settings::load(&file);
    assert_eq!(read.forks[0].prototypes.as_deref(), Some("p"));
    let kept: Vec<&str> = read.favourites("C:/forks/starlight").iter().map(|favourite| favourite.id.as_str()).collect();
    assert_eq!(kept, ["FloorWood", "WallSolid"]);

    // Another fork has its own, and a session of this fork reads them back.
    let (mut other, _other_engine) = knowing();
    other.favourites = crate::session::favourites::Favourites::load(file.clone(), "C:/checkouts/wizden");
    assert!(other.favourites().is_empty());
    let (mut again, _again_engine) = knowing();
    again.favourites = crate::session::favourites::Favourites::load(file, "C:\\forks\\starlight\\");
    assert_eq!(ids(&again), ["FloorWood", "WallSolid"]);
}
