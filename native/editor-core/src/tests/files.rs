use super::*;

/// A folder of its own under the system's temporary folder, empty.
fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("editor-core-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn a_dialog_starts_in_the_folder_last_used_while_it_is_there() {
    let maps = folder("maps");
    let other = folder("other");
    let mut last = LastFolder::default();
    assert_eq!(last.start_in(&maps), maps);

    last.used(&other.join("Bagel.yml"));
    assert_eq!(last.start_in(&maps), other);
    // A file with no folder changes nothing.
    last.used(Path::new("Bagel.yml"));
    assert_eq!(last.start_in(&maps), other);

    // A folder that has gone is passed over.
    std::fs::remove_dir_all(&other).unwrap();
    assert_eq!(last.start_in(&maps), maps);
    let _ = std::fs::remove_dir_all(&maps);
}

#[test]
fn a_map_saved_with_no_extension_is_given_yml() {
    assert_eq!(with_yml(Path::new("C:/maps/cork")), PathBuf::from("C:/maps/cork.yml"));
    assert_eq!(with_yml(Path::new("C:/maps/cork.yml")), PathBuf::from("C:/maps/cork.yml"));
    // An extension the user typed is theirs.
    assert_eq!(with_yml(Path::new("C:/maps/cork.yaml")), PathBuf::from("C:/maps/cork.yaml"));
    assert_eq!(forward_slashes(Path::new("C:\\maps\\cork.yml")), "C:/maps/cork.yml");
}

#[test]
fn save_as_offers_the_map_s_own_file_or_untitled() {
    let maps = folder("save-maps");
    let mut last = LastFolder::default();
    assert_eq!(save_as_start(None, &last, &maps), (maps.clone(), "untitled.yml".to_owned()));

    let saved = Path::new("C:/elsewhere/Cork.yml");
    assert_eq!(save_as_start(Some(saved), &last, &maps), (PathBuf::from("C:/elsewhere"), "Cork.yml".to_owned()));

    // A map never saved starts in the folder last used.
    let other = folder("save-other");
    last.used(&other.join("Bagel.yml"));
    assert_eq!(save_as_start(None, &last, &maps), (other.clone(), "untitled.yml".to_owned()));
    let _ = std::fs::remove_dir_all(&maps);
    let _ = std::fs::remove_dir_all(&other);
}
