//! The Problems overlay's drawing. A small warning badge sits on each cell where Validate Map's
//! cheap rules (`crate::validate::quick`) find something: an orange disc with a dark ring and a
//! "!", in the orange of the air overlay's leaks. One badge stands for every finding on its
//! cell. The session runs the rules and keeps the badges (`session/problems.rs`).
//!
//! A badge is half a tile wide close up, in the top right corner of its cell, so the thing it
//! marks still shows. Further out it keeps at least [`BADGE_PIXELS`] on screen and moves to
//! the middle of its cell, so a station's problems can be seen whole.

use crate::air::{Icon, LEAK};
use crate::air_draw::{disc, icon_shapes, quad, Point};
use crate::model::Grid;
use crate::validate::{Finding, Rule};

/// How wide a badge is on screen at the least, in the interface's pixels.
pub const BADGE_PIXELS: f32 = 16.0;
/// How wide a badge is close up, in tiles.
pub const BADGE_TILES: f32 = 0.5;
/// The dark ring round a badge and the ink of its "!".
const INK: [u8; 3] = [10, 10, 22];

/// Everything found on one cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Badge {
    /// The grid, or 0 for the map itself, and the cell.
    pub grid: i32,
    pub cell: (i32, i32),
    /// The findings' rules and messages, in the order they were found.
    pub found: Vec<(Rule, String)>,
}

impl Badge {
    /// The lines of the box the mouse shows over the badge: a title, then each finding.
    pub fn lines(&self) -> Vec<String> {
        let mut rules: Vec<Rule> = self.found.iter().map(|(rule, _)| *rule).collect();
        rules.dedup();
        let title = match rules.as_slice() {
            [one] => one.label().to_owned(),
            _ => format!("{} problems here", self.found.len()),
        };
        std::iter::once(title).chain(self.found.iter().map(|(_, message)| message.clone())).collect()
    }
}

/// The findings put together by cell, each cell once, in the order its first finding comes.
pub fn badges(findings: &[Finding]) -> Vec<Badge> {
    let mut badges: Vec<Badge> = Vec::new();
    let mut at: std::collections::HashMap<(i32, (i32, i32)), usize> = std::collections::HashMap::new();
    for finding in findings {
        let index = *at.entry((finding.grid, finding.cell)).or_insert_with(|| {
            badges.push(Badge { grid: finding.grid, cell: finding.cell, found: Vec::new() });
            badges.len() - 1
        });
        badges[index].found.push((finding.rule, finding.message.clone()));
    }
    badges
}

/// How wide a badge is in tiles, for a tile this many of the interface's pixels wide on
/// screen: [`BADGE_TILES`], or more to keep [`BADGE_PIXELS`]. The size goes in steps of a
/// quarter of a doubling of the zoom, so a zoom builds the badges again only now and then.
pub fn badge_size(tile_pixels: f32) -> f32 {
    if !(tile_pixels > 0.0) || tile_pixels.is_infinite() {
        return BADGE_TILES;
    }
    let step = (tile_pixels.log2() * 4.0).floor() / 4.0;
    (BADGE_PIXELS / step.exp2()).max(BADGE_TILES)
}

/// The middle of a badge of this size in its grid's own coordinates: the top right corner of
/// its cell while it is smaller than the cell, and the middle once it is not.
fn middle_local(cell: (i32, i32), size: f32) -> Point {
    let off = ((1.0 - size) / 2.0 - 0.04).max(0.0);
    (cell.0 as f32 + 0.5 + off, cell.1 as f32 + 0.5 + off)
}

/// The grid a badge is on, turned into the map's coordinates, or the map itself.
fn to_world<'a>(grids: &'a [Grid], grid: i32) -> impl Fn(Point) -> Point + 'a {
    let found = grids.iter().find(|known| known.uid == grid);
    move |point: Point| found.map_or(point, |grid| grid.to_world(point))
}

/// The rectangles of every badge, in world tiles, as the renderer takes them. `size` is
/// [`badge_size`]. A badge on a grid the mirror does not have is drawn as if on the map.
pub fn quads(grids: &[Grid], badges: &[Badge], size: f32) -> Vec<f32> {
    let mut out = Vec::new();
    let radius = size / 2.0;
    let ring = disc((0.5, 0.5), 0.5);
    let fill = disc((0.5, 0.5), 0.41);
    let bang = icon_shapes(Icon::Bang);
    for badge in badges {
        let world = to_world(grids, badge.grid);
        let middle = middle_local(badge.cell, size);
        // From the unit square of a picture, down its y, to the grid.
        let at = |point: Point| world((middle.0 - radius + point.0 * size, middle.1 + radius - point.1 * size));
        for shape in &ring {
            quad(shape.map(at), INK, 0.9, &mut out);
        }
        for shape in &fill {
            quad(shape.map(at), LEAK, 1.0, &mut out);
        }
        for shape in &bang {
            quad(shape.map(at), INK, 1.0, &mut out);
        }
    }
    out
}

/// The badge under a point of the map, for badges of this size. Of two that overlap, the last
/// drawn, which is on top.
pub fn badge_at<'a>(grids: &[Grid], badges: &'a [Badge], world: Point, size: f32) -> Option<&'a Badge> {
    let radius = size / 2.0;
    badges.iter().rev().find(|badge| {
        let local = grids.iter().find(|grid| grid.uid == badge.grid).map_or(world, |grid| grid.to_local(world));
        let middle = middle_local(badge.cell, size);
        let (dx, dy) = (local.0 - middle.0, local.1 - middle.1);
        dx * dx + dy * dy <= radius * radius
    })
}

#[cfg(test)]
#[path = "tests/problems.rs"]
mod tests;
