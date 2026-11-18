//! The side panel. The tile, entity and decal palettes, the prefabs and the grids sit under a row
//! of tabs, with the layers below them, in the web editor's markup (its PalettePanel.tsx,
//! TilePalette.tsx, EntityPalette.tsx and DecalPalette.tsx).

use std::rc::Rc;

use dioxus_native::prelude::*;
use editor_core::palette::{tile_order, TreeRow};
use editor_core::session::{decal_colour_bytes, decal_colour_text, shown_name, Favourite, FavouriteKind, FavouriteState, InHand};

use crate::dialogs::{Chevron, Star, TextField, Triangle};
use crate::thumbs::BIG;
use crate::{bump, drawn, watch, with, Editor};

#[derive(PartialEq, Clone, Copy, Debug)]
pub(crate) enum PaletteTab {
    Favourites,
    Tiles,
    Entities,
    Decals,
    Prefabs,
    Grids,
}

/// The tab the side panel opens on: Tiles, or the one `SS14_EDITOR_TAB` names (favourites,
/// tiles, entities, decals, prefabs or grids), a developer's switch for looking at a tab
/// without a click.
pub(crate) fn first_tab() -> PaletteTab {
    match std::env::var("SS14_EDITOR_TAB").unwrap_or_default().to_lowercase().as_str() {
        "favourites" => PaletteTab::Favourites,
        "entities" => PaletteTab::Entities,
        "decals" => PaletteTab::Decals,
        "prefabs" => PaletteTab::Prefabs,
        "grids" => PaletteTab::Grids,
        _ => PaletteTab::Tiles,
    }
}

/// The star on a palette's row that makes it a favourite or takes it off. It shows while the
/// row is under the mouse, and always on a favourite (`.fav-star` in local.css).
#[component]
pub(crate) fn FavouriteStar(favourite: Favourite, on: bool, #[props(default = 14)] size: u32, #[props(default)] corner: bool) -> Element {
    let class = match (on, corner) {
        (true, true) => "fav-star on fav-corner",
        (false, true) => "fav-star fav-corner",
        (true, false) => "fav-star on",
        (false, false) => "fav-star",
    };
    rsx! {
        span { class,
            onclick: move |event| {
                event.stop_propagation();
                let favourite = favourite.clone();
                with(|editor| editor.session.toggle_favourite(favourite));
                bump();
            },
            Star { filled: on, size }
        }
    }
}

/// The heights of the entity palette's rows, which it lays out itself to draw only those in view.
const CATEGORY_ROW: f64 = 22.0;
const ENTITY_ROW: f64 = 36.0;
/// How tall the list is taken to be before the first frame measures it.
const FIRST_VIEW: f64 = 1200.0;

thread_local! {
    /// The entity list's scroll area while the palette is shown, and the view its rows are built
    /// for, which `follow_entity_scroll` keeps to the area's scroll.
    static ENTITY_LIST: std::cell::RefCell<Option<(dioxus_native::NodeHandle, Signal<(f64, f64)>)>> = const { std::cell::RefCell::new(None) };
}

/// Called before and after each frame is drawn. If the entity list was scrolled since its rows
/// were built, it says so to the palette, which builds the rows now in view, and returns true.
///
/// Blitz moves a scroll area without a `scroll` event for some scrolls (a program's, a smooth
/// one, a fling, and the clamping of an area whose content got shorter), so the palette reads
/// the scroll from the area itself rather than wait for an event. It is one look-up a frame.
pub(crate) fn follow_entity_scroll() -> bool {
    ENTITY_LIST.with(|list| {
        let list = list.borrow();
        let Some((handle, view)) = list.as_ref() else {
            return false;
        };
        let Some(seen) = handle.try_doc().and_then(|doc| {
            let node = doc.get_node(handle.node_id())?;
            Some((node.scroll_offset().y, (node.final_layout().size.height as f64).max(100.0)))
        }) else {
            return false;
        };
        if view.try_peek().map_or(true, |built| *built == seen) {
            return false;
        }
        let mut view = *view;
        match view.try_write() {
            Ok(mut built) => *built = seen,
            Err(_) => return false,
        }
        true
    })
}

