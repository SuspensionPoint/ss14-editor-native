use super::*;

/// A folder under the system's temporary folder that is removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("editor-textures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_png(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

#[test]
fn baked_images_keep_straight_alpha_and_srgb_colour() {
    let fork = Folder::new("straight");
    let engine = fork.0.join("RobustToolbox").join("Resources");
    std::fs::create_dir_all(&engine).unwrap();
    // A half-clear pixel, a clear one with a colour, and an opaque one.
    let pixels = [200, 100, 50, 128, 10, 20, 30, 0, 1, 2, 3, 255];
    write_png(&fork.0.join("Resources/Textures/thing.rsi/icon.png"), 3, 1, &pixels);
    let output = fork.0.join("textures.sstx");
    let report = bake(&fork.0, &output).unwrap();
    assert_eq!((report.images, report.failed), (1, 0));

    let baked = BakedTextures::open(&output).unwrap();
    let sheet = baked.sheet("/Textures/thing.rsi/icon.png").unwrap();
    assert_eq!(sheet.pixels, pixels);
    // A palette's picture is premultiplied, as Blitz takes it.
    assert_eq!(baked.thumbnail("/Textures/thing.rsi/icon.png", 1).unwrap(), [100, 50, 25, 128]);
}

#[test]
fn an_rsi_the_game_loads_without_srgb_is_stored_so_that_decoding_gives_its_bytes() {
    let fork = Folder::new("linear");
    std::fs::create_dir_all(fork.0.join("RobustToolbox").join("Resources")).unwrap();
    let rsi = fork.0.join("Resources/Textures/Effects/bullet_hole.rsi");
    write_png(&rsi.join("bullet_hole.png"), 1, 1, &[128, 0, 255, 200]);
    std::fs::write(rsi.join("meta.json"), r#"{ "version": 1, "size": { "x": 1, "y": 1 }, "load": { "srgb": false }, "states": [] }"#).unwrap();
    // The same picture in an ordinary RSI beside it.
    let plain = fork.0.join("Resources/Textures/Effects/plain.rsi");
    write_png(&plain.join("plain.png"), 1, 1, &[128, 0, 255, 200]);
    std::fs::write(plain.join("meta.json"), r#"{ "version": 1, "size": { "x": 1, "y": 1 }, "states": [] }"#).unwrap();

    let output = fork.0.join("textures.sstx");
    bake(&fork.0, &output).unwrap();
    let baked = BakedTextures::open(&output).unwrap();
    let linear = baked.sheet("/Textures/Effects/bullet_hole.rsi/bullet_hole.png").unwrap().pixels;
    assert_eq!(linear, [188, 0, 255, 200]);
    assert!((crate::colour::byte_to_linear(linear[0]) - 128.0 / 255.0).abs() < 0.005);
    assert_eq!(baked.sheet("/Textures/Effects/plain.rsi/plain.png").unwrap().pixels, [128, 0, 255, 200]);
}

#[test]
fn a_file_from_an_older_editor_is_refused_with_a_reason() {
    let folder = Folder::new("old");
    let path = folder.0.join("textures.sstx");
    std::fs::write(&path, b"SSTX1\0\0\0\0\0\0\0\0\0\0\0").unwrap();
    let error = BakedTextures::open(&path).err().unwrap();
    assert!(error.contains("older editor"), "{error}");
}
