//! The toolbar's and the options bar's icons: SVG line drawings in `assets/icons`, most from
//! Lucide (see assets/CREDITS.md), shown as images. Blitz draws an SVG image through usvg, and
//! its fonts have no symbols to draw them with. Each is given its colour when it is made, since
//! an image does not follow the text colour, and kept so that it is made once.

use std::cell::RefCell;
use std::collections::HashMap;

use base64::Engine as _;
use dioxus_native::prelude::*;

/// Every icon, by name, as its file holds it.
const ICONS: &[(&str, &str)] = &[
    ("select", include_str!("../assets/icons/square-dashed-mouse-pointer.svg")),
    ("pan", include_str!("../assets/icons/hand.svg")),
    ("pick", include_str!("../assets/icons/pipette.svg")),
    ("paint", include_str!("../assets/icons/paintbrush.svg")),
    ("erase", include_str!("../assets/icons/eraser.svg")),
    ("fill", include_str!("../assets/icons/paint-bucket.svg")),
    ("room", include_str!("../assets/icons/house.svg")),
    ("link", include_str!("../assets/icons/link.svg")),
    ("cables", include_str!("../assets/icons/cable.svg")),
    ("pipes", include_str!("../assets/icons/pipe.svg")),
    ("tiles", include_str!("../assets/icons/grid-2x2.svg")),
    ("entities", include_str!("../assets/icons/box.svg")),
    ("decals", include_str!("../assets/icons/spray-can.svg")),
    ("point", include_str!("../assets/icons/point.svg")),
    ("line", include_str!("../assets/icons/line.svg")),
    ("rectangle", include_str!("../assets/icons/square.svg")),
    ("circle", include_str!("../assets/icons/circle.svg")),
    ("box", include_str!("../assets/icons/square-dashed.svg")),
    ("ellipse", include_str!("../assets/icons/circle-dashed.svg")),
    ("lasso", include_str!("../assets/icons/lasso.svg")),
    ("polygon", include_str!("../assets/icons/polygon-lasso.svg")),
    ("same", include_str!("../assets/icons/wand-sparkles.svg")),
    ("replace", include_str!("../assets/icons/square.svg")),
    ("add", include_str!("../assets/icons/square-plus.svg")),
    ("subtract", include_str!("../assets/icons/square-minus.svg")),
];

thread_local! {
    static MADE: RefCell<HashMap<(&'static str, &'static str), String>> = RefCell::new(HashMap::new());
}

/// An icon in a colour, as a data URI an image shows. None for a name with no icon.
pub(crate) fn icon_uri(name: &'static str, colour: &'static str) -> Option<String> {
    MADE.with(|made| {
        if let Some(uri) = made.borrow().get(&(name, colour)) {
            return Some(uri.clone());
        }
        let (_, source) = ICONS.iter().find(|(known, _)| *known == name)?;
        let svg = source.replace("currentColor", colour);
        let uri = format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg));
        made.borrow_mut().insert((name, colour), uri.clone());
        Some(uri)
    })
}

/// An icon, square, `size` pixels wide, in a colour (`#rrggbb`).
#[component]
pub(crate) fn Icon(name: &'static str, colour: &'static str, #[props(default = 20)] size: u32) -> Element {
    match icon_uri(name, colour) {
        Some(uri) => rsx! { img { class: "icon", src: "{uri}", style: "width: {size}px; height: {size}px;" } },
        None => rsx! { span { style: "width: {size}px; height: {size}px;" } },
    }
}