/// A choice made away from the palettes, as by the pick tool: the palette of its kind is shown,
/// its search is cleared if it hides the choice, and the list is scrolled to it.
pub(crate) fn reveal_choice(editor: &mut Editor) {
    match editor.session.in_hand() {
        Some(InHand::Tile(id)) => {
            editor.tab = PaletteTab::Tiles;
            if !id.to_lowercase().contains(&editor.tile_search.to_lowercase()) {
                editor.tile_search.clear();
            }
        }
        Some(InHand::Entity { index, .. }) => {
            editor.tab = PaletteTab::Entities;
            let search = editor.entity_search.to_lowercase();
            let words: Vec<&str> = search.split_whitespace().collect();
            if !words.is_empty() && !editor.session.entity_search(&words).contains(&index) {
                editor.entity_search.clear();
            }
            // Its category is opened in the tree.
            if let Some(category) = editor.session.entity_tree().and_then(|tree| tree.category_holding(index).map(|at| tree.categories[at].path.clone())) {
                editor.open_categories.insert(category);
            }
        }
        Some(InHand::Decal(id)) => {
            editor.tab = PaletteTab::Decals;
            let search = editor.decal_search.to_lowercase();
            let words: Vec<&str> = search.split_whitespace().collect();
            if !words.is_empty() && !editor.session.decals_matching(&words).iter().any(|&index| editor.session.decal_prototypes[index].id == id) {
                editor.decal_search.clear();
            }
            if let Some(group) = editor.session.decal_groups().into_iter().find(|group| group.decals.iter().any(|&index| editor.session.decal_prototypes[index].id == id)) {
                editor.open_decal_groups.insert(group.tag);
            }
        }
        None => return,
    }
    editor.scroll_to_chosen = true;
}

/// A panel with a header that opens and shuts it, as the web editor's CollapsiblePanel.
#[component]
pub(crate) fn Collapsible(title: String, children: Element) -> Element {
    drawn();
    let mut open = use_signal(|| true);
    rsx! {
        div { class: "flex flex-col border-b border-subtle",
            // The triangle on the left, as on every row that opens: the one sign of it.
            button { class: "flex items-center gap-2 w-full px-3 py-1.5 bg-surface hover:bg-hover text-primary text-xs font-medium cursor-pointer select-none border-none outline-none",
                onclick: move |_| open.toggle(),
                span { class: "text-muted flex items-center justify-center", style: "width: 8px;", Chevron { open: open() } }
                span { "{title}" }
            }
            if open() {
                div { class: "overflow-y-auto", style: "max-height: 45vh;", {children} }
            }
        }
    }
}

#[component]
pub(crate) fn SidePanel() -> Element {
    let _ = watch();
    let (tab, selected, laying, tile_search, chosen_tile, tiles, starred_tiles) = with(|editor| {
        let session = &editor.session;
        let selected = session.doc.is_some() && !(session.selection.is_empty() && session.selected_decals.is_empty());
        let starred: Vec<String> = session.favourites().into_iter().filter(|row| row.favourite.kind == FavouriteKind::Tile).map(|row| row.favourite.id).collect();
        (editor.tab, selected, session.tool().is_infrastructure_tool(), editor.tile_search.clone(), session.chosen_tile().map(str::to_owned), session.tiles.len(), starred)
    });
    let tabs = [(PaletteTab::Tiles, "Tiles"), (PaletteTab::Entities, "Entities"), (PaletteTab::Decals, "Decals"), (PaletteTab::Prefabs, "Prefabs"), (PaletteTab::Grids, "Grids")];
    rsx! {
        div { class: "flex flex-col min-w-[280px] max-w-[400px] w-[20vw] bg-panel border-l border-subtle overflow-hidden",
            // The web editor's order: what is selected while anything is, then the cable and
            // pipe choices while either tool is in hand, then the palette, then the layers.
            if selected {
                Collapsible { title: "Selection", crate::selection::SelectionPanel {} }
            }
            if laying {
                Collapsible { title: "Infrastructure", crate::selection::InfrastructurePanel {} }
            }
            div { class: "flex-1 flex flex-col overflow-hidden",
                div { class: "flex border-b border-subtle bg-surface",
                    // The favourites first, a star alone for its name, as six names do not fit.
                    button {
                        class: if tab == PaletteTab::Favourites { "flex items-center justify-center py-1.5 cursor-pointer border-none border-b-2 bg-active border-accent" } else { "flex items-center justify-center py-1.5 cursor-pointer border-none border-b-2 bg-surface border-transparent" },
                        style: "flex: 0 0 34px;",
                        onclick: move |_| {
                            with(|editor| editor.tab = PaletteTab::Favourites);
                            bump();
                        },
                        Star { filled: tab == PaletteTab::Favourites, size: 13 }
                    }
                    for (each, name) in tabs {
                        button {
                            class: if each == tab { "flex-1 py-1.5 text-[11px] cursor-pointer border-none border-b-2 bg-active text-white font-bold border-accent" } else { "flex-1 py-1.5 text-[11px] cursor-pointer border-none border-b-2 bg-surface text-muted font-normal border-transparent hover:text-primary" },
                            onclick: move |_| {
                                with(|editor| editor.tab = each);
                                bump();
                            },
                            "{name}"
                        }
                    }
                }
                match tab {
                    PaletteTab::Favourites => rsx! { FavouritesPalette {} },
                    PaletteTab::Tiles => rsx! { TilePalette { search: tile_search, chosen: chosen_tile, count: tiles, starred: starred_tiles } },
                    PaletteTab::Entities => rsx! { EntityPalette {} },
                    PaletteTab::Decals => rsx! { DecalPalette {} },
                    PaletteTab::Prefabs => rsx! { crate::prefabs::PrefabPalette {} },
                    PaletteTab::Grids => rsx! { crate::prefabs::GridPalette {} },
                }
            }
            Collapsible { title: "Layers", crate::selection::LayersPanel {} }
        }
    }
}

