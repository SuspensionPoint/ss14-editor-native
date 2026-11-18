use super::*;
use crate::forks::tests::folder;

#[test]
fn a_change_keeps_what_was_written_since_a_copy_was_read() {
    // The start screen read its copy, then the editor noted a map. A fork added from the
    // screen must not take the map away, as a save of the old copy did.
    let file = folder("settings-update").join("settings.json");
    let starlight = Path::new("C:/forks/starlight");
    let screen_copy = Settings::update(&file, |settings| settings.used_fork(starlight, None, None)).unwrap();
    Settings::update(&file, |settings| settings.opened_map(starlight, Path::new("C:/forks/starlight/Resources/Maps/bagel.yml"))).unwrap();
    assert!(screen_copy.recent_maps.is_empty());

    let saved = Settings::update(&file, |settings| {
        settings.add_fork(Path::new("C:/forks/delta-v"));
    })
    .unwrap();
    assert_eq!(saved.recent_maps.len(), 1, "the map noted since is kept");
    assert_eq!(saved.forks.len(), 2);
    assert_eq!(Settings::load(&file), saved);
}

#[test]
fn an_unreadable_settings_file_is_moved_aside_not_written_over() {
    let file = folder("settings-unreadable").join("settings.json");
    std::fs::write(&file, "{ not json").unwrap();
    let saved = Settings::update(&file, |settings| {
        settings.add_fork(Path::new("C:/forks/starlight"));
    })
    .unwrap();
    assert_eq!(saved.forks.len(), 1);
    assert_eq!(std::fs::read_to_string(file.with_extension("json.unreadable")).unwrap(), "{ not json");
}

#[test]
fn settings_are_written_and_read_back() {
    let file = folder("settings").join("ss14-editor").join("settings.json");
    assert_eq!(Settings::load(&file), Settings::default(), "no file is no settings");
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), Some("p1".into()), Some("t1".into()));
    settings.opened_map(Path::new("C:/forks/starlight"), Path::new("C:/forks/starlight/Resources/Maps/bagel.yml"));
    settings.save(&file).unwrap();
    assert_eq!(Settings::load(&file), settings);
    assert!(!file.with_extension("json.partial").exists());

    std::fs::write(&file, "{ not json").unwrap();
    assert_eq!(Settings::load(&file), Settings::default(), "a broken file is no settings");
    // A file from an older editor, with fields missing, is read with what it has.
    std::fs::write(&file, r#"{ "forks": [ { "path": "C:/forks/wizden" } ] }"#).unwrap();
    assert_eq!(Settings::load(&file).forks[0].path, "C:/forks/wizden");
}

#[test]
fn a_fork_is_added_once_named_for_its_folder() {
    let mut settings = Settings::default();
    settings.add_fork(Path::new("C:\\forks\\starlight"));
    settings.add_fork(Path::new("C:/forks/starlight/"));
    settings.add_fork(Path::new("C:/forks/wizden"));
    assert_eq!(settings.forks.len(), 2);
    assert_eq!(settings.forks[0].name, "starlight");
    assert_eq!(settings.forks[0].path, "C:/forks/starlight");
    assert_eq!(settings.forks[0].maps_folder(), Path::new("C:/forks/starlight").join("Resources").join("Maps"));
}

#[test]
fn the_fork_used_goes_first_and_keeps_its_keys() {
    let mut settings = Settings::default();
    settings.add_fork(Path::new("C:/forks/starlight"));
    settings.add_fork(Path::new("C:/forks/wizden"));
    settings.used_fork(Path::new("C:/forks/wizden"), Some("p1".into()), Some("t1".into()));
    assert_eq!(settings.forks[0].path, "C:/forks/wizden");
    assert_eq!(settings.last_fork.as_deref(), Some("C:/forks/wizden"));
    // Opened from the command line with what the scripts made: the keys stay as they were.
    settings.used_fork(Path::new("C:/forks/wizden"), None, None);
    assert_eq!((settings.forks[0].prototypes.as_deref(), settings.forks[0].textures.as_deref()), (Some("p1"), Some("t1")));
    assert_eq!(settings.forks.len(), 2);
    assert_eq!(settings.cached_keys(), ["p1", "t1"]);
    // Opened with an engine side built for it: the cache keeps that too, until it needs none.
    settings.used_engine(Path::new("C:/forks/wizden"), Some("e1".into()));
    assert_eq!(settings.cached_keys(), ["p1", "t1", "e1"]);
    settings.used_engine(Path::new("C:/forks/wizden"), None);
    assert_eq!(settings.cached_keys(), ["p1", "t1"]);
}

