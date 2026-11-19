//! Animated sprites: which frame of an animated RSI state shows at a given time.
//!
//! The engine names each frame of an animated state with the frame after it and how long it
//! shows (`TextureSource::next`, `delay`, from the state's folded timeline, RSI.State.GetDelays).
//! A frame's cycle is the ring those links make. The game's client steps a sprite layer through
//! its frames by time (SpriteComponent.Layer.AdvanceFrameAnimation). Here every sprite keeps the
//! frame the engine recorded it with as its start, and moves on by the editor's clock, so
//! sprites of one state turn over together. A layer the game does not animate on its own
//! (`AutoAnimated` off) is not told apart, and turns over too.

use std::collections::HashMap;

use crate::drawlist::TextureSource;

/// The frames of one animated state in one direction, in order, with when each starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Cycle {
    pub frames: Vec<u32>,
    /// When each frame starts, in seconds from the first.
    pub starts: Vec<f32>,
    /// How long the whole cycle lasts.
    pub total: f32,
}

impl Cycle {
    /// The frame that shows `seconds` after the frame at `position` started.
    pub fn frame_after(&self, position: usize, seconds: f32) -> u32 {
        if self.total <= 0.0 || !seconds.is_finite() {
            return self.frames[position];
        }
        let at = (self.starts[position] + seconds).rem_euclid(self.total);
        // The last frame whose start is at or before the time.
        let index = self.starts.partition_point(|start| *start <= at).saturating_sub(1);
        self.frames[index]
    }
}

/// The cycles found so far, and where each texture is in its cycle.
#[derive(Default)]
pub struct Cycles {
    cycles: Vec<Cycle>,
    /// Each texture of a cycle: which cycle, and its place in it. None for a texture with no
    /// cycle, so that it is not looked for again.
    places: HashMap<u32, Option<(usize, usize)>>,
}

/// The most frames a cycle may have. A ring of links that never comes back to its start is
/// not a cycle.
const MOST_FRAMES: usize = 1024;

impl Cycles {
    /// The cycle a texture is part of, and its place in it, or None if it is not animated.
    pub fn of(&mut self, textures: &HashMap<u32, TextureSource>, id: u32) -> Option<(&Cycle, usize)> {
        if !self.places.contains_key(&id) {
            self.find(textures, id);
        }
        let (cycle, place) = (*self.places.get(&id)?)?;
        Some((&self.cycles[cycle], place))
    }

    fn find(&mut self, textures: &HashMap<u32, TextureSource>, id: u32) {
        let mut frames = vec![id];
        let mut delays = Vec::new();
        let mut at = id;
        while let Some(source) = textures.get(&at) {
            let Some(next) = source.next else { break };
            delays.push(source.delay.max(0.0));
            if next == id {
                break;
            }
            if frames.len() >= MOST_FRAMES || frames.contains(&next) {
                delays.clear();
                break;
            }
            frames.push(next);
            at = next;
        }
        // A frame that is not part of a closed ring, or a ring of one, does not move.
        if delays.len() != frames.len() || frames.len() < 2 {
            self.places.insert(id, None);
            return;
        }
        let mut starts = Vec::with_capacity(frames.len());
        let mut total = 0.0;
        for delay in &delays {
            starts.push(total);
            total += delay;
        }
        let index = self.cycles.len();
        for (place, frame) in frames.iter().enumerate() {
            self.places.insert(*frame, Some((index, place)));
        }
        self.cycles.push(Cycle { frames, starts, total });
    }
}

#[cfg(test)]
#[path = "tests/animation.rs"]
mod tests;
