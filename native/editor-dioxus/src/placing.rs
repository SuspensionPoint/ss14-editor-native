//! How walls are placed, as the options bar shows it: laying a tile under new walls for Paint
//! on entities, and the Room tool's floor, edge tile, wall and Intersection switch. Blitz has
//! no drop-down lists, so a tile or a wall is chosen from a short list in a box that opens
//! under its button (`Popover`).

use dioxus_native::prelude::*;
use editor_core::palette::tile_order;
use editor_core::session::shown_name;

use crate::dialogs::TextField;
use crate::{bump, watch, with, Editor};

/// How many tiles or walls a short list shows at most.
const LISTED: usize = 40;

/// A tile's picture, small, for a row.
fn tile_picture(editor: &mut Editor, id: &str) -> Option<String> {
    let Editor { session, textures, thumbs, .. } = editor;
    let sprite = session.tiles.iter().find(|tile| tile.id == id).and_then(|tile| tile.sprite.clone());
    thumbs.tile(textures, sprite.as_deref(), 32)
}

/// A wall's picture and name, for a row.
fn wall_row(editor: &mut Editor, index: usize) -> (String, Option<String>) {
    editor.session.want_previews(&[index]);
    let Editor { session, textures, thumbs, .. } = editor;
    let name = session.prototypes().get(index).map(shown_name).unwrap_or_default();
    (name, thumbs.entity(session, textures, index, 32))
}

/// A small picture with its name beside it, or a blank square where there is no picture.
#[component]
fn Pictured(picture: Option<String>, name: String) -> Element {
    rsx! {
        span { class: "chip-picture",
            if let Some(picture) = picture {
                img { src: "{picture}", class: "pixelated", style: "width: 20px; height: 20px;" }
            }
        }
        span { class: "chip-name", "{name}" }
    }
}

/// A button showing a choice, with its picture and name, that opens a box under it to change
/// the choice in. The box closes when its button is pressed again, or when the box calls
/// for it through the signal it is given.
#[component]
fn Popover(picture: Option<String>, name: String, title: String, open: Signal<bool>, children: Element) -> Element {
    let mut open = open;
    rsx! {
        div { class: "popover-anchor",
            button { class: if open() { "chip on" } else { "chip" }, title: "{title}",
                onclick: move |_| open.toggle(),
                Pictured { picture, name }
                span { class: "chip-caret", crate::dialogs::Chevron { open: open() } }
            }
            if open() {
                div { class: "popover",
                    onmousedown: move |event| event.stop_propagation(),
                    {children}
                }
            }
        }
    }
}

/// A short list of the fork's tiles to choose one from, with a search. The common tiles come
/// first, as in the tile palette.
#[component]
fn TilePicker(onpick: EventHandler<String>) -> Element {
    let mut search = use_signal(String::new);
    let tiles: Vec<(String, Option<String>)> = with(|editor| {
        if editor.tile_order.0 != editor.session.tiles.len() {
            editor.tile_order = (editor.session.tiles.len(), tile_order(&editor.session.tiles));
        }
        let wanted = search().to_lowercase();
        let ids: Vec<String> = editor.tile_order.1.iter().map(|&index| editor.session.tiles[index].id.clone()).filter(|id| wanted.is_empty() || id.to_lowercase().contains(&wanted)).take(LISTED).collect();
        ids.into_iter().map(|id| {
            let picture = tile_picture(editor, &id);
            (id, picture)
        }).collect()
    });
    rsx! {
        div { class: "popover-stack",
            TextField { value: search(), placeholder: "Search tiles...", oninput: move |text: String| search.set(text) }
            div { class: "flex flex-wrap gap-1 overflow-y-auto", style: "max-height: 168px;",
                if tiles.is_empty() {
                    div { class: "hint-text", "No results" }
                }
                for (id, picture) in tiles {
                    button { key: "{id}", title: "{id}",
                        class: "tile-choice",
                        onclick: {
                            let id = id.clone();
                            move |_| onpick.call(id.clone())
                        },
                        if let Some(picture) = picture {
                            img { src: "{picture}", class: "pixelated", style: "width: 100%; height: 100%;" }
                        } else {
                            span { class: "text-muted", style: "font-size: 8px;", "{id}" }
                        }
                    }
                }
            }
        }
    }
}

