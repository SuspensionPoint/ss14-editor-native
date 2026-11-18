use super::*;
use crate::forks::tests::{folder, write};

fn print(value: &str) -> Fingerprint {
    Fingerprint { lines: vec![("Resources/Prototypes".into(), value.into())] }
}

#[test]
fn a_fork_s_pack_and_textures_are_in_a_folder_of_their_fingerprint() {
    let cache = Cache::new(PathBuf::from("C:/cache"));
    let (prototypes, textures) = (print("a"), print("b"));
    let prepared = plan("Starlight", &cache, &prototypes, &textures, (None, None), |_| true);
    assert_eq!(prepared.pack, cache.pack(&prototypes.key()));
    assert!(prepared.pack.ends_with(format!("prototypes/{}/kernel-resources.zip", prototypes.key())));
    assert!(prepared.textures.ends_with(format!("textures/{}/textures.sstx", textures.key())));
    assert_eq!((prepared.pack_stage, prepared.textures_stage), (None, None), "both are there: nothing to make");
    assert_ne!(prototypes.key(), print("c").key());
}

#[test]
fn what_is_missing_is_made_with_words_for_why() {
    let cache = Cache::new(PathBuf::from("C:/cache"));
    let (prototypes, textures) = (print("a"), print("b"));
    let first = plan("Starlight", &cache, &prototypes, &textures, (None, None), |_| false);
    assert_eq!(first.pack_stage.as_deref(), Some("Packing Starlight's prototypes for the first time"));
    assert_eq!(first.textures_stage.as_deref(), Some("Baking Starlight's textures for the first time (up to a minute)"));

    let changed = plan("Starlight", &cache, &prototypes, &textures, (Some("old"), Some("old")), |_| false);
    assert_eq!(changed.pack_stage.as_deref(), Some("Starlight's prototypes changed since last time, packing them again"));
    assert_eq!(changed.textures_stage.as_deref(), Some("Starlight's textures changed since last time, baking them again (up to a minute)"));

    // The same as last time, but the cache was cleared.
    let same = (prototypes.key(), textures.key());
    let cleared = plan("Starlight", &cache, &prototypes, &textures, (Some(&same.0), Some(&same.1)), |path| path.ends_with("textures.sstx"));
    assert_eq!(cleared.pack_stage.as_deref(), Some("Packing Starlight's prototypes"));
    assert_eq!(cleared.textures_stage, None);
}

#[test]
fn the_cache_keeps_what_known_forks_use_and_the_last_written() {
    let root = folder("prune");
    let cache = Cache::new(root.clone());
    for key in ["kept", "old1", "old2", "old3", "old4"] {
        write(&cache.textures(key), "baked");
    }
    write(&cache.pack("pack"), "packed");
    cache.prune(&["kept".to_owned(), "pack".to_owned()]);
    let mut left: Vec<String> = std::fs::read_dir(root.join("textures")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left.len(), 3, "{left:?}");
    assert!(left.contains(&"kept".to_owned()));
    assert!(cache.pack("pack").is_file());
}

#[test]
fn a_part_is_put_in_place_only_once_it_is_whole() {
    let root = folder("partial");
    let output = root.join("textures").join("key").join(TEXTURES_FILE);
    let partial = partial(&output).unwrap();
    assert!(partial.ends_with("textures.sstx.partial"));
    assert!(partial.parent().unwrap().is_dir(), "its folder is made");
    std::fs::write(&partial, "whole").unwrap();
    finish(&partial, &output).unwrap();
    assert!(output.is_file() && !partial.exists());
}
