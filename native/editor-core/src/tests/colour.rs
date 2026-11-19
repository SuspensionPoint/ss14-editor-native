use super::*;

#[test]
fn the_ends_and_a_middle_grey_convert_as_clyde_does() {
    assert_eq!(srgb_to_linear(0.0), 0.0);
    assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
    // 128 of 255 in sRGB is about a fifth of the light of white.
    assert!((byte_to_linear(128) - 0.2159).abs() < 1e-3);
    // Half the light of white is 188 of 255 in sRGB.
    assert_eq!(linear_to_byte(0.5), 188);
    // The straight part near black.
    assert!((srgb_to_linear(0.04) - 0.04 / 12.92).abs() < 1e-7);
}

#[test]
fn every_byte_comes_back_through_linear() {
    for byte in 0..=255u8 {
        assert_eq!(linear_to_byte(byte_to_linear(byte)), byte);
    }
}

#[test]
fn premultiplying_leaves_opaque_pixels_and_scales_the_rest() {
    let mut pixels = [200, 100, 50, 255, 200, 100, 50, 128, 9, 9, 9, 0];
    premultiply(&mut pixels);
    assert_eq!(pixels, [200, 100, 50, 255, 100, 50, 25, 128, 0, 0, 0, 0]);
}

#[test]
fn a_half_clear_colour_over_black_is_blended_in_linear_light() {
    // A red of 200 at half alpha lands at half its light, which is 147 in sRGB, where blending
    // the sRGB values would give 100.
    let mut picture = LinearPicture::filled(1, 1, [0, 0, 0, 255]);
    picture.blend(0, &[200, 0, 0, 128], [1.0; 4]);
    assert_eq!(picture.to_premultiplied_bytes(), [147, 0, 0, 255]);
}

#[test]
fn a_tint_multiplies_in_linear_light() {
    let mut picture = LinearPicture::filled(1, 1, [0; 4]);
    picture.blend(0, &[255, 255, 255, 255], [srgb_to_linear(128.0 / 255.0), 1.0, 0.0, 1.0]);
    assert_eq!(picture.to_premultiplied_bytes(), [128, 255, 0, 255]);
}

#[test]
fn a_clear_picture_stays_clear() {
    let picture = LinearPicture::filled(2, 1, [0; 4]);
    assert_eq!(picture.to_premultiplied_bytes(), [0; 8]);
}