/// The line over a palette that names what it has chosen, as the web editor has it.
#[component]
fn ChosenLine(chosen: Option<String>) -> Element {
    rsx! {
        if let Some(id) = chosen {
            div { class: "px-2 pb-2 border-b border-subtle text-[11px] text-primary", "Selected: ", strong { "{id}" } }
        }
    }
}

/// What the palette shows of a row under the mouse: its picture larger and its name, left of
/// the pointer, as the web editor's preview beside its palette.
#[component]
fn HoverCard(x: f64, y: f64, picture: Option<String>, name: String, more: Vec<String>) -> Element {
    let size = BIG;
    rsx! {
        div { class: "fixed bg-elevated border border-subtle rounded shadow-lg p-2 pointer-events-none",
            style: "left: {(x - size as f64 - 40.0).max(4.0)}px; top: {(y - size as f64 / 2.0 - 20.0).max(4.0)}px; z-index: 200; max-width: 260px;",
            if let Some(picture) = picture {
                img { src: "{picture}", class: "pixelated", style: "width: {size}px; height: {size}px;" }
            } else {
                div { class: "bg-surface rounded flex items-center justify-center text-muted text-xs", style: "width: {size}px; height: {size}px;", "No sprite" }
            }
            div { class: "text-primary text-[11px] text-center mt-1", style: "max-width: {size}px;", "{name}" }
            for line in more {
                div { class: "text-muted text-[10px]", style: "max-width: 240px;", "{line}" }
            }
        }
    }
}

/// One row of the favourites, as built.
#[derive(Clone, PartialEq)]
struct FavouriteRowData {
    at: usize,
    key: Option<usize>,
    name: String,
    kind: &'static str,
    picture: Option<String>,
    chosen: bool,
    missing: bool,
    last: bool,
}

/// The favourites: tiles, entities and decals in one list in the mapper's order, each with its
/// number key, its picture, its name and its kind. A click chooses it as its own palette does.
/// The small triangles move it up and down, and the cross takes it off. One the fork does not
/// have is greyed and cannot be chosen.
#[component]
fn FavouritesPalette() -> Element {
    let _ = watch();
    let rows: Vec<FavouriteRowData> = with(|editor| {
        let rows = editor.session.favourites();
        let entities: Vec<usize> = rows.iter().filter_map(|row| match (row.favourite.kind, row.state) {
            (FavouriteKind::Entity, FavouriteState::Here(index)) => Some(index),
            _ => None,
        }).collect();
        editor.session.want_previews(&entities);
        let count = rows.len();
        let Editor { session, textures, thumbs, .. } = editor;
        rows.into_iter()
            .enumerate()
            .map(|(at, row)| {
                let picture = match (row.favourite.kind, row.state) {
                    (FavouriteKind::Tile, FavouriteState::Here(index)) => {
                        let sprite = session.tiles[index].sprite.clone();
                        thumbs.tile(textures, &row.favourite.id, sprite.as_deref(), 32)
                    }
                    (FavouriteKind::Entity, FavouriteState::Here(index)) => thumbs.entity(session, textures, index, 32),
                    (FavouriteKind::Decal, FavouriteState::Here(index)) => thumbs.decal(session, textures, index),
                    _ => None,
                };
                FavouriteRowData {
                    at,
                    key: row.key,
                    name: row.name,
                    kind: row.favourite.kind.word(),
                    picture,
                    chosen: row.chosen,
                    missing: row.state == FavouriteState::Missing,
                    last: at + 1 == count,
                }
            })
            .collect()
    });
    let count = rows.len();
    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            div { class: "px-2 pt-2 pb-1 text-xs text-muted", "Favourites" }
            if rows.is_empty() {
                div { class: "text-muted text-xs p-3 text-center", style: "line-height: 1.6;",
                    div { class: "flex justify-center mb-2", Star { filled: false, size: 20 } }
                    "No favourites yet. Click the star on any tile, entity or decal, or the one next to what you're holding in the status bar, to add it here."
                    div { class: "mt-2", "Keys 1 to 9 pick the first nine." }
                }
            }
            div { class: "flex-1 overflow-y-auto py-1 px-1",
                for row in rows {
                    FavouriteLine { key: "{row.at}", row }
                }
            }
            div { class: "px-2 py-1 text-[10px] text-muted border-t border-subtle", "{count} favourites" }
        }
    }
}

