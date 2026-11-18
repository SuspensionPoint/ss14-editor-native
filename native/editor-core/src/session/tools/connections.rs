//! The connections view: every device link and device list entry drawn as a line from the
//! middle of one end to the middle of the other, as the web editor's Connections switch does.
//!
//! Lines are rectangles of the overlay drawn over the map (`overlay_quads`), which the
//! renderer puts under the selection border. Cyan is a device list and orange is links. With
//! nothing selected every line is drawn at medium strength. With a selection, the lines that
//! touch a selected entity are drawn bright and thick, and the rest faint. Below four pixels
//! to a tile nothing is drawn, as in the web editor. Line widths are in pixels, so they are
//! worked out again at each zoom.

use super::*;

/// The web editor's colours for a device list and for links, dim and lit.
const LIST: [f32; 3] = [0.0, 0.8, 0.8];
const LINKS: [f32; 3] = [1.0, 0.533, 0.0];
const LIST_LIT: [f32; 3] = [0.0, 1.0, 1.0];
const LINKS_LIT: [f32; 3] = [1.0, 0.667, 0.0];

/// Below this many pixels to a tile no line is drawn.
const FEWEST_PIXELS: f32 = 4.0;

/// A rectangle along a line, `width` across, for the overlay.
fn line_quad(from: (f32, f32), to: (f32, f32), width: f32, colour: [f32; 4], over: &mut Vec<f32>) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 0.0 {
        return;
    }
    // Half the width, square to the line, to its left.
    let (nx, ny) = (-dy / length * width / 2.0, dx / length * width / 2.0);
    for corner in [(from.0 - nx, from.1 - ny), (to.0 - nx, to.1 - ny), (to.0 + nx, to.1 + ny), (from.0 + nx, from.1 + ny)] {
        over.extend_from_slice(&[corner.0, corner.1]);
    }
    over.extend_from_slice(&colour);
    over.push(WHITE_TEXTURE as f32);
    over.extend_from_slice(&WHITE_SOURCE);
}

impl Session {
    /// The connections view's lines, for `overlay_quads`, while it is switched on and the
    /// entities are shown, as in the web editor.
    pub(super) fn connection_overlay(&self, over: &mut Vec<f32>) {
        if !self.show_connections || !self.layers.entities {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(mirror) = doc.links.as_ref() else { return };
        let pixels = 32.0 * self.camera.zoom;
        if pixels < FEWEST_PIXELS {
            return;
        }
        let selected: HashSet<i32> = self.selection.iter().copied().collect();
        // Each line: its two ends, whether it is of a list, and whether it touches the selection.
        // A pair linked by several ports has one line.
        let mut lines: Vec<(i32, i32, bool)> = mirror.all_links().iter().map(|link| (link.source, link.sink, false)).collect();
        lines.sort_unstable();
        lines.dedup();
        for (list, devices) in mirror.all_lists() {
            lines.extend(devices.iter().map(|&device| (list, device, true)));
        }
        let mut lit = Vec::new();
        for (from, to, list) in lines {
            let (Some(start), Some(end)) = (doc.world_of(from), doc.world_of(to)) else { continue };
            if selected.contains(&from) || selected.contains(&to) {
                lit.push((start, end, list));
                continue;
            }
            // The web editor's 1.5 pixels at 0.4, or with a selection 1 pixel at 0.15.
            let (width, alpha) = if selected.is_empty() { (1.5, 0.4) } else { (1.0, 0.15) };
            let [r, g, b] = if list { LIST } else { LINKS };
            line_quad(start, end, width / pixels, [r, g, b, alpha], over);
        }
        // Lit lines last, so that they lie over the faint ones.
        for (start, end, list) in lit {
            let [r, g, b] = if list { LIST_LIT } else { LINKS_LIT };
            line_quad(start, end, 3.0 / pixels, [r, g, b, 1.0], over);
        }
    }
}
