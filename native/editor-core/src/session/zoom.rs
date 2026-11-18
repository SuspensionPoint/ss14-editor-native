//! The wheel's zoom, given over a few frames so that a notch of scroll glides.
//!
//! A notch that zooms does not reach the camera at once. The session keeps the zoom still owed
//! and the point of the view it zooms about, and each frame gives a part of it
//! ([`Session::advance_zoom`]): nine tenths in a tenth of a second. What is left under a
//! fortieth of a notch is given at once, and the glide is over. A notch that comes during a glide adds to
//! what is owed, and the rest goes about the cursor of the latest. What takes the wheel for itself
//! (R held turning, the select and pick lists stepping) takes whole notches and owes nothing.

use super::*;

/// How many steps of [`Session::zoom`] one notch of the wheel is.
pub(super) const ZOOM_STEPS_PER_NOTCH: f32 = 0.8;

/// The share of what is owed that a glide gives in `IN` seconds.
const SHARE: f32 = 0.9;
const IN: f32 = 0.1;
/// What is left of a glide that is given at once, in notches.
const REST: f32 = 1.0 / 40.0;
/// The longest frame a glide is moved on by: a tenth of a second.
const LONGEST: f32 = 0.1;
/// The frame a glide begins in. The interface was idle before it, and its frame time says how
/// long it was idle, so a sixtieth of a second is taken for such a frame.
const FIRST: f32 = 1.0 / 60.0;

/// The zoom the wheel asked for and the frames have yet to give.
#[derive(Default)]
pub(super) struct ZoomGlide {
    /// Notches owed, positive to zoom in.
    owed: f32,
    /// The point of the view the zoom goes about, in device pixels, and the view's size.
    about: (f32, f32),
    view: (u32, u32),
    /// Set while no frame has given any of a glide.
    fresh: bool,
}

impl Session {
    /// The wheel turned by `notches` over a point of the view and nothing took it. The zoom is
    /// owed, and the frames give it.
    pub(super) fn owe_zoom(&mut self, notches: f32, point: (f32, f32), view: (u32, u32)) {
        let glide = &mut self.glide;
        glide.fresh |= glide.owed == 0.0;
        glide.owed += notches;
        (glide.about, glide.view) = (point, view);
    }

    /// Gives the part of the owed zoom that a frame of `seconds` gives, and says whether more is
    /// owed. The interface calls it once a frame and asks for another frame while it says so.
    pub fn advance_zoom(&mut self, seconds: f32) -> bool {
        let glide = &mut self.glide;
        if glide.owed == 0.0 {
            return false;
        }
        let seconds = if std::mem::take(&mut glide.fresh) { FIRST } else { seconds.clamp(0.0, LONGEST) };
        let given = if glide.owed.abs() < REST { glide.owed } else { glide.owed * (1.0 - (1.0 - SHARE).powf(seconds / IN)) };
        glide.owed -= given;
        let (about, view) = (glide.about, glide.view);
        self.zoom(given * ZOOM_STEPS_PER_NOTCH, about, view);
        self.zooming()
    }

    /// Whether the wheel's zoom has more to give.
    pub fn zooming(&self) -> bool {
        self.glide.owed != 0.0
    }

    /// Drops the zoom still owed, as when the view is set outright.
    pub(super) fn stop_zoom(&mut self) {
        self.glide = ZoomGlide::default();
    }
}