/// The tile laid under new walls and on a room's edge, as a button that opens a list of tiles.
#[component]
fn FloorTileChooser(title: String) -> Element {
    let _ = watch();
    let mut open = use_signal(|| false);
    let (id, picture) = with(|editor| {
        let id = editor.session.floor_tile().to_owned();
        let picture = tile_picture(editor, &id);
        (id, picture)
    });
    rsx! {
        Popover { picture, name: id, title, open,
            TilePicker {
                onpick: move |id: String| {
                    with(|editor| editor.session.set_floor_tile(&id));
                    open.set(false);
                    bump();
                }
            }
        }
    }
}

/// For Paint on entities: whether a brush lays a tile under the walls, windows, grilles and
/// doors it puts on cells with no tile, and which tile.
#[component]
pub(crate) fn WallPlacement() -> Element {
    let _ = watch();
    let on = with(|editor| editor.session.floor_under_walls());
    rsx! {
        label { class: "option-check",
            title: "Walls, windows, grilles and doors placed where there's no tile get this tile under them",
            input { r#type: "checkbox", class: "check", checked: on,
                oninput: move |event| {
                    let on = event.checked();
                    with(|editor| editor.session.set_floor_under_walls(on));
                    bump();
                }
            }
            "Tile under walls"
        }
        FloorTileChooser { title: "The tile laid under walls" }
    }
}

/// The Room tool's options: the floor it lays inside (the tile in hand), the tile on its
/// edge, the wall it builds with, with a list to choose another, and its Intersection switch.
#[component]
pub(crate) fn RoomOptions() -> Element {
    let _ = watch();
    let mut choosing = use_signal(|| false);
    let mut search = use_signal(String::new);
    let (floor, wall, walls, joining) = with(|editor| {
        let floor = editor.session.chosen_tile().map(str::to_owned).map(|id| {
            let picture = tile_picture(editor, &id);
            (id, picture)
        });
        let wall = editor.session.room_wall().map(|index| wall_row(editor, index));
        let walls: Vec<(usize, String, Option<String>)> = if choosing() {
            let lowered = search().to_lowercase();
            let words: Vec<&str> = lowered.split_whitespace().collect();
            let found = editor.session.room_walls(&words);
            found.into_iter().take(LISTED).map(|index| {
                let (name, picture) = wall_row(editor, index);
                (index, name, picture)
            }).collect()
        } else {
            Vec::new()
        };
        (floor, wall, walls, editor.session.room_intersection())
    });
    let (wall_name, wall_picture) = wall.unwrap_or(("No wall chosen".to_owned(), None));
    rsx! {
        span { class: "options-label", "Floor" }
        if let Some((id, picture)) = floor {
            span { class: "chip static", title: "The tile in hand goes inside", Pictured { picture, name: id } }
        } else {
            span { class: "options-warning", "Choose a tile in the Tiles tab" }
        }
        span { class: "options-label", "Edge" }
        FloorTileChooser { title: "The tile under the walls on the edge" }
        span { class: "options-label", "Wall" }
        Popover { picture: wall_picture, name: wall_name, title: "The wall on the edge", open: choosing,
            div { class: "popover-stack",
                TextField { value: search(), placeholder: "Search walls...", oninput: move |text: String| search.set(text) }
                div { class: "overflow-y-auto", style: "max-height: 220px;",
                    if walls.is_empty() {
                        div { class: "hint-text", "No results" }
                    }
                    for (index, name, picture) in walls {
                        button { key: "{index}", class: "list-choice",
                            onclick: move |_| {
                                with(|editor| editor.session.set_room_wall(index));
                                choosing.set(false);
                                bump();
                            },
                            Pictured { picture, name }
                        }
                    }
                }
            }
        }
        div { class: "options-rule" }
        label { class: "option-check", title: "Joins rooms it overlaps into one. Good for hallways and extensions",
            input { r#type: "checkbox", class: "check", checked: joining,
                oninput: move |event| {
                    let on = event.checked();
                    with(|editor| editor.session.set_room_intersection(on));
                    bump();
                }
            }
            "Join overlapping rooms"
        }
    }
}