#[component]
fn FavouriteLine(row: FavouriteRowData) -> Element {
    let FavouriteRowData { at, key, name, kind, picture, chosen, missing, last } = row;
    let class = if chosen {
        "flex items-center gap-1.5 px-1 text-xs w-full rounded-sm mb-px bg-active text-accent border border-accent cursor-pointer"
    } else if missing {
        "flex items-center gap-1.5 px-1 text-xs w-full rounded-sm mb-px text-muted border border-transparent"
    } else {
        "flex items-center gap-1.5 px-1 text-xs w-full rounded-sm mb-px text-primary hover:bg-hover border border-transparent cursor-pointer"
    };
    let key = key.map_or(String::new(), |key| key.to_string());
    let small = "flex items-center justify-center bg-transparent border-none text-muted hover:text-primary cursor-pointer p-0";
    rsx! {
        div { class, style: if missing { "height: 36px; opacity: 0.5;" } else { "height: 36px;" },
            onclick: move |_| {
                with(|editor| editor.session.choose_favourite(at));
                bump();
            },
            span { class: "text-muted text-center font-mono", style: "width: 10px; font-size: 11px; flex-shrink: 0;", "{key}" }
            span { style: "width: 32px; height: 32px; flex-shrink: 0;",
                if let Some(picture) = picture {
                    img { src: "{picture}", class: "pixelated", style: "width: 32px; height: 32px;" }
                }
            }
            span { class: "flex flex-col overflow-hidden flex-1",
                span { class: "truncate", style: "font-size: 13px;", "{name}" }
                span { class: "truncate text-muted", style: "font-size: 10.5px;",
                    if missing { "{kind} (not in this fork)" } else { "{kind}" }
                }
            }
            button { class: small, style: if at == 0 { "width: 16px; height: 20px; opacity: 0.3;" } else { "width: 16px; height: 20px;" },
                onclick: move |event| {
                    event.stop_propagation();
                    with(|editor| editor.session.move_favourite(at, true));
                    bump();
                },
                Triangle { towards: "up", size: 4 }
            }
            button { class: small, style: if last { "width: 16px; height: 20px; opacity: 0.3;" } else { "width: 16px; height: 20px;" },
                onclick: move |event| {
                    event.stop_propagation();
                    with(|editor| editor.session.move_favourite(at, false));
                    bump();
                },
                Triangle { towards: "down", size: 4 }
            }
            button { class: small, style: "width: 16px; height: 20px; font-size: 14px;",
                onclick: move |event| {
                    event.stop_propagation();
                    with(|editor| editor.session.remove_favourite(at));
                    bump();
                },
                "\u{d7}"
            }
        }
    }
}

/// The tile palette. It draws again only when the search, the choice, the tiles or the
/// favourite tiles change, so that its few hundred squares are not built again for every move
/// of the mouse on the map.
#[component]
fn TilePalette(search: String, chosen: Option<String>, count: usize, starred: Vec<String>) -> Element {
    drawn();
    let hovered = use_signal(|| None::<(String, f64, f64)>);
    let tiles: Vec<(String, Option<String>)> = with(|editor| {
        // The common tiles first, worked out again only when the list changes.
        if editor.tile_order.0 != editor.session.tiles.len() {
            editor.tile_order = (editor.session.tiles.len(), tile_order(&editor.session.tiles));
        }
        let wanted = search.to_lowercase();
        let Editor { session, textures, thumbs, tile_order, .. } = editor;
        tile_order
            .1
            .iter()
            .map(|&index| &session.tiles[index])
            .filter(|tile| wanted.is_empty() || tile.id.to_lowercase().contains(&wanted))
            .map(|tile| (tile.id.clone(), thumbs.tile(textures, &tile.id, tile.sprite.as_deref(), 32)))
            .collect()
    });
    let shown = tiles.len();
    let card = hovered().map(|(id, x, y)| {
        let picture = with(|editor| {
            let sprite = editor.session.tiles.iter().find(|tile| tile.id == id).and_then(|tile| tile.sprite.clone());
            let Editor { textures, thumbs, .. } = editor;
            thumbs.tile(textures, &id, sprite.as_deref(), BIG)
        });
        (id, x, y, picture)
    });
    let _ = count;
    rsx! {
        div { class: "flex-1 bg-surface flex flex-col overflow-hidden",
            div { class: "px-2 pt-2 pb-1 text-xs text-muted", "Tiles" }
            ChosenLine { chosen: chosen.clone() }
            div { class: "px-2 py-1",
                TextField { value: search.clone(), placeholder: "Search tiles...", oninput: move |text: String| { with(|editor| editor.tile_search = text); bump(); } }
            }
            div { class: "flex flex-wrap gap-1.5 p-2 overflow-y-auto flex-1 content-start",
                for (id, picture) in tiles {
                    TileButton { key: "{id}", chosen: chosen.as_deref() == Some(id.as_str()), starred: starred.contains(&id), id, picture, hovered }
                }
            }
            div { class: "px-2 py-1 text-[10px] text-muted border-t border-subtle", "{shown} tiles" }
        }
        if let Some((id, x, y, picture)) = card {
            HoverCard { x, y, picture, name: id, more: Vec::new() }
        }
    }
}

