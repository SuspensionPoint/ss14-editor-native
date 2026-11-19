//! How walls are placed: the block under the entity palette that lays a tile under new walls,
//! and the Room tool's panel with its floor, edge tile and wall. Blitz has no drop-down lists,
//! so a tile or a wall is chosen from a short list that opens under its row.

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
    thumbs.tile(textures, id, sprite.as_deref(), 32)
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
        span { class: "inline-block bg-surface border border-subtle rounded-sm flex-shrink-0", style: "width: 24px; height: 24px;",
            if let Some(picture) = picture {
                img { src: "{picture}", class: "pixelated", style: "width: 22px; height: 22px;" }
            }
        }
        span { class: "truncate flex-1 text-[11px] text-primary", "{name}" }
    }
}

/// The small button at the end of a row that opens or closes its list.
#[component]
fn ChangeButton(open: Signal<bool>) -> Element {
    let mut open = open;
    rsx! {
        button { class: "px-2 py-0.5 text-[10px] bg-surface border border-subtle rounded-sm text-primary cursor-pointer hover:border-accent",
            onclick: move |_| open.toggle(),
            if open() { "Close" } else { "Change" }
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
        div { class: "space-y-1",
            TextField { value: search(), placeholder: "Search tiles...", oninput: move |text: String| search.set(text) }
            div { class: "flex flex-wrap gap-1 overflow-y-auto", style: "max-height: 140px;",
                if tiles.is_empty() {
                    div { class: "text-muted text-[10px] p-1 italic", "No results" }
                }
                for (id, picture) in tiles {
                    button { key: "{id}", title: "{id}",
                        class: "w-8 h-8 rounded-sm border cursor-pointer p-0 overflow-hidden bg-panel shrink-0 border-subtle hover:border-accent",
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

/// The row of the tile laid under new walls and on a room's edge, with its list.
#[component]
fn FloorTileRow(label: String) -> Element {
    let _ = watch();
    let open = use_signal(|| false);
    let (id, picture) = with(|editor| {
        let id = editor.session.floor_tile().to_owned();
        let picture = tile_picture(editor, &id);
        (id, picture)
    });
    rsx! {
        div { class: "flex items-center gap-2",
            label { class: "text-[11px] text-muted w-12", "{label}" }
            Pictured { picture, name: id }
            ChangeButton { open }
        }
        if open() {
            TilePicker {
                onpick: move |id: String| {
                    with(|editor| editor.session.set_floor_tile(&id));
                    let mut open = open;
                    open.set(false);
                    bump();
                }
            }
        }
    }
}

/// Under the entity palette: whether a brush lays a tile under the walls, windows, grilles and
/// doors it puts on cells with no tile, and which tile.
#[component]
pub(crate) fn WallPlacement() -> Element {
    let _ = watch();
    let on = with(|editor| editor.session.floor_under_walls());
    rsx! {
        div { class: "border-t border-subtle px-2 py-2 space-y-1.5",
            div { class: "text-[10px] text-muted uppercase tracking-wider", "Placement" }
            label { class: "flex items-center gap-1 text-[11px] text-primary cursor-pointer",
                title: "Walls, windows, grilles and doors placed where there's no tile get this tile under them",
                input { r#type: "checkbox", class: "check", checked: on,
                    oninput: move |event| {
                        let on = event.checked();
                        with(|editor| editor.session.set_floor_under_walls(on));
                        bump();
                    }
                }
                "Lay a tile under walls"
            }
            FloorTileRow { label: "Tile" }
        }
    }
}

/// The Room tool's panel: the floor it lays inside (the tile in hand), the tile on its edge,
/// the wall it builds with, with a list to choose another, and its Intersection switch.
#[component]
pub(crate) fn RoomPanel() -> Element {
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
    rsx! {
        div { class: "px-2 py-2 space-y-1.5",
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Floor" }
                if let Some((id, picture)) = floor {
                    Pictured { picture, name: id }
                } else {
                    span { class: "text-[11px] text-warning", "Choose a tile in the Tiles tab" }
                }
            }
            FloorTileRow { label: "Edge" }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-muted w-12", "Wall" }
                if let Some((name, picture)) = wall {
                    Pictured { picture, name }
                } else {
                    span { class: "text-[11px] text-warning flex-1", "No wall chosen" }
                }
                ChangeButton { open: choosing }
            }
            if choosing() {
                div { class: "space-y-1",
                    TextField { value: search(), placeholder: "Search walls...", oninput: move |text: String| search.set(text) }
                    div { class: "overflow-y-auto", style: "max-height: 160px;",
                        if walls.is_empty() {
                            div { class: "text-muted text-[10px] p-1 italic", "No results" }
                        }
                        for (index, name, picture) in walls {
                            button { key: "{index}",
                                class: "flex items-center gap-1.5 px-1 py-0.5 w-full text-left border-none rounded-sm bg-transparent cursor-pointer hover:bg-hover",
                                style: "height: 28px;",
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
            div {
                label { class: "flex items-center gap-1 text-[11px] text-primary cursor-pointer",
                    input { r#type: "checkbox", class: "check", checked: joining,
                        oninput: move |event| {
                            let on = event.checked();
                            with(|editor| editor.session.set_room_intersection(on));
                            bump();
                        }
                    }
                    "Intersection"
                }
                div { class: "text-[10px] text-muted pl-4", "Joins rooms it overlaps into one. Good for hallways and extensions." }
            }
            div { class: "text-[10px] text-muted", "Drag out a room. Doors are up to you. Right-click or Escape cancels." }
        }
    }
}
