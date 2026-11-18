//! Prefabs: Save as Prefab from the region's menu, the list of the folder, and stamping a prefab.

use super::paste::{hover, one_copy};
use super::region_copy::{answer, region_held, right_click, TEXT};
use super::*;
use crate::prefab::tests::temp_folder;
use crate::prefab::{self, Prefab};

/// A session as `region_held` gives it, keeping its prefabs in a folder of its own.
fn held_with_folder(name: &str) -> (Session, Sender<FromEngine>, Receiver<ToEngine>, PathBuf) {
    let (mut session, answers, engine) = region_held();
    let folder = temp_folder(name);
    session.set_prefabs_folder(folder.clone());
    (session, answers, engine, folder)
}

/// A session as `held_with_folder` gives it, whose region was saved as the prefab
/// "rooms/kitchen" through the region's menu, with the copy answered.
fn saved(name: &str) -> (Session, Sender<FromEngine>, Receiver<ToEngine>, PathBuf) {
    let (mut session, answers, engine, folder) = held_with_folder(name);
    right_click(&mut session, (1, 1));
    session.choose_from_region_menu(MenuAction::SaveAsPrefab);
    // The interface is asked for a name once.
    assert!(session.wants_prefab_name());
    assert!(!session.wants_prefab_name());
    session.save_region_as_prefab("rooms/kitchen");
    let (seq, uids) = one_copy(&engine);
    assert_eq!(uids, [CHAIR, WALL]);
    // Nothing is written until the copy has come.
    assert!(!prefab::exists(&folder, "rooms/kitchen"));
    answer(&mut session, &answers, seq);
    (session, answers, engine, folder)
}

