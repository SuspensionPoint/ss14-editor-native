use super::*;
use crate::drawlist::FLOATS_PER_QUAD;

fn finding(rule: Rule, grid: i32, cell: (i32, i32), message: &str) -> Finding {
    Finding { rule, message: message.to_owned(), grid, cell, entity: None, decal: None }
}

#[test]
fn findings_on_one_cell_make_one_badge() {
    let findings = [
        finding(Rule::StackedWalls, 2, (1, 1), "stacked"),
        finding(Rule::WallWithoutFloor, 2, (5, 0), "loose"),
        finding(Rule::Duplicates, 2, (1, 1), "twice"),
        // The same cell of another grid is another badge.
        finding(Rule::Duplicates, 3, (1, 1), "other grid"),
    ];
    let badges = badges(&findings);
    let places: Vec<(i32, (i32, i32), usize)> = badges.iter().map(|badge| (badge.grid, badge.cell, badge.found.len())).collect();
    assert_eq!(places, [(2, (1, 1), 2), (2, (5, 0), 1), (3, (1, 1), 1)]);
    // The box over a badge names its rule, or counts them, then says each finding.
    assert_eq!(badges[0].lines(), ["2 problems here", "stacked", "twice"]);
    assert_eq!(badges[1].lines(), ["Walls with no floor", "loose"]);
}

#[test]
fn a_badge_keeps_its_size_on_screen_far_out() {
    // Close up it is half a tile.
    assert_eq!(badge_size(32.0), BADGE_TILES);
    assert_eq!(badge_size(64.0), BADGE_TILES);
    // At 8 pixels a tile it is two tiles, 16 pixels.
    assert_eq!(badge_size(8.0), 2.0);
    // It changes by steps, so a small zoom keeps it.
    assert_eq!(badge_size(8.1), badge_size(8.2));
    assert_eq!(badge_size(0.0), BADGE_TILES);
}

#[test]
fn a_badge_sits_in_its_cell_s_corner_and_is_found_under_the_mouse() {
    let badges = badges(&[finding(Rule::StackedWalls, 0, (2, 3), "stacked")]);
    let quads = quads(&[], &badges, BADGE_TILES);
    assert!(!quads.is_empty() && quads.len() % FLOATS_PER_QUAD == 0);
    // Every corner is in the top right quarter of the cell.
    for quad in quads.chunks_exact(FLOATS_PER_QUAD) {
        for corner in quad[..8].chunks_exact(2) {
            assert!((2.45..=3.0).contains(&corner[0]) && (3.45..=4.0).contains(&corner[1]), "{corner:?}");
        }
    }
    // Over it, the badge. Over the rest of the cell, none.
    assert!(badge_at(&[], &badges, (2.71, 3.71), BADGE_TILES).is_some());
    assert!(badge_at(&[], &badges, (2.2, 3.2), BADGE_TILES).is_none());
    // Far out it is bigger than the cell, and over its middle.
    assert!(badge_at(&[], &badges, (2.5, 3.5), 2.0).is_some());
}