#[component]
fn TileButton(id: String, picture: Option<String>, chosen: bool, starred: bool, hovered: Signal<Option<(String, f64, f64)>>) -> Element {
    let (pick, enter) = (id.clone(), id.clone());
    let favourite = Favourite::new(FavouriteKind::Tile, id.clone());
    // Where the name goes when there is no picture: a floor without its "Floor".
    let label = id.strip_prefix("Floor").unwrap_or(&id).to_owned();
    rsx! {
        button {
            class: if chosen { "fav-row w-12 h-12 rounded-sm border cursor-pointer p-0 overflow-hidden relative bg-panel shrink-0 border-accent ring-1 ring-accent" } else { "fav-row w-12 h-12 rounded-sm border cursor-pointer p-0 overflow-hidden relative bg-panel shrink-0 border-subtle hover:border-accent" },
            onclick: move |_| {
                with(|editor| editor.session.choose_tile(&pick));
                bump();
            },
            onmouseenter: move |event| {
                let at = event.client_coordinates();
                hovered.set(Some((enter.clone(), at.x, at.y)));
            },
            onmouseleave: move |_| hovered.set(None),
            if let Some(picture) = picture {
                img { src: "{picture}", class: "pixelated", style: "width: 100%; height: 100%;" }
            } else {
                span { class: "text-muted", style: "font-size: 9px;", "{label}" }
            }
            FavouriteStar { favourite, on: starred, size: 12, corner: true }
        }
    }
}

