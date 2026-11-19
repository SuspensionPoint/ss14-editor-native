//! Colour spaces, as the game's renderer uses them.
//!
//! Clyde loads every texture as sRGB (TextureLoadParameters.Default has Srgb set) and draws into
//! an sRGB target, so the GPU decodes each texel to linear light, blends in linear and encodes
//! the result again. The modulate colour of a sprite is written in sRGB and turned to linear in
//! the vertex shader (`zFromSrgb` in z-library.glsl). The editor's renderer does the same on the
//! GPU, and the pictures painted on the CPU use these functions to match it.

use std::sync::OnceLock;

/// An sRGB value from 0 to 1 in linear light, by the formula of Clyde's `zFromSrgb`.
pub fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear value from 0 to 1 in sRGB, by the formula of Clyde's `zToSrgb`.
pub fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        value.powf(1.0 / 2.4) * 1.055 - 0.055
    }
}

/// An 8-bit sRGB channel in linear light, from a table made once.
pub fn byte_to_linear(byte: u8) -> f32 {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|at| srgb_to_linear(at as f32 / 255.0)))[byte as usize]
}

/// A linear value as an 8-bit sRGB channel, rounded to the nearest.
pub fn linear_to_byte(value: f32) -> u8 {
    (linear_to_srgb(value.clamp(0.0, 1.0)) * 255.0).round() as u8
}

/// Multiplies the alpha into the colour of 8-bit straight RGBA pixels, in place. Blitz and the
/// palette pictures take pictures in that form.
pub fn premultiply(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * alpha + 127) / 255) as u8;
            }
        }
    }
}

/// A picture being painted in linear light, alpha multiplied in, the way the GPU blends. Each
/// pixel is red, green, blue and alpha.
pub struct LinearPicture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}

impl LinearPicture {
    /// A picture of one colour, given as 8-bit straight sRGB.
    pub fn filled(width: u32, height: u32, colour: [u8; 4]) -> LinearPicture {
        let alpha = colour[3] as f32 / 255.0;
        let pixel = [byte_to_linear(colour[0]) * alpha, byte_to_linear(colour[1]) * alpha, byte_to_linear(colour[2]) * alpha, alpha];
        LinearPicture { width, height, pixels: vec![pixel; (width * height) as usize] }
    }

    /// Lays an 8-bit straight sRGB pixel over one of the picture, tinted by a colour whose red,
    /// green and blue are linear and whose alpha is plain, as the game blends a sprite.
    pub fn blend(&mut self, at: usize, source: &[u8], tint: [f32; 4]) {
        let alpha = source[3] as f32 / 255.0 * tint[3];
        if alpha <= 0.0 {
            return;
        }
        let target = &mut self.pixels[at];
        for channel in 0..3 {
            target[channel] = byte_to_linear(source[channel]) * tint[channel] * alpha + target[channel] * (1.0 - alpha);
        }
        target[3] = alpha + target[3] * (1.0 - alpha);
    }

    /// The picture as 8-bit sRGB with the alpha multiplied in, the form Blitz is handed.
    pub fn to_premultiplied_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            let alpha = pixel[3].clamp(0.0, 1.0);
            if alpha <= 0.0 {
                bytes.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let straight = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha].map(linear_to_byte);
            let alpha_byte = (alpha * 255.0).round() as u8;
            let mut out = [straight[0], straight[1], straight[2], alpha_byte];
            premultiply(&mut out);
            bytes.extend_from_slice(&out);
        }
        bytes
    }
}

#[cfg(test)]
#[path = "tests/colour.rs"]
mod tests;
