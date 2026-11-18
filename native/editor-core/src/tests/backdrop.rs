use super::*;

fn placing(tiled: bool, slowness: f32) -> Placing {
    Placing { scale_x: 1.0, scale_y: 1.0, tiled, home_x: 0.0, home_y: 0.0, adjust_x: 0.0, adjust_y: 0.0, slowness, scroll_x: 0.0, scroll_y: 0.0 }
}

#[test]
fn a_layer_shown_once_is_centred_on_its_home_and_follows_the_eye_by_its_slowness() {
    // 320 pixels are 10 tiles.
    assert_eq!(placing(false, 0.0).place((320, 320), (100.0, 40.0), 0.0), ((-5.0, -5.0), (10.0, 10.0)));
    assert_eq!(placing(false, 1.0).place((320, 320), (100.0, 40.0), 0.0), ((95.0, 35.0), (10.0, 10.0)));
    assert_eq!(placing(false, 0.5).place((320, 320), (100.0, 40.0), 0.0), ((45.0, 15.0), (10.0, 10.0)));
}

#[test]
fn a_tiled_layer_is_placed_at_the_repeat_under_the_eye() {
    let ((x, y), (width, height)) = placing(true, 0.0).place((320, 320), (103.0, -42.0), 0.0);
    assert_eq!((width, height), (10.0, 10.0));
    // Corners repeat at -5 + 10 n; the eye is inside the repeat that starts here.
    assert_eq!((x, y), (95.0, -45.0));
}

/// A layer of one colour, `pixels` each way.
fn plain(pixels: u32, colour: [u8; 4], placing: Placing) -> Layer {
    Layer { picture: Picture::new(pixels, pixels, colour.repeat((pixels * pixels) as usize)), placing }
}

#[test]
fn a_picture_of_a_backdrop_has_its_tiled_layers_everywhere_and_the_others_at_home() {
    // A grey that repeats, and over it a red square of 320 pixels (10 tiles) at home,
    // which is a quarter of the 40 tiles the picture shows across.
    let backdrop = Backdrop {
        layers: vec![plain(64, [40, 40, 40, 255], placing(true, 0.0)), plain(320, [255, 0, 0, 255], placing(false, 0.0))],
        ..Default::default()
    };
    let thumb = thumbnail(&backdrop, (160, 90));
    assert_eq!((thumb.width, thumb.height, thumb.pixels.len()), (160, 90, 160 * 90 * 4));
    let at = |x: u32, y: u32| &thumb.pixels[((y * 160 + x) * 4) as usize..][..4];
    assert_eq!(at(80, 45), [255, 0, 0, 255]);
    assert_eq!(at(2, 2), [40, 40, 40, 255]);
    // The red square is 40 pixels across, about the middle.
    let red = (0..160).filter(|&x| at(x, 45)[0] == 255).count();
    assert_eq!(red, 40);

    // With no layers, black.
    let none = thumbnail(&Backdrop::default(), (4, 2));
    assert_eq!(none.pixels, [0, 0, 0, 255].repeat(8));
}

#[test]
fn a_half_clear_layer_lets_what_is_under_it_through() {
    let backdrop = Backdrop { layers: vec![plain(4, [100, 0, 0, 128], placing(true, 0.0))], ..Default::default() };
    // Premultiplied red at half over black.
    assert_eq!(thumbnail(&backdrop, (2, 2)).pixels[..4], [100, 0, 0, 255]);
}

#[test]
fn smaller_copies_halve_down_to_one_pixel() {
    let picture = Picture::new(4, 2, vec![100; 4 * 2 * 4]);
    assert_eq!(picture.levels.len(), 3);
    assert_eq!(picture.level_size(1), (2, 1));
    assert_eq!(picture.level_size(2), (1, 1));
    assert_eq!(picture.levels[2], vec![100; 4]);
}
