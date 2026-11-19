use super::*;
use serde_json::json;

/// A message of items, each with one rectangle.
fn message(items: &[Value]) -> Vec<u8> {
    let header = json!({ "doc": 1, "full": true, "items": items }).to_string();
    let mut message = b"DRW1".to_vec();
    message.extend((header.len() as i32).to_le_bytes());
    message.extend(header.as_bytes());
    for _ in items {
        message.extend([0f32; FLOATS_PER_QUAD].iter().flat_map(|float| float.to_le_bytes()));
    }
    message
}

#[test]
fn ties_go_by_the_depth_from_before_the_floor_was_seen_through() {
    // A cable, a thin pipe and a disposal pipe on one cell, all drawn above doors, the
    // cable with the lowest id. Thick goes under thin, and thin under the cable.
    let mut lists = DrawLists::default();
    lists
        .apply(&message(&[json!([1, 10, 0, -3.0, 1, KIND_SUBFLOOR, -14]), json!([2, 10, 0, -3.0, 1, KIND_SUBFLOOR, -15]), json!([3, 10, 0, -3.0, 1, KIND_SUBFLOOR, -21])]))
        .unwrap();
    assert_eq!(lists.documents.get_mut(&1).unwrap().in_draw_order(), &[3, 2, 1]);
}

#[test]
fn without_a_tie_the_id_decides() {
    let mut lists = DrawLists::default();
    lists.apply(&message(&[json!([2, 10, 0, -3.0, 1, KIND_ENTITY]), json!([1, 10, 0, -3.0, 1, KIND_ENTITY])])).unwrap();
    assert_eq!(lists.documents.get_mut(&1).unwrap().in_draw_order(), &[1, 2]);
}

/// The game's RSIResource.FoldDelays: per direction, which frame of the state's image each
/// step of the folded timeline shows.
fn folded_frames(delays: &[Vec<f32>]) -> Vec<Vec<usize>> {
    if delays.len() == 1 {
        return vec![(0..delays[0].len()).collect()];
    }
    let mut left: Vec<Vec<i32>> = delays.iter().map(|list| list.iter().map(|delay| (delay * 1000.0) as i32).collect()).collect();
    let longest = left.iter().map(|list| list.iter().sum::<i32>()).max().unwrap();
    for list in &mut left {
        let total: i32 = list.iter().sum();
        *list.last_mut().unwrap() += longest - total;
    }
    let starts: Vec<usize> = delays.iter().scan(0, |start, list| { let at = *start; *start += list.len(); Some(at) }).collect();
    let mut at = vec![0; delays.len()];
    let mut frames = vec![Vec::new(); delays.len()];
    loop {
        let step = (0..delays.len()).map(|d| left[d][at[d]]).min().unwrap();
        for d in 0..delays.len() {
            frames[d].push(starts[d] + at[d]);
        }
        for d in 0..delays.len() {
            left[d][at[d]] -= step;
            if left[d][at[d]] == 0 {
                at[d] += 1;
            }
            if at[d] == left[d].len() {
                return frames;
            }
        }
    }
}

#[test]
fn folding_repeats_the_frames_of_a_direction_with_fewer() {
    // The arcade's screen: six frames facing south, one each other way.
    let frames = folded_frames(&[vec![0.1; 6], vec![0.6], vec![0.6], vec![0.6]]);
    assert_eq!(frames[0], [0, 1, 2, 3, 4, 5]);
    assert_eq!(frames[1], [6; 6]);
    assert_eq!(frames[3], [8; 6]);
}

/// The frame each texture of an animated state stands for, as the engine side registers it,
/// against the game's own folding of the state's delays. The arcades' screens have more
/// frames facing south than the other ways, which the engine folds into one timeline.
/// Needs a published engine side in src/Editor.Native/bin/r2r (or EDITOR_ENGINE), a fork with
/// its resources in upstream/starlight (or EDITOR_FORK) and its pack in
/// native/cache/kernel-resources.zip (or EDITOR_PACK).
/// `cargo test -p editor-core --release rsi_frames -- --ignored --nocapture`
#[test]
#[ignore]
fn rsi_frames() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let from = |name: &str, default: &str| std::env::var_os(name).map(std::path::PathBuf::from).unwrap_or_else(|| root.join(default));
    let fork = from("EDITOR_FORK", "upstream/starlight");
    let engine = crate::engine::Engine::load(&from("EDITOR_ENGINE", "src/Editor.Native/bin/r2r")).unwrap();
    let path = |path: &std::path::Path| path.to_string_lossy().replace('\\', "/");
    engine.start(&path(&fork), Some(&path(&from("EDITOR_PACK", "native/cache/kernel-resources.zip"))));
    engine.wait_ready().unwrap();

    // Space Invaders' screen is only on the abstract ArcadeBase, which has no preview.
    let prototypes = ["SpaceVillainArcade", "BlockGameArcade", "LancerArcade"];
    let items: Vec<Value> = prototypes.iter().enumerate().map(|(key, prototype)| json!({ "key": key, "prototype": prototype })).collect();
    let _: Value = engine.call("previews", &json!({ "items": items, "directions": false })).unwrap();
    let mut lists = DrawLists::default();
    lists.apply(&engine.with_draw_list(PREVIEWS, <[u8]>::to_vec).unwrap()).unwrap();

    let mut checked = 0;
    for (rsi, state) in [
        ("/Textures/Structures/Machines/arcade.rsi", "screen_spacevillain"),
        ("/Textures/Structures/Machines/arcade.rsi", "screen_blockgame"),
        ("/Textures/_Starlight/Structures/Machines/arcade.rsi", "screen_lancer"),
    ] {
        let mut ids: Vec<(&u32, &TextureSource)> = lists.textures.iter().filter(|(_, source)| source.path == rsi && source.state.as_deref() == Some(state)).collect();
        assert!(!ids.is_empty(), "{state} was not registered");
        ids.sort_by_key(|(id, _)| **id);
        let registered: Vec<u32> = ids.iter().map(|(_, source)| source.index).collect();

        let meta: Value = serde_json::from_str(std::fs::read_to_string(fork.join("Resources").join(&rsi[1..]).join("meta.json")).unwrap().trim_start_matches('\u{feff}')).unwrap();
        let entry = meta["states"].as_array().unwrap().iter().find(|entry| entry["name"] == state).unwrap();
        let delays: Vec<Vec<f32>> = entry["delays"].as_array().unwrap().iter().map(|list| list.as_array().unwrap().iter().map(|delay| delay.as_f64().unwrap() as f32).collect()).collect();
        let expected: Vec<u32> = folded_frames(&delays).into_iter().flatten().map(|frame| frame as u32).collect();
        println!("{state}: {registered:?}");
        assert_eq!(registered, expected, "{state}");
        // Every frame is inside the image: the frames are laid out in rows as wide as the image.
        let frames: usize = delays.iter().map(Vec::len).sum();
        assert!(registered.iter().all(|&index| (index as usize) < frames));
        checked += 1;
    }
    assert_eq!(checked, 3);
}