#[test]
fn save_as_prefab_writes_the_region_into_its_subfolder() {
    let (mut session, _answers, engine, folder) = saved("prefab-save");
    assert_eq!(session.status_line(), "Saved prefab rooms/kitchen");
    let file = prefab::load(&folder, "rooms/kitchen").unwrap();
    assert_eq!(Some(&file), session.clipboard().and_then(Clipboard::to_prefab).as_ref());
    assert_eq!((file.text.as_str(), file.from, file.entities, file.tiles.len(), file.decals.len()), (TEXT, (1, 1), 3, 5, 1));
    // The list has it in its group, without being refreshed by hand.
    let listed: Vec<(&str, &str)> = session.prefabs().iter().map(|prefab| (prefab.group.as_str(), prefab.title.as_str())).collect();
    assert_eq!(listed, [("rooms", "kitchen")]);
    // A save changes nothing on the map, and the region stays.
    assert!(asked(&engine).is_empty());
    assert_eq!(session.region_size(), 6);

    // A name that is taken, in any spelling, is refused before anything is asked.
    session.save_region_as_prefab(" rooms\\kitchen.ss14prefab ");
    assert_eq!(session.status_line(), "Prefab rooms/kitchen not saved: a prefab with that name already exists");
    session.save_region_as_prefab(" / ");
    assert_eq!(session.status_line(), "Prefab not saved: give it a name");
    session.clear_region();
    session.save_region_as_prefab("other");
    assert_eq!(session.status_line(), "Prefab other not saved: no region is selected");
    assert_eq!(engine.try_iter().count(), 0);
    assert_eq!(prefab::list(&folder).len(), 1);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_region_with_nothing_on_it_is_saved_at_once() {
    let (mut session, _answers, engine, folder) = held_with_folder("prefab-bare");
    drag(&mut session, (0, 3), (1, 4));
    session.save_region_as_prefab("floor");
    assert_eq!(engine.try_iter().count(), 0);
    let file = prefab::load(&folder, "floor").unwrap();
    assert_eq!((file.text.as_str(), file.cells.len(), file.tiles.len()), ("", 4, 4));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_chosen_prefab_stamps_as_the_copied_region_does() {
    let (mut session, _answers, engine, folder) = saved("prefab-stamp");
    session.tiles = vec![Tile { id: "FloorSteel".into(), sprite: None, allows_rotation: true, can_crowbar: false }];
    let copied = session.clipboard().unwrap().region_cells().to_vec();
    // A stamp of the region as copied, turned once.
    session.begin_paste();
    tap_r(&mut session, false);
    hover(&mut session, (2.5, 3.5));
    let copied_ghost = session.region_paste_cells();
    click(&mut session, (2.5, 3.5));
    let from_copy = asked(&engine);
    session.escape();

    // The prefab, on a clipboard that held nothing, stamps the same.
    session.clipboard = None;
    session.set_tool(Tool::Paint);
    session.choose_prefab("rooms/kitchen");
    assert_eq!((session.tool(), session.is_pasting(), session.chosen_prefab()), (Tool::Area, true, Some("rooms/kitchen")));
    assert_eq!(session.status_line(), "Prefab rooms/kitchen (3x2): click to place, R to turn, right-click or Escape to stop");
    let clipboard = session.clipboard().unwrap();
    assert_eq!((clipboard.region_cells(), clipboard.text()), (&copied[..], TEXT));
    tap_r(&mut session, false);
    hover(&mut session, (2.5, 3.5));
    assert_eq!(session.region_paste_cells(), copied_ghost);
    // The ghost fills the cells, and has no pictures of the entities.
    assert_eq!(session.overlay_quads().1.len(), 6 * FLOATS_PER_QUAD);
    click(&mut session, (2.5, 3.5));
    click(&mut session, (5.5, 5.5));
    let stamps = edits(&engine);
    assert_eq!(labels(&stamps), ["Paste region", "Paste region"]);
    assert_eq!(vec![stamps[0].2.clone()], from_copy);
    // Right-click ends it as it ends a paste.
    right_click(&mut session, (1, 1));
    assert!(!session.is_pasting());

    // A copy takes the prefab off the clipboard.
    session.set_tool(Tool::Area);
    drag(&mut session, (0, 3), (1, 4));
    session.copy();
    assert_eq!(session.chosen_prefab(), None);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_prefab_that_cannot_be_read_is_refused_with_a_message() {
    let (mut session, _answers, _engine, folder) = held_with_folder("prefab-refused");
    std::fs::write(folder.join("broken.ss14prefab"), "not a prefab").unwrap();
    let newer = Prefab { from: (0, 0), roots: 0, entities: 0, cells: vec![(0, 0)], tiles: Vec::new(), decals: Vec::new(), text: String::new() }.write().replacen("\"version\":1", "\"version\":9", 1);
    std::fs::write(folder.join("newer.ss14prefab"), newer).unwrap();
    session.set_tool(Tool::Select);

    session.choose_prefab("broken");
    assert_eq!(session.status_line(), "Prefab broken not read: it isn't a prefab file, or it's damaged");
    session.choose_prefab("newer");
    assert_eq!(session.status_line(), "Prefab newer not read: it was made by a newer editor (format 9, this editor reads up to 1)");
    session.choose_prefab("missing");
    assert!(session.status_line().starts_with("Prefab missing not read: "));
    assert_eq!((session.clipboard().is_none(), session.is_pasting(), session.tool()), (true, false, Tool::Select));
    // The list shows them, with why each cannot be read.
    let errors: Vec<bool> = session.prefabs().iter().map(|prefab| prefab.shape.is_err()).collect();
    assert_eq!(errors, [true, true]);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn the_list_is_searched_refreshed_imported_into_and_deleted_from() {
    let (mut session, _answers, _engine, folder) = held_with_folder("prefab-list");
    let outside = temp_folder("prefab-list-from");
    let pod = Prefab { from: (0, 0), roots: 0, entities: 0, cells: vec![(0, 0), (1, 0)], tiles: Vec::new(), decals: Vec::new(), text: String::new() };
    prefab::save(&folder, "rooms/kitchen", &pod).unwrap();
    prefab::save(&folder, "rooms/bar", &pod).unwrap();
    assert_eq!(session.prefabs().len(), 2);

    // A file put in the folder by hand shows after a refresh.
    prefab::save(&folder, "Airlock", &pod).unwrap();
    assert_eq!(session.prefabs().len(), 2);
    session.refresh_prefabs();
    let names = |session: &mut Session, search: &str| {
        let found = session.prefabs_matching(search);
        found.into_iter().map(|index| session.prefabs()[index].name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(&mut session, ""), ["Airlock", "rooms/bar", "rooms/kitchen"]);
    assert_eq!(names(&mut session, "Rooms K"), ["rooms/kitchen"]);

    // An import copies the file into the top of the folder. Quotes around the path are dropped.
    std::fs::write(outside.join("Pod.ss14prefab"), pod.write()).unwrap();
    session.import_prefab(&format!(" \"{}\" ", outside.join("Pod.ss14prefab").display()));
    assert_eq!(session.status_line(), "Imported prefab Pod");
    assert_eq!(names(&mut session, "pod"), ["Pod"]);
    session.import_prefab(&outside.join("Pod.ss14prefab").display().to_string());
    assert_eq!(session.status_line(), "Prefab not imported: a prefab named Pod already exists");
    session.import_prefab("  ");
    assert_eq!(session.status_line(), "Prefab not imported: enter a file path");

    // A delete takes the file, and the group goes with its last prefab.
    session.delete_prefab("rooms/kitchen");
    session.delete_prefab("rooms/bar");
    assert_eq!(session.status_line(), "Deleted prefab rooms/bar");
    assert_eq!(names(&mut session, ""), ["Airlock", "Pod"]);
    assert!(!folder.join("rooms").exists());
    session.delete_prefab("rooms/bar");
    assert!(session.status_line().starts_with("Prefab rooms/bar not deleted: "));
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}
