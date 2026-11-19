//! The View menu's Animate sprites switch, kept in the settings file for every fork.
//!
//! With it on, the renderer moves animated sprites through their frames
//! (`MapRenderer::animate`), and the interface draws again for as long as one is in view. With
//! it off, every sprite shows the frame the engine recorded and nothing is drawn again for it.
//! It is off until a mapper turns it on. Tests in `tools/tests/animate.rs`.

use super::*;
use crate::forks::settings::Settings;

/// The switch, and the settings file it is kept in. None keeps it in the session.
#[derive(Default)]
pub(super) struct Animate {
    on: bool,
    kept: Option<PathBuf>,
}

impl Animate {
    /// The switch as a settings file keeps it, kept there again at each change.
    pub(super) fn load(file: PathBuf) -> Animate {
        Animate { on: Settings::load(&file).animate_sprites, kept: Some(file) }
    }
}

impl Session {
    /// Whether animated sprites move.
    pub fn animate_sprites(&self) -> bool {
        self.animate.on
    }

    /// Turns animated sprites on or off, and keeps the choice.
    pub fn set_animate_sprites(&mut self, on: bool) {
        self.animate.on = on;
        if let Some(file) = &self.animate.kept {
            if let Err(error) = Settings::update(file, |settings| settings.animate_sprites = on) {
                self.set_status(format!("Couldn't save the Animate sprites switch: {error}"));
            }
        }
    }
}
