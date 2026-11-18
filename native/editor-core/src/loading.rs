//! The loading screen's model: the stages of one load, which are done, which failed and why, and
//! when each finished. It also drives the animation. As each stage finishes, a floor tile drops
//! into its slot and a clown hops onto it. An interface draws [`Loading::frame`] every frame and
//! asks for another while something is still moving.
//!
//! Positions are in CSS pixels within the stage area. x runs from the left edge of the row of
//! slots and y runs down from the top of the area. The clown's position is the middle of its feet.

use std::f32::consts::PI;
use std::path::Path;
use std::time::Instant;

/// A stage of loading, as the engine's thread and the interface report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The fork's prototypes packed into the cache, when they were not there.
    Pack,
    /// The fork's textures baked into the cache, when they were not there.
    Bake,
    /// The .NET runtime loaded into the process.
    Runtime,
    /// Both kernels up with the fork's content, and the warm-up run.
    Kernels,
    /// The engine's lists of tiles, prototypes, decals and ports read.
    Prototypes,
    /// The engine opened the map, or started an empty one.
    Map,
    /// The map's draw list reached the session.
    Picture,
    /// The interface put the map's sprites on the GPU and drew it.
    Textures,
}

/// The side of a floor tile.
pub const TILE: f32 = 64.0;
/// The height of the area the row and the clown are in. The row stands on its bottom.
pub const AREA_HEIGHT: f32 = 190.0;
/// How wide the clown is drawn. Its height keeps the picture's proportions.
pub const CLOWN_WIDTH: f32 = 64.0;
/// Where the clown waits before the first tile: just left of the row, on the row's floor.
pub const CLOWN_START: (f32, f32) = (-TILE / 2.0, AREA_HEIGHT);

/// The least time between two tiles dropping, so that stages done together still drop in turn.
const STEP: f32 = 0.42;
/// A tile's drop, and how far above its slot it starts.
const DROP: f32 = 0.28;
const DROP_FROM: f32 = 80.0;
/// The clown hops this long after its tile starts to drop.
const HOP_DELAY: f32 = 0.16;
const HOP: f32 = 0.38;
const HOP_HEIGHT: f32 = 34.0;
/// The squash after a hop lands.
const LANDING: f32 = 0.22;
/// The slip off the row after a failure, and how far into it the clown is out of sight.
const SLIP: f32 = 0.9;
const SLIP_END: f32 = 1.2;
const FADE: f32 = 0.6;

/// One load: the stages, with when each finished in seconds from the start.
#[derive(Clone, Debug)]
pub struct Loading {
    begun: Instant,
    stages: Vec<(Step, String)>,
    done: Vec<Option<f32>>,
    /// The stage that failed, the engine's error, and when.
    failed: Option<(usize, String, f32)>,
}

/// Where the clown is and how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClownPose {
    pub x: f32,
    pub y: f32,
    /// Wider and lower by this much, around its feet: the picture is drawn scaled by
    /// `1 + 0.18 * squash` across and `1 - 0.18 * squash` up.
    pub squash: f32,
    /// Turned clockwise around its feet, in degrees.
    pub turn: f32,
    pub opacity: f32,
}

/// A tile on its way into its slot: how far above the slot it is, and how solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TilePose {
    pub lift: f32,
    pub opacity: f32,
}

/// What the screen shows at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// One per slot: None while the slot is empty.
    pub tiles: Vec<Option<TilePose>>,
    pub clown: ClownPose,
    /// The whole screen's opacity, which falls as it fades into the editor.
    pub opacity: f32,
    /// Whether anything moves, so that another frame is wanted.
    pub moving: bool,
    /// Whether the screen has faded away.
    pub gone: bool,
}

impl Loading {
    /// The first start of the editor: the engine's start, then the map's stages. An empty
    /// map is opened as quickly as the engine is ready, and shows only the start.
    pub fn first_start(map: Option<&str>, begun: Instant) -> Loading {
        Loading::first_start_after(Vec::new(), map, begun)
    }

    /// The first start with stages before the engine's own: the fork's pack and textures,
    /// made when the cache did not have them.
    pub fn first_start_after(before: Vec<(Step, String)>, map: Option<&str>, begun: Instant) -> Loading {
        let mut stages = before;
        stages.extend([
            (Step::Runtime, "Starting the .NET runtime".to_owned()),
            (Step::Kernels, "Starting the engine kernels and warming up".to_owned()),
            (Step::Prototypes, "Reading the fork's prototypes".to_owned()),
        ]);
        if let Some(map) = map {
            stages.extend(map_stages(Some(map)));
        }
        Loading::of(stages, begun)
    }

    /// A map opened later, or an empty one started: only the map's stages.
    pub fn open(map: Option<&str>, begun: Instant) -> Loading {
        Loading::of(map_stages(map), begun)
    }