/// The entity palette: the tree of categories, or a search's list. It lays its rows out itself
/// and builds only those in view, since a fork has many thousands of prototypes.
#[component]
fn EntityPalette() -> Element {
    let _ = watch();
    let view = use_signal(|| (0.0f64, FIRST_VIEW));
    let mut hovered = use_signal(|| None::<(usize, f64, f64)>);
    let mut list = use_signal(|| None::<Rc<MountedData>>);
    use_drop(move || {
        ENTITY_LIST.with(|list| {
            let mut list = list.borrow_mut();
            if list.as_ref().is_some_and(|(_, shown)| *shown == view) {
                *list = None;
            }
        })
    });

    let (search, chosen_id, rows, count_text, chosen, scroll_to) = with(|editor| {
        // A search is worked out again only when it or the list changes. The palette offers
        // only what a mapper can place.
        let tree = editor.session.entity_tree().is_some();
        let key = (editor.entity_search.to_lowercase(), editor.session.prototypes().len(), tree);
        if editor.entity_matches_for.as_ref() != Some(&key) {
            let words: Vec<&str> = key.0.split_whitespace().collect();
            editor.entity_count = editor.session.placeable_matching(&[]).len();
            editor.entity_matches = editor.session.entity_search(&words);
            editor.entity_matches_for = Some(key);
        }
        let searching = !editor.entity_search.trim().is_empty();
        let rows: Vec<TreeRow> = match editor.session.entity_tree() {
            Some(tree) if !searching => tree.rows(&editor.open_categories),
            _ => editor.entity_matches.iter().map(|&index| TreeRow::Entity(index)).collect(),
        };
        let count = if searching { format!("{} of {} entities", editor.entity_matches.len(), editor.entity_count) } else { format!("{} entities", editor.entity_count) };
        let chosen = editor.session.chosen_entity();
        let scroll_to = std::mem::take(&mut editor.scroll_to_chosen);
        (editor.entity_search.clone(), editor.session.chosen_prototype().map(|prototype| prototype.id.clone()), rows, count, chosen, scroll_to)
    });

    // Each row where the rows above it put it, and only those in view are built.
    let (top, height) = view();
    let mut at = 0.0;
    let mut shown = Vec::new();
    let mut chosen_top = None;
    for row in &rows {
        let row_height = if matches!(row, TreeRow::Category(..)) { CATEGORY_ROW } else { ENTITY_ROW };
        if matches!(row, TreeRow::Entity(index) if Some(*index) == chosen) {
            chosen_top = Some(at);
        }
        if at + row_height >= top - ENTITY_ROW && at <= top + height + ENTITY_ROW {
            shown.push((at, *row));
        }
        at += row_height;
    }
    let total = at;
    // A choice made elsewhere scrolls the list to it.
    if let (true, Some(chosen_top), Some(list)) = (scroll_to, chosen_top, list()) {
        let offset = (chosen_top - height / 2.0).max(0.0);
        spawn(async move {
            let _ = list.scroll(dioxus_html::geometry::PixelsVector2D::new(0.0, offset), ScrollBehavior::Instant).await;
        });
    }

    let built: Vec<(f64, EntityRowData)> = with(|editor| {
        let indices: Vec<usize> = shown.iter().filter_map(|(_, row)| if let TreeRow::Entity(index) = row { Some(*index) } else { None }).collect();
        editor.session.want_previews(&indices);
        let Editor { session, textures, thumbs, .. } = editor;
        shown
            .iter()
            .filter_map(|&(at, row)| {
                let data = match row {
                    TreeRow::Category(category, open) => {
                        let tree = session.entity_tree()?;
                        let category = &tree.categories[category];
                        EntityRowData::Category { path: category.path.clone(), short: category.short_name().to_uppercase(), count: category.entries.len(), open }
                    }
                    TreeRow::Entity(index) => {
                        let prototype = session.prototypes().get(index)?;
                        let (name, id) = (shown_name(prototype), prototype.id.clone());
                        let picture = thumbs.entity(session, textures, index, 32);
                        let starred = session.is_favourite(&Favourite::new(FavouriteKind::Entity, id.clone()));
                        EntityRowData::Entity { index, name, id, picture, chosen: chosen == Some(index), starred }
                    }
                };
                Some((at, data))
            })
            .collect()
    });

    let card = hovered().and_then(|(index, x, y)| {
        with(|editor| {
            let Editor { session, textures, thumbs, .. } = editor;
            let prototype = session.prototypes().get(index)?;
            let (name, id, description) = (shown_name(prototype), prototype.id.clone(), prototype.description.clone());
            let picture = thumbs.entity(session, textures, index, BIG);
            let mut more = vec![id];
            if !description.is_empty() {
                more.push(description);
            }
            Some((x, y, picture, name, more))
        })
    });

    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            div { class: "px-2 pt-2 pb-1 text-xs text-muted", "Entities" }
            ChosenLine { chosen: chosen_id }
            div { class: "m-1",
                TextField { value: search, placeholder: "Search entities...", oninput: move |text: String| { with(|editor| editor.entity_search = text); bump(); } }
            }
            div { class: "flex-1 overflow-y-auto py-1",
                // The frame loop reads this area's scroll (`follow_entity_scroll`).
                onmounted: move |event| {
                    if let Some(handle) = event.downcast::<dioxus_native::NodeHandle>() {
                        ENTITY_LIST.with(|list| *list.borrow_mut() = Some((handle.clone(), view)));
                    }
                    list.set(Some(event.data()));
                },
                if rows.is_empty() {
                    div { class: "text-muted text-xs p-3 italic text-center", "No results" }
                }
                div { style: "position: relative; height: {total}px;",
                    for (at, row) in built {
                        match row {
                            EntityRowData::Category { path, short, count, open } => rsx! {
                                button { key: "c{path}",
                                    class: "flex items-center px-2 py-1 text-[10px] uppercase tracking-wider text-muted bg-surface cursor-pointer hover:bg-hover select-none w-full text-left border-none",
                                    style: "position: absolute; left: 0; right: 0; top: {at}px; height: {CATEGORY_ROW - 2.0}px;",
                                    onclick: move |_| {
                                        with(|editor| {
                                            if !editor.open_categories.remove(&path) {
                                                editor.open_categories.insert(path.clone());
                                            }
                                        });
                                        bump();
                                    },
                                    span { class: "text-muted mr-1.5 flex items-center", Chevron { open } }
                                    "{short}"
                                    span { class: "text-muted ml-1", "({count})" }
                                }
                            },
                            EntityRowData::Entity { index, name, id, picture, chosen, starred } => rsx! {
                                button { key: "e{index}",
                                    class: if chosen { "fav-row flex items-center gap-1.5 px-2 py-0.5 text-xs cursor-pointer w-full text-left border-none rounded-sm mb-px bg-active text-accent border border-accent" } else { "fav-row flex items-center gap-1.5 px-2 py-0.5 text-xs cursor-pointer w-full text-left border-none rounded-sm mb-px text-primary hover:bg-hover bg-transparent border border-transparent" },
                                    style: "position: absolute; left: 0; right: 0; top: {at}px; height: {ENTITY_ROW - 2.0}px;",
                                    onclick: move |_| {
                                        with(|editor| editor.session.choose_entity(index));
                                        bump();
                                    },
                                    onmouseenter: move |event| {
                                        let at = event.client_coordinates();
                                        hovered.set(Some((index, at.x, at.y)));
                                    },
                                    onmouseleave: move |_| hovered.set(None),
                                    span { style: "width: 32px; height: 32px; flex-shrink: 0;",
                                        if let Some(picture) = picture {
                                            img { src: "{picture}", class: "pixelated", style: "width: 32px; height: 32px;" }
                                        }
                                    }
                                    span { class: "flex flex-col overflow-hidden flex-1",
                                        span { class: "truncate", style: "font-size: 13px;", "{name}" }
                                        span { class: "truncate text-muted", style: "font-size: 10.5px;", "{id}" }
                                    }
                                    FavouriteStar { favourite: Favourite::new(FavouriteKind::Entity, id.clone()), on: starred }
                                }
                            },
                        }
                    }
                }
            }
            div { class: "px-2 py-1 text-[10px] text-muted border-t border-subtle", "{count_text}" }
        }
        if let Some((x, y, picture, name, more)) = card {
            HoverCard { x, y, picture, name, more }
        }
    }
}

