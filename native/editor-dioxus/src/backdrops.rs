//! The backdrop gallery behind Map > Backdrop.... Each of the fork's backdrops gets a tile with
//! its picture and name, and the map's current one is marked. Hovering a tile previews it behind
//! the map, clicking gives it to the map, and Escape or the close button changes nothing. The
//! gallery's state and questions live in the session (editor-core's `session/backdrops.rs`).

use dioxus_native::prelude::*;

use crate::{bump, watch, with, Editor};

/// One tile as it is drawn.
#[derive(Clone, PartialEq)]
struct Tile {
    index: usize,
    label: String,
    /// The picture's data address, None while it is painted, or why there is none.
    picture: Option<Result<String, String>>,
    current: bool,
    hovered: bool,
}

#[component]
pub(crate) fn BackdropDialog() -> Element {
    let _ = watch();
    let Some((tiles, failure)) = with(|editor| editor.session.backdrops_open().then(|| tiles(editor))) else { return rsx! {} };
    let close = move |_| {
        with(|editor| editor.session.cancel_backdrops());
        bump();
    };
    rsx! {
        // The page is darkened only a little: the map behind shows the backdrop under the mouse.
        // The mouse anywhere but over a tile is the mouse having left the tiles.
        div { class: "fixed inset-0 flex items-center justify-center", style: "background-color: rgba(0,0,0,0.25); z-index: 9998;",
            onmousedown: move |event| event.stop_propagation(),
            onmousemove: move |_| leave(),
            onclick: close,
            div { class: "bg-elevated border border-subtle rounded-lg overflow-y-auto text-primary text-[13px]",
                // Four tiles across.
                style: "width: 762px; max-height: 80vh; padding: 20px;",
                onclick: move |event| event.stop_propagation(),
                div { class: "flex items-center justify-between mb-2",
                    h2 { class: "text-base font-semibold text-accent m-0", "Backdrop" }
                    button { class: "bg-transparent border-none text-muted hover:text-primary cursor-pointer text-lg leading-none px-1",
                        onclick: close,
                        "\u{d7}"
                    }
                }
                p { class: "text-[11px] text-muted mt-0 mb-3",
                    "What the game draws behind the map. Hover over one to preview it, and click to use it."
                }
                if let Some(failure) = failure {
                    p { class: "text-[12px] text-warning", "The engine couldn't list the backdrops: {failure}" }
                } else if tiles.is_empty() {
                    p { class: "text-[12px] text-muted", "Loading backdrops..." }
                }
                div { class: "backdrop-grid",
                    for tile in tiles {
                        BackdropTile { key: "{tile.index}", tile }
                    }
                }
            }
        }
    }
}

#[component]
fn BackdropTile(tile: Tile) -> Element {
    let Tile { index, label, picture, current, hovered } = tile;
    let class = match (current, hovered) {
        (true, true) => "backdrop-tile current hovered",
        (true, false) => "backdrop-tile current",
        (false, true) => "backdrop-tile hovered",
        (false, false) => "backdrop-tile",
    };
    let title = if current { format!("{label} (current backdrop)") } else { label.clone() };
    rsx! {
        div { class: "{class}", title: "{title}",
            // Each move over the tile, as Blitz has no enter and leave to count on here. Only
            // a change draws again.
            onmousemove: move |event| {
                event.stop_propagation();
                if with(|editor| editor.session.hover_backdrop(index)) {
                    bump();
                }
            },
            onclick: move |_| {
                with(|editor| editor.session.choose_backdrop(index));
                bump();
            },
            match picture {
                Some(Ok(picture)) => rsx! { img { class: "backdrop-picture", src: "{picture}" } },
                Some(Err(_)) => rsx! { div { class: "backdrop-placeholder", "No preview" } },
                None => rsx! { div { class: "backdrop-placeholder", "Drawing..." } },
            }
            div { class: "backdrop-name", "{label}" }
        }
    }
}

/// The mouse is over no tile: the map's own backdrop again.
fn leave() {
    if with(|editor| editor.session.leave_backdrops()) {
        bump();
    }
}

/// The tiles in the gallery's order, and why there are none if the engine would not list them.
fn tiles(editor: &mut Editor) -> (Vec<Tile>, Option<String>) {
    let Editor { session, thumbs, .. } = editor;
    let tiles = session
        .backdrop_entries()
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let picture = session.backdrop_thumb(&entry.shows).map(|thumb| thumb.map(|thumb| thumbs.backdrop(thumb, &entry.shows)).map_err(str::to_owned));
            Tile { index, label: entry.label, picture, current: entry.current, hovered: entry.hovered }
        })
        .collect();
    (tiles, session.backdrop_list_failure().map(str::to_owned))
}