#[test]
fn a_map_opened_is_its_fork_s_last_and_first_of_the_recent() {
    let mut settings = Settings::default();
    let fork = Path::new("C:/forks/starlight");
    for index in 0..12 {
        settings.opened_map(fork, &Path::new("C:/maps").join(format!("map{index}.yml")));
    }
    settings.opened_map(fork, Path::new("C:/forks/starlight/Resources/Maps/Stations/bagel.yml"));
    settings.opened_map(fork, Path::new("C:/maps/map11.yml"));
    assert_eq!(settings.recent_maps.len(), RECENT_MAPS);
    assert_eq!(settings.recent_maps[0].map, "C:/maps/map11.yml");
    assert_eq!(settings.recent_maps[1].map, "C:/forks/starlight/Resources/Maps/Stations/bagel.yml");
    assert_eq!(settings.recent_maps.iter().filter(|recent| recent.map == "C:/maps/map11.yml").count(), 1);
    let known = settings.fork("C:/forks/starlight").unwrap();
    assert_eq!(known.last_map.as_deref(), Some("C:/maps/map11.yml"));
    assert_eq!(known.maps_folder(), PathBuf::from("C:/maps"));
}

#[test]
fn a_fork_taken_off_the_list_takes_its_maps_with_it() {
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), None, None);
    settings.opened_map(Path::new("C:/forks/starlight"), Path::new("C:/maps/a.yml"));
    settings.opened_map(Path::new("C:/forks/wizden"), Path::new("C:/maps/b.yml"));
    settings.remove_fork("C:/forks/starlight/");
    assert_eq!(settings.forks.iter().map(|fork| fork.path.as_str()).collect::<Vec<_>>(), ["C:/forks/wizden"]);
    assert_eq!(settings.recent_maps.len(), 1);
    assert_eq!(settings.last_fork, None);
}

#[test]
fn paths_are_the_same_with_either_slash() {
    assert!(same_path("C:\\forks\\starlight", "C:/forks/starlight/"));
    assert!(!same_path("C:/forks/starlight", "C:/forks/starlight2"));
    assert_eq!(same_path("C:/Forks", "C:/forks"), cfg!(windows));
}

#[test]
fn a_downloaded_fork_is_marked_as_the_editor_s() {
    let mut settings = Settings::default();
    settings.add_fork(Path::new("C:/forks/mine"));
    settings.add_downloaded(Path::new("C:/data/ss14-editor/forks/ss14Starlight-space-station-14"), "https://github.com/ss14Starlight/space-station-14", "Starlight");
    let (mine, downloaded) = (&settings.forks[0], &settings.forks[1]);
    assert!(!mine.managed && mine.url.is_none());
    assert!(downloaded.managed);
    assert_eq!(downloaded.name, "Starlight");
    assert_eq!(downloaded.url.as_deref(), Some("https://github.com/ss14Starlight/space-station-14"));
    // Kept when it is used, and read back from the file.
    settings.used_fork(Path::new("C:/data/ss14-editor/forks/ss14Starlight-space-station-14"), Some("p".into()), None);
    assert!(settings.forks[0].managed);
    let file = folder("settings-managed").join("settings.json");
    settings.save(&file).unwrap();
    assert_eq!(Settings::load(&file), settings);
}

#[test]
fn favourites_are_kept_per_fork_and_read_back() {
    let mut settings = Settings::default();
    settings.used_fork(Path::new("C:/forks/starlight"), None, None);
    let wall = Favourite::new(FavouriteKind::Entity, "WallSolid");
    let steel = Favourite::new(FavouriteKind::Tile, "FloorSteel");
    settings.set_favourites("C:/forks/starlight", vec![wall.clone(), steel.clone()]);
    // A checkout given on the command line, not among the forks, keeps its own under its folder.
    settings.set_favourites("C:\\checkouts\\wizden", vec![Favourite::new(FavouriteKind::Decal, "Arrows")]);
    assert_eq!(settings.forks.len(), 1);
    assert_eq!(settings.favourites("C:/forks/starlight/"), [wall, steel]);
    assert_eq!(settings.favourites("C:/checkouts/wizden")[0].id, "Arrows");
    assert!(settings.favourites("C:/forks/other").is_empty());

    let file = folder("settings-favourites").join("settings.json");
    settings.save(&file).unwrap();
    assert_eq!(Settings::load(&file), settings);
    assert!(std::fs::read_to_string(&file).unwrap().contains(r#""kind": "entity""#));

    // An empty list takes the fork's entry away, and removing a fork takes its favourites.
    settings.set_favourites("C:/checkouts/wizden", Vec::new());
    assert_eq!(settings.favourites.len(), 1);
    settings.remove_fork("C:/forks/starlight");
    assert!(settings.favourites.is_empty());
}
