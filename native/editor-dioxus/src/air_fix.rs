//! The dialog behind Map > Fix grid atmos.... It lets you pick the grids, explains what the fix
//! does, lists grids with no atmosphere with the option to give them one first, shows how many
//! tiles will change, and offers Fix or Cancel. The state and the questions to the engine live in
//! the session (editor-core's `session/air_fix.rs`).

use dioxus_native::prelude::*;

use crate::dialogs::{Modal, Radio};
use crate::{bump, watch, with};

#[component]
pub(crate) fn FixAirDialog() -> Element {
    let _ = watch();
    let Some(dialog) = with(|editor| editor.session.fix_air_dialog()) else { return rsx! {} };
    let close = move || {
        with(|editor| editor.session.cancel_fix_air());
        bump();
    };
    let choose = move |all: bool| {
        with(|editor| editor.session.choose_fix_air_grids(all));
        bump();
    };
    let can_fix = dialog.blocked.is_none();
    let without = dialog.without.join(", ");
    let choice = |chosen: bool| if chosen { "flex items-center gap-2 w-full px-3 py-1.5 text-left border border-subtle rounded cursor-pointer text-xs bg-active text-accent" } else { "flex items-center gap-2 w-full px-3 py-1.5 text-left border border-subtle rounded cursor-pointer text-xs bg-transparent text-primary hover:bg-hover" };
    rsx! {
        Modal { title: "Fix grid atmos", width: 460, onclose: move |_| close(),
            p { class: "text-[12px] text-primary mt-0 mb-3", "{dialog.does}" }
            div { class: "flex flex-col gap-1 mb-3",
                button { class: choice(dialog.all), onclick: move |_| choose(true),
                    span { class: "w-[14px] flex items-center justify-center", Radio { chosen: dialog.all } }
                    span { "{dialog.all_label}" }
                }
                if let Some(one) = dialog.one_label.clone() {
                    button { class: choice(!dialog.all), onclick: move |_| choose(false),
                        span { class: "w-[14px] flex items-center justify-center", Radio { chosen: !dialog.all } }
                        span { "{one}" }
                    }
                }
            }
            if !dialog.without.is_empty() {
                p { class: "text-[11px] text-warning mt-0 mb-1", "No atmosphere: {without}" }
                label { class: "flex items-center gap-2 text-[12px] text-primary mb-3 cursor-pointer",
                    input { r#type: "checkbox", class: "check", checked: dialog.add_first,
                        oninput: move |event| {
                            let add = event.checked();
                            with(|editor| editor.session.set_fix_air_add_first(add));
                            bump();
                        }
                    }
                    span { "{dialog.add_label}" }
                }
            }
            p { class: "text-[12px] text-muted mt-0 mb-2", "{dialog.count}" }
            for (index, why) in dialog.left_alone.iter().enumerate() {
                p { key: "{index}", class: "text-[11px] text-warning mt-0 mb-1", "Skipped: {why}" }
            }
            if let Some(blocked) = dialog.blocked.clone() {
                p { class: "text-[11px] text-danger mt-0 mb-1", "{blocked}" }
            }
            p { class: "text-[11px] text-muted mt-2 mb-0", "You can undo this in one step." }
            // Laid out here: the web editor's stylesheet has no classes for a row ending on the right.
            div { style: "display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px;",
                button { class: "bg-transparent border border-subtle rounded text-primary text-[13px] px-6 py-2 cursor-pointer hover:bg-hover", onclick: move |_| close(), "Cancel" }
                button {
                    disabled: !can_fix,
                    class: if can_fix { "bg-active border border-subtle rounded text-accent text-[13px] px-6 py-2 cursor-pointer hover:bg-hover" } else { "bg-transparent border border-subtle rounded text-muted text-[13px] px-6 py-2 cursor-default" },
                    onclick: move |_| {
                        if can_fix {
                            with(|editor| editor.session.confirm_fix_air());
                            bump();
                        }
                    },
                    "Fix"
                }
            }
        }
    }
}