    /// A map asked for while this load is still under way: the stages of the engine's start
    /// are kept as they are, and the map's stages are those of the new map.
    pub fn reopen(&self, map: Option<&str>) -> Loading {
        let start = self.stages.iter().take_while(|(step, _)| matches!(step, Step::Pack | Step::Bake | Step::Runtime | Step::Kernels | Step::Prototypes)).count();
        let mut stages = self.stages[..start].to_vec();
        stages.extend(map_stages(map));
        let mut done = self.done[..start].to_vec();
        done.resize(stages.len(), None);
        Loading { begun: self.begun, stages, done, failed: None }
    }

    fn of(stages: Vec<(Step, String)>, begun: Instant) -> Loading {
        let done = vec![None; stages.len()];
        Loading { begun, stages, done, failed: None }
    }

    fn seconds(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.begun).as_secs_f32()
    }

    pub fn stages(&self) -> &[(Step, String)] {
        &self.stages
    }

    /// When each stage finished, in seconds from the start of the load.
    pub fn done(&self) -> &[Option<f32>] {
        &self.done
    }

    /// The stage that failed, by its index, and the engine's error.
    pub fn failure(&self) -> Option<(usize, &str)> {
        self.failed.as_ref().map(|(index, error, _)| (*index, error.as_str()))
    }

    /// The stage under way, by its index. None once all are done, or one failed.
    pub fn current(&self) -> Option<usize> {
        if self.failed.is_some() {
            return None;
        }
        self.done.iter().position(Option::is_none)
    }

    /// The step of the stage under way.
    pub fn current_step(&self) -> Option<Step> {
        self.current().map(|index| self.stages[index].0)
    }

    pub fn finished(&self) -> bool {
        self.failed.is_none() && self.done.iter().all(Option::is_some)
    }

    /// A stage finished. Any before it that had not are taken as done with it. A step this
    /// load does not have, or one after a failure, changes nothing.
    pub fn finish(&mut self, step: Step, now: Instant) {
        if self.failed.is_some() {
            return;
        }
        let Some(index) = self.stages.iter().position(|(have, _)| *have == step) else { return };
        let at = self.seconds(now);
        for done in &mut self.done[..=index] {
            done.get_or_insert(at);
        }
    }

    /// The stage under way failed, with the engine's error. Nothing fails once all are done.
    pub fn fail(&mut self, error: &str, now: Instant) {
        if let Some(index) = self.current() {
            self.failed = Some((index, error.to_owned(), self.seconds(now)));
        }
    }

    /// The line under the row: the stage under way with its number, the stage that failed, or
    /// that all are done.
    pub fn stage_line(&self) -> (String, Option<String>) {
        if let Some((index, _, _)) = &self.failed {
            return (format!("{} failed.", self.stages[*index].1), None);
        }
        match self.current() {
            Some(index) => (format!("{}...", self.stages[index].1), Some(format!("({} of {})", index + 1, self.stages.len()))),
            None => ("Ready.".to_owned(), None),
        }
    }

    /// When each finished stage's tile drops, in seconds from the start: when it finished, but
    /// never sooner than [`STEP`] after the tile before.
    fn laid(&self) -> Vec<f32> {
        let mut laid: Vec<f32> = Vec::new();
        for done in self.done.iter().map_while(|done| *done) {
            laid.push(laid.last().map_or(done, |before| done.max(before + STEP)));
        }
        laid
    }

    /// The screen at this moment.
    pub fn frame(&self, now: Instant) -> Frame {
        let t = self.seconds(now);
        let laid = self.laid();
        let tiles = (0..self.stages.len()).map(|index| laid.get(index).filter(|&&at| t >= at).map(|&at| tile_drop(t - at))).collect();

        // The last hop begun, and where the clown is when it rests.
        let hopped = laid.iter().rposition(|&at| t >= at + HOP_DELAY);
        let (mut clown, rest) = match hopped {
            None => (resting(CLOWN_START, t, None), CLOWN_START),
            Some(index) => {
                let begun = laid[index] + HOP_DELAY;
                let k = (t - begun) / HOP;
                let to = slot_top(index);
                if k < 1.0 {
                    let from = if index == 0 { CLOWN_START } else { slot_top(index - 1) };
                    let (x, y, squash) = hop(from, to, HOP_HEIGHT, k);
                    (ClownPose { x, y, squash, turn: 0.0, opacity: 1.0 }, to)
                } else {
                    (resting(to, t, Some(begun + HOP)), to)
                }
            }
        };
        // When the last hop there will be has landed.
        let settled = laid.last().map_or(0.0, |at| at + HOP_DELAY + HOP + LANDING);

        let mut frame = Frame { tiles, clown, opacity: 1.0, moving: true, gone: false };
        if let Some((_, _, failed)) = &self.failed {
            let begun = failed.max(settled);
            if t >= begun {
                let k = (t - begun) / SLIP;
                clown = slip(rest, k);
                frame.clown = clown;
                frame.moving = k < SLIP_END;
            }
        } else if self.finished() {
            let k = ((t - settled) / FADE).clamp(0.0, 1.0);
            frame.opacity = 1.0 - EASE.at(k);
            frame.gone = t >= settled + FADE;
            frame.moving = !frame.gone;
        }
        frame
    }
}

