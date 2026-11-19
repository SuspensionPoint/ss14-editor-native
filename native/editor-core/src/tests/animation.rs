use super::*;

fn frame(next: Option<u32>, delay: f32) -> TextureSource {
    TextureSource { path: "/Textures/a.rsi".into(), state: Some("a".into()), index: 0, width: 32, height: 32, next, delay }
}

/// Three frames, 0.1, 0.2 and 0.3 seconds long, as textures 5, 6 and 7, and a still one, 9.
fn textures() -> HashMap<u32, TextureSource> {
    HashMap::from([(5, frame(Some(6), 0.1)), (6, frame(Some(7), 0.2)), (7, frame(Some(5), 0.3)), (9, frame(None, 0.0))])
}

#[test]
fn a_ring_of_frames_is_one_cycle_from_any_of_them() {
    let textures = textures();
    let mut cycles = Cycles::default();
    let (cycle, place) = cycles.of(&textures, 6).unwrap();
    assert_eq!(cycle.frames, vec![6, 7, 5]);
    assert_eq!(place, 0);
    // The others are found in the same cycle, at their places.
    let (again, place) = cycles.of(&textures, 5).unwrap();
    assert_eq!(again.frames, vec![6, 7, 5]);
    assert_eq!(place, 2);
}

#[test]
fn a_still_frame_has_no_cycle() {
    let mut cycles = Cycles::default();
    assert!(cycles.of(&textures(), 9).is_none());
    assert!(cycles.of(&textures(), 42).is_none());
}

#[test]
fn a_chain_that_does_not_close_has_no_cycle() {
    let textures = HashMap::from([(1, frame(Some(2), 0.1)), (2, frame(Some(3), 0.1))]);
    assert!(Cycles::default().of(&textures, 1).is_none());
    // Nor one that runs into a ring without coming back to where it started.
    let textures = HashMap::from([(1, frame(Some(2), 0.1)), (2, frame(Some(3), 0.1)), (3, frame(Some(2), 0.1))]);
    assert!(Cycles::default().of(&textures, 1).is_none());
}

#[test]
fn the_frame_follows_the_delays_from_where_it_started() {
    let textures = textures();
    let mut cycles = Cycles::default();
    let (cycle, place) = cycles.of(&textures, 5).unwrap();
    let cycle = cycle.clone();
    // Frame 5 shows for 0.1 s, then 6 for 0.2 s, then 7 for 0.3 s, then 5 again.
    assert_eq!(cycle.frame_after(place, 0.0), 5);
    assert_eq!(cycle.frame_after(place, 0.05), 5);
    assert_eq!(cycle.frame_after(place, 0.15), 6);
    assert_eq!(cycle.frame_after(place, 0.35), 7);
    assert_eq!(cycle.frame_after(place, 0.65), 5);
    // Started from 7, 0.3 s later it is 5.
    let (_, seven) = cycles.of(&textures, 7).unwrap();
    assert_eq!(cycle.frame_after(seven, 0.31), 5);
}

#[test]
fn a_cycle_with_no_length_stays_put() {
    let textures = HashMap::from([(1, frame(Some(2), 0.0)), (2, frame(Some(1), 0.0))]);
    let mut cycles = Cycles::default();
    let (cycle, place) = cycles.of(&textures, 2).unwrap();
    assert_eq!(cycle.frame_after(place, 3.0), 2);
}
