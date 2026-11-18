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
