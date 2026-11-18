use std::time::Duration;

use super::*;

fn steps(loading: &Loading) -> Vec<Step> {
    loading.stages().iter().map(|(step, _)| *step).collect()
}

fn after(begun: Instant, seconds: f32) -> Instant {
    begun + Duration::from_secs_f32(seconds)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn the_first_start_has_the_engine_s_stages_then_the_map_s() {
    let loading = Loading::first_start(Some("Maps/_Starlight/Stations/Starboard.yml"), Instant::now());
    assert_eq!(steps(&loading), [Step::Runtime, Step::Kernels, Step::Prototypes, Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(loading.stages()[3].1, "Opening Starboard.yml");
    assert_eq!(loading.stage_line(), ("Starting the .NET runtime...".to_owned(), Some("(1 of 6)".to_owned())));
}

#[test]
fn a_start_with_no_map_has_only_the_engine_s_stages() {
    let loading = Loading::first_start(None, Instant::now());
    assert_eq!(steps(&loading), [Step::Runtime, Step::Kernels, Step::Prototypes]);
}

#[test]
fn a_fork_s_pack_and_textures_come_before_the_engine_and_stay_for_a_map_asked_for() {
    let begun = Instant::now();
    let before = vec![(Step::Pack, "Packing Starlight's prototypes for the first time".to_owned()), (Step::Bake, "Baking".to_owned())];
    let mut loading = Loading::first_start_after(before, Some("Bagel.yml"), begun);
    assert_eq!(steps(&loading), [Step::Pack, Step::Bake, Step::Runtime, Step::Kernels, Step::Prototypes, Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(loading.stage_line(), ("Packing Starlight's prototypes for the first time...".to_owned(), Some("(1 of 8)".to_owned())));
    loading.finish(Step::Pack, after(begun, 2.0));
    let again = loading.reopen(None);
    assert_eq!(steps(&again), [Step::Pack, Step::Bake, Step::Runtime, Step::Kernels, Step::Prototypes, Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(again.current_step(), Some(Step::Bake));
}

#[test]
fn a_later_open_has_only_the_map_s_stages() {
    let loading = Loading::open(Some("C:/maps/Bagel.yml"), Instant::now());
    assert_eq!(steps(&loading), [Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(loading.stages()[0].1, "Opening Bagel.yml");
    assert_eq!(Loading::open(None, Instant::now()).stages()[0].1, "Starting an empty map");
}

#[test]
fn a_map_asked_for_during_the_start_keeps_the_start_s_stages() {
    let begun = Instant::now();
    let mut loading = Loading::first_start(Some("Bagel.yml"), begun);
    loading.finish(Step::Runtime, after(begun, 1.0));
    let again = loading.reopen(Some("Cork.yml"));
    assert_eq!(steps(&again), [Step::Runtime, Step::Kernels, Step::Prototypes, Step::Map, Step::Picture, Step::Textures]);
    assert_eq!(again.stages()[3].1, "Opening Cork.yml");
    assert_eq!(again.current(), Some(1));
}

#[test]
fn stages_complete_in_order_and_the_row_fills() {
    let begun = Instant::now();
    let mut loading = Loading::open(Some("Bagel.yml"), begun);
    assert_eq!(loading.current_step(), Some(Step::Map));

    // A step the load does not have changes nothing.
    loading.finish(Step::Kernels, after(begun, 0.5));
    assert_eq!(loading.current(), Some(0));

    loading.finish(Step::Map, after(begun, 1.0));
    assert_eq!(loading.stage_line(), ("Receiving the first picture...".to_owned(), Some("(2 of 3)".to_owned())));
    // The picture and the textures in the same frame: done together, laid in turn.
    loading.finish(Step::Picture, after(begun, 2.0));
    loading.finish(Step::Textures, after(begun, 2.0));
    assert!(loading.finished());
    assert_eq!(loading.done(), [Some(1.0), Some(2.0), Some(2.0)]);
    assert_eq!(loading.stage_line().0, "Ready.");

    // At 2.1 s the second tile is dropping and the third is not yet.
    let frame = loading.frame(after(begun, 2.1));
    assert!(frame.tiles[0].is_some() && frame.tiles[1].is_some() && frame.tiles[2].is_none());
    assert!(frame.moving && !frame.gone && frame.opacity == 1.0);

    // The third drops at 2.42 s and its hop lands at 2.96 s. The fade follows the landing.
    let landed = 2.42 + 0.16 + 0.38 + 0.22;
    assert!(loading.frame(after(begun, landed - 0.01)).opacity == 1.0);
    let fading = loading.frame(after(begun, landed + 0.3));
    assert!(fading.opacity > 0.0 && fading.opacity < 1.0 && fading.moving);
    let gone = loading.frame(after(begun, landed + 0.61));
    assert!(gone.gone && !gone.moving && gone.opacity == 0.0);
}

#[test]
fn a_later_stage_finishing_finishes_those_before_it() {
    let begun = Instant::now();
    let mut loading = Loading::first_start(Some("Bagel.yml"), begun);
    loading.finish(Step::Prototypes, after(begun, 3.0));
    assert_eq!(&loading.done()[..4], [Some(3.0), Some(3.0), Some(3.0), None]);
}

#[test]
fn a_failure_stops_the_load_and_the_clown_slips_away() {
    let begun = Instant::now();
    let mut loading = Loading::first_start(Some("Bagel.yml"), begun);
    loading.finish(Step::Runtime, after(begun, 0.5));
    loading.finish(Step::Kernels, after(begun, 2.0));
    loading.finish(Step::Prototypes, after(begun, 3.0));
    loading.fail("Could not find file 'Bagel.yml'.", after(begun, 3.5));

    assert_eq!(loading.failure(), Some((3, "Could not find file 'Bagel.yml'.")));
    assert_eq!(loading.stage_line(), ("Opening Bagel.yml failed.".to_owned(), None));
    assert_eq!(loading.current(), None);
    assert!(!loading.finished());
    // Nothing finishes after a failure.
    loading.finish(Step::Map, after(begun, 4.0));
    assert_eq!(loading.done()[3], None);

    // The slip begins once the third hop has landed, at 3.76 s, from the third tile.
    let landed = 3.0 + 0.16 + 0.38 + 0.22;
    let start = loading.frame(after(begun, landed));
    assert!(near(start.clown.x, slot_top(2).0) && near(start.clown.y, slot_top(2).1));
    let falling = loading.frame(after(begun, landed + 0.9 * 0.8));
    assert!(falling.clown.x > slot_top(2).0 && falling.clown.y > AREA_HEIGHT && falling.clown.turn > 90.0 && falling.clown.opacity < 1.0);
    assert!(falling.moving);

    // Out of sight, and still: no more frames are wanted, and the screen stays.
    let still = loading.frame(after(begun, landed + 0.9 * 1.3));
    assert_eq!((still.clown.opacity, still.moving, still.gone, still.opacity), (0.0, false, false, 1.0));
    assert_eq!(still.tiles.iter().filter(|tile| tile.is_some()).count(), 3);
}

#[test]
fn nothing_fails_once_all_are_done() {
    let begun = Instant::now();
    let mut loading = Loading::first_start(None, begun);
    loading.finish(Step::Prototypes, after(begun, 1.0));
    loading.fail("late", after(begun, 2.0));
    assert_eq!(loading.failure(), None);
}

#[test]
fn the_hop_starts_and_ends_where_it_should_and_rises_by_its_height() {
    let (from, to) = (CLOWN_START, slot_top(0));
    let (x, y, squash) = hop(from, to, 34.0, 0.0);
    assert_eq!((x, y), from);
    // It pushes off squashed.
    assert!(near(squash, 0.6));
    let (x, y, squash) = hop(from, to, 34.0, 1.0);
    assert!(near(x, to.0) && near(y, to.1) && squash == 0.0);
    // Half way it is the height above the line between the two.
    let (x, y, _) = hop(from, to, 34.0, 0.5);
    assert!(near(x, (from.0 + to.0) / 2.0) && near(y, (from.1 + to.1) / 2.0 - 34.0));
}

#[test]
fn the_clown_waits_left_of_the_row_and_lands_on_each_tile() {
    let begun = Instant::now();
    let mut loading = Loading::open(Some("Bagel.yml"), begun);
    let waiting = loading.frame(after(begun, 0.0));
    assert_eq!((waiting.clown.x, waiting.clown.y), CLOWN_START);
    // It bobs a little where it stands.
    let bobbing = loading.frame(after(begun, 0.4));
    assert!(near(bobbing.clown.x, CLOWN_START.0) && bobbing.clown.y != CLOWN_START.1 && (bobbing.clown.y - CLOWN_START.1).abs() <= 2.5);

    loading.finish(Step::Map, after(begun, 1.0));
    // Mid hop it is above both ends.
    let mid = loading.frame(after(begun, 1.0 + 0.16 + 0.19));
    assert!(mid.clown.y < slot_top(0).1);
    // Landed, squashed, on the first tile.
    let landed = loading.frame(after(begun, 1.0 + 0.16 + 0.38 + 0.05));
    assert!(near(landed.clown.x, slot_top(0).0) && near(landed.clown.y, slot_top(0).1) && landed.clown.squash > 0.0);
}

#[test]
fn a_tile_drops_from_above_with_a_little_overshoot_and_settles() {
    assert_eq!(tile_drop(0.0), TilePose { lift: 80.0, opacity: 0.0 });
    let settled = tile_drop(0.28);
    assert!(near(settled.lift, 0.0) && near(settled.opacity, 1.0));
    // Past its slot for a moment on the way.
    assert!((1..28).any(|step| tile_drop(step as f32 / 100.0).lift < -1.0));
}

#[test]
fn the_timing_curves_run_from_nothing_to_all() {
    for curve in [DROP_EASE, EASE] {
        assert!(near(curve.at(0.0), 0.0) && near(curve.at(1.0), 1.0));
    }
    // CSS's ease is a little past half at a quarter of the time.
    assert!(near(EASE.at(0.25), 0.41));
}