/// One built row of the entity palette.
#[derive(Clone, PartialEq)]
enum EntityRowData {
    Category { path: String, short: String, count: usize, open: bool },
    Entity { index: usize, name: String, id: String, picture: Option<String>, chosen: bool, starred: bool },
}

/// The decal palette: the decals by tag or by a search, with their pictures, and the settings
/// they are placed with.
#[component]
fn DecalPalette() -> Element {
    let _ = watch();
    let mut hovered = use_signal(|| None::<(usize, f64, f64)>);
    let (search, chosen, groups, found, total) = with(|editor| {
        let search = editor.decal_search.clone();
        let lowered = search.to_lowercase();
        let words: Vec<&str> = lowered.split_whitespace().collect();
        let found = (!words.is_empty()).then(|| editor.session.decals_matching(&words));
        let groups: Vec<(String, Vec<usize>, bool)> = editor.session.decal_groups().into_iter().map(|group| {
            let open = editor.open_decal_groups.contains(&group.tag);
            (group.tag, group.decals, open)
        }).collect();
        editor.scroll_to_chosen = false;
        (search, editor.session.chosen_decal().map(str::to_owned), groups, found, editor.session.decal_prototypes.len())
    });

    let row = |index: usize| -> Element {
        let (id, picture, tags, starred) = with(|editor| {
            let Editor { session, textures, thumbs, .. } = editor;
            let picture = thumbs.decal(session, textures, index);
            let decal = &session.decal_prototypes[index];
            let starred = session.is_favourite(&Favourite::new(FavouriteKind::Decal, decal.id.clone()));
            (decal.id.clone(), picture, decal.tags.join(", "), starred)
        });
        let selected = chosen.as_deref() == Some(id.as_str());
        let pick = id.clone();
        let favourite = Favourite::new(FavouriteKind::Decal, id.clone());
        rsx! {
            button { key: "{id}",
                class: if selected { "fav-row flex items-center gap-1.5 px-2 py-0.5 text-xs cursor-pointer w-full text-left border-none rounded-sm mb-px bg-active text-accent border border-accent" } else { "fav-row flex items-center gap-1.5 px-2 py-0.5 text-xs cursor-pointer w-full text-left border-none rounded-sm mb-px text-primary hover:bg-hover bg-transparent border border-transparent" },
                style: "height: 28px;",
                onclick: move |_| {
                    with(|editor| editor.session.choose_decal(&pick));
                    bump();
                },
                onmouseenter: move |event| {
                    let at = event.client_coordinates();
                    hovered.set(Some((index, at.x, at.y)));
                },
                onmouseleave: move |_| hovered.set(None),
                span { class: "inline-block bg-surface border border-subtle rounded-sm flex-shrink-0", style: "width: 24px; height: 24px;",
                    if let Some(picture) = picture {
                        img { src: "{picture}", class: "pixelated", style: "width: 22px; height: 22px;" }
                    }
                }
                span { class: "truncate flex-1", title: "{tags}", "{id}" }
                FavouriteStar { favourite, on: starred }
            }
        }
    };

    let card = hovered().map(|(index, x, y)| {
        with(|editor| {
            let Editor { session, textures, thumbs, .. } = editor;
            let picture = thumbs.decal(session, textures, index);
            let decal = &session.decal_prototypes[index];
            let tags = decal.tags.join(", ");
            (x, y, picture, decal.id.clone(), if tags.is_empty() { Vec::new() } else { vec![format!("Tags: {tags}")] })
        })
    });

    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            div { class: "px-2 pt-2 pb-1 text-xs text-muted", "Decals" }
            ChosenLine { chosen: chosen.clone() }
            div { class: "m-1",
                TextField { value: search, placeholder: "Search decals...", oninput: move |text: String| { with(|editor| editor.decal_search = text); bump(); } }
            }
            div { class: "flex-1 overflow-y-auto py-1",
                div { class: "px-2",
                    if let Some(found) = found {
                        // A search shows one list of what it found, as the web editor does.
                        if found.is_empty() {
                            div { class: "text-muted text-xs p-3 italic text-center", "No results" }
                        }
                        for index in found {
                            {row(index)}
                        }
                    } else {
                        for (tag, decals, open) in groups {
                            button { key: "g{tag}",
                                class: "flex items-center px-2 py-1 text-[10px] uppercase tracking-wider text-muted bg-surface cursor-pointer hover:bg-hover select-none w-full text-left border-none",
                                onclick: {
                                    let tag = tag.clone();
                                    move |_| {
                                        with(|editor| {
                                            if !editor.open_decal_groups.remove(&tag) {
                                                editor.open_decal_groups.insert(tag.clone());
                                            }
                                        });
                                        bump();
                                    }
                                },
                                span { class: "text-muted mr-1.5 flex items-center", Chevron { open } }
                                "{tag}"
                                span { class: "text-muted ml-1", "({decals.len()})" }
                            }
                            if open {
                                div { class: "pl-2 pr-2",
                                    for index in decals {
                                        {row(index)}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "px-2 py-1 text-[10px] text-muted border-t border-subtle", "{total} decals" }
            DecalPlacement {}
        }
        if let Some((x, y, picture, name, more)) = card {
            HoverCard { x, y, picture, name, more }
        }
    }
}

/// The placement settings, as the web editor has them under its decal palette. A colour is
/// typed as #RRGGBBAA: Blitz has no colour picker.
#[component]
fn DecalPlacement() -> Element {
    let _ = watch();
    let settings = with(|editor| editor.session.decal_settings().clone());
    let set = |change: Box<dyn FnOnce(&mut editor_core::session::DecalSettings)>| {
        with(|editor| {
            let mut settings = editor.session.decal_settings().clone();
            change(&mut settings);
            editor.session.set_decal_settings(settings);
        });
        bump();
    };
    let degrees = format!("{:.1}", settings.angle.to_degrees());
    let swatch = settings.color.as_deref().and_then(decal_colour_bytes).map(|[red, green, blue, alpha]| format!("rgba({red}, {green}, {blue}, {})", alpha as f32 / 255.0));
    rsx! {
        div { class: "border-t border-subtle px-2 py-2 space-y-1.5",
            div { class: "text-[10px] text-muted uppercase tracking-wider", "Placement" }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-primary w-12", "Colour" }
                input { r#type: "checkbox", class: "check", checked: settings.color.is_some(),
                    oninput: move |event| {
                        let on = event.checked();
                        set(Box::new(move |settings| settings.color = on.then(|| "#FFFFFFFF".to_owned())));
                    }
                }
                if let Some(text) = settings.color.clone() {
                    span { class: "w-6 h-6 border border-subtle rounded-sm", style: "background: {swatch.clone().unwrap_or_default()}; width: 16px; height: 16px;" }
                    ColourField { value: text, onchange: move |text: String| set(Box::new(move |settings| settings.color = Some(text))) }
                } else {
                    span { class: "text-[10px] text-muted", "the decal's own" }
                }
            }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-primary w-12", "Angle" }
                NumberField { value: degrees, onchange: move |value: f64| set(Box::new(move |settings| settings.angle = value.clamp(0.0, 360.0).to_radians())) }
                span { class: "text-[10px] text-muted", "deg" }
            }
            div { class: "flex items-center gap-2",
                label { class: "text-[11px] text-primary w-12", "Z-index" }
                NumberField { value: settings.z_index.to_string(), onchange: move |value: f64| set(Box::new(move |settings| settings.z_index = value.round() as i32)) }
            }
            div { class: "flex items-center gap-4",
                label { class: "flex items-center gap-1 text-[11px] text-primary cursor-pointer",
                    input { r#type: "checkbox", class: "check", checked: settings.snap, oninput: move |event| { let on = event.checked(); set(Box::new(move |settings| settings.snap = on)); } }
                    "Snap"
                }
                label { class: "flex items-center gap-1 text-[11px] text-primary cursor-pointer",
                    input { r#type: "checkbox", class: "check", checked: settings.cleanable, oninput: move |event| { let on = event.checked(); set(Box::new(move |settings| settings.cleanable = on)); } }
                    "Cleanable"
                }
            }
        }
    }
}

/// A number typed into a small field, handed on with Enter or when the field lets go.
#[component]
pub(crate) fn NumberField(value: String, onchange: EventHandler<f64>, #[props(default)] disabled: bool) -> Element {
    let mut typed = use_signal(|| None::<String>);
    let shown = typed().unwrap_or(value.clone());
    let mut send = move || {
        if let Some(text) = typed.take() {
            if let Ok(number) = text.trim().trim_end_matches('\u{b0}').parse::<f64>() {
                onchange.call(number);
            }
        }
    };
    rsx! {
        TextField {
            value: shown,
            disabled,
            class: "flex-1 px-1 py-0.5 bg-surface border border-subtle rounded-sm text-primary text-[11px] outline-none focus:border-accent w-16",
            oninput: move |text: String| typed.set(Some(text)),
            onenter: move |_| send(),
            onleave: move |_| send(),
        }
    }
}

/// A colour typed as #RRGGBB or #RRGGBBAA, handed on with Enter or when the field lets go,
/// as the game writes it.
#[component]
pub(crate) fn ColourField(value: String, onchange: EventHandler<String>, #[props(default)] disabled: bool) -> Element {
    let mut typed = use_signal(|| None::<String>);
    let shown = typed().unwrap_or(value.clone());
    let mut send = move || {
        if let Some(text) = typed.take() {
            if let Some(bytes) = decal_colour_bytes(text.trim()) {
                onchange.call(decal_colour_text(bytes));
            }
        }
    };
    rsx! {
        TextField {
            value: shown,
            disabled,
            class: "px-1 py-0.5 bg-surface border border-subtle rounded-sm text-primary text-[11px] outline-none focus:border-accent font-mono w-24",
            oninput: move |text: String| typed.set(Some(text)),
            onenter: move |_| send(),
            onleave: move |_| send(),
        }
    }
}
