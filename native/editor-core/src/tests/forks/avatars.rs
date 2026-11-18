use super::*;
use crate::forks::tests::folder;

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[test]
fn an_avatar_is_asked_of_github_by_its_owner_and_kept_in_small_letters() {
    assert_eq!(avatar_url("RMC-14"), "https://github.com/RMC-14.png?size=96");
    assert_eq!(avatar_file(Path::new("cache"), "RMC-14"), Path::new("cache").join("rmc-14.png"));
}

#[test]
fn only_a_png_counts_as_an_avatar() {
    assert!(is_png(&PNG));
    assert!(!is_png(b"<!DOCTYPE html>"));
    assert!(!is_png(&[]));
}

#[test]
fn a_kept_avatar_is_read_without_the_network() {
    let root = folder("avatars-kept");
    let mut bytes = PNG.to_vec();
    bytes.extend_from_slice(b"rest");
    std::fs::write(avatar_file(&root, "Owner"), &bytes).unwrap();
    assert_eq!(avatar(&root, "owner"), Some(bytes));
}

#[test]
fn a_failed_fetch_keeps_nothing() {
    let root = folder("avatars-failed");
    let file = avatar_file(&root, "nobody");
    // A file address curl reads with no network, which is not a picture.
    let page = root.join("page.html");
    std::fs::write(&page, "<html></html>").unwrap();
    let url = format!("file:///{}", page.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    assert!(fetch(&url, &file).is_err());
    assert!(!file.exists());
    assert!(!file.with_extension("png.partial").exists());
}
