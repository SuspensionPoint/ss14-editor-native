//! The palettes' pictures.

use super::*;

/// A fork's folder with one tile picture, baked as the editor bakes a fork's textures, in a
/// folder under the system's temporary folder that is removed after.
fn baked_plating() -> (std::path::PathBuf, BakedTextures) {
    let folder = std::env::temp_dir().join(format!("editor-thumbs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let tiles = folder.join("Resources").join("Textures").join("Tiles");
    std::fs::create_dir_all(&tiles).unwrap();
    let file = std::fs::File::create(tiles.join("plating.png")).unwrap();
    let mut encoder = png::Encoder::new(file, 32, 32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[128; 32 * 32 * 4]).unwrap();
    let baked = folder.join("textures.sstx");
    editor_core::textures::bake_with(&folder, &folder.join("none"), &baked).unwrap();
    let textures = BakedTextures::open(&baked).unwrap();
    (folder, textures)
}

#[test]
fn a_tile_asked_for_before_its_sprite_is_known_gets_its_picture_once_it_is() {
    let (folder, textures) = baked_plating();
    let mut thumbs = Thumbs::default();
    // The options bar asks for Plating before the engine has sent the list of tiles.
    assert_eq!(thumbs.tile(&textures, None, 32), None);
    // The palette asks again once the list has come, and gets the picture.
    let picture = thumbs.tile(&textures, Some("/Textures/Tiles/plating.png"), 32);
    assert!(picture.as_deref().is_some_and(|uri| uri.starts_with("data:image/png;base64,")));
    // A sprite with no picture has none.
    assert_eq!(thumbs.tile(&textures, Some("/Textures/Tiles/none.png"), 32), None);
    let _ = std::fs::remove_dir_all(&folder);
}