/// The map's stages: opening it, its first picture, and its textures.
fn map_stages(map: Option<&str>) -> Vec<(Step, String)> {
    let opening = match map {
        Some(map) => format!("Opening {}", Path::new(map).file_name().map_or(map.into(), |name| name.to_string_lossy())),
        None => "Starting an empty map".to_owned(),
    };
    vec![(Step::Map, opening), (Step::Picture, "Receiving the first picture".to_owned()), (Step::Textures, "Loading textures".to_owned())]
}

/// Where the clown stands on the tile of a slot.
pub fn slot_top(index: usize) -> (f32, f32) {
    (index as f32 * TILE + TILE / 2.0, AREA_HEIGHT - TILE)
}

/// The clown standing at a place: the squash of landing there, if it hopped there, then a
/// gentle bob that starts from rest.
fn resting(at: (f32, f32), t: f32, landed: Option<f32>) -> ClownPose {
    let still = ClownPose { x: at.0, y: at.1, squash: 0.0, turn: 0.0, opacity: 1.0 };
    let since = match landed {
        Some(landed) if t - landed < LANDING => return ClownPose { squash: landing((t - landed) / LANDING), ..still },
        Some(landed) => t - landed - LANDING,
        None => t,
    };
    let (drop, squash) = bob(since);
    ClownPose { y: at.1 + drop, squash, ..still }
}

/// A hop in an arc of the given height, at `k` from 0 to 1 of the way: where the clown is, and
/// its squash as it pushes off.
pub fn hop(from: (f32, f32), to: (f32, f32), height: f32, k: f32) -> (f32, f32, f32) {
    let k = k.clamp(0.0, 1.0);
    let x = from.0 + (to.0 - from.0) * k;
    let y = from.1 + (to.1 - from.1) * k - (PI * k).sin() * height;
    let squash = if k < 0.12 { (0.12 - k) / 0.12 * 0.6 } else { 0.0 };
    (x, y, squash)
}

/// The squash of landing, at `k` from 0 to 1 of the way through it.
pub fn landing(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    (PI * k).sin() * (1.0 - k) * 1.2
}

/// The bob while the clown waits, `t` seconds after it started: how far down it is, and its squash.
pub fn bob(t: f32) -> (f32, f32) {
    let wave = (t * 3.0).sin();
    (wave * 2.5, wave.max(0.0) * 0.12)
}

/// A tile `t` seconds into its drop: with a little overshoot past its slot, and solid by 60%
/// of the way.
pub fn tile_drop(t: f32) -> TilePose {
    let k = (t / DROP).clamp(0.0, 1.0);
    let lift = DROP_FROM * (1.0 - DROP_EASE.at(k));
    let opacity = DROP_EASE.at((k / 0.6).min(1.0)).clamp(0.0, 1.0);
    TilePose { lift, opacity }
}

/// The slip off the end of the row after a failure, at `k` of [`SLIP`]: a little rise, then a
/// fall that turns and fades. Out of sight by `k` of 1.
pub fn slip(from: (f32, f32), k: f32) -> ClownPose {
    let k = k.max(0.0);
    let x = from.0 + k * 50.0;
    let y = from.1 - (k.min(0.5) * PI).sin() * 18.0 + (k - 0.3).max(0.0).powi(2) * 400.0;
    let opacity = (1.0 - (k - 0.6).max(0.0) * 2.5).max(0.0);
    ClownPose { x, y, squash: 0.0, turn: k * 160.0, opacity }
}

/// A CSS cubic Bézier timing function, from (0, 0) to (1, 1) through two control points.
#[derive(Clone, Copy, Debug)]
pub struct Bezier(pub f32, pub f32, pub f32, pub f32);

/// The mockup's drop, `cubic-bezier(.3, 1.5, .5, 1)`, and CSS's `ease` for the fade.
const DROP_EASE: Bezier = Bezier(0.3, 1.5, 0.5, 1.0);
const EASE: Bezier = Bezier(0.25, 0.1, 0.25, 1.0);

impl Bezier {
    /// The value at `x` of the way through time.
    pub fn at(&self, x: f32) -> f32 {
        if x <= 0.0 || x >= 1.0 {
            return x.clamp(0.0, 1.0);
        }
        let curve = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3);
        // The curve's x grows with s, so halving finds the s of this x.
        let (mut low, mut high) = (0.0f32, 1.0f32);
        for _ in 0..30 {
            let middle = (low + high) / 2.0;
            if curve(self.0, self.2, middle) < x {
                low = middle;
            } else {
                high = middle;
            }
        }
        curve(self.1, self.3, (low + high) / 2.0)
    }
}

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;
