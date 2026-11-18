//! Device links: the links of the selected entity, and the link tool's port chooser for a pair.
//! The session decides and the engine makes each change. This file only draws what the session
//! offers and hands a choice back.

use dioxus_native::prelude::*;
use editor_core::model::Link;
use editor_core::session::LinkKind;

use crate::dialogs::Triangle;
use crate::{bump, watch, with};

const SMALL: &str = "px-1.5 py-0.5 rounded-sm border border-subtle text-primary text-[10px] cursor-pointer bg-elevated hover:bg-hover";

/// What a click in the links asks for.
#[derive(Clone, PartialEq)]
enum Row {
    Jump(i32),
    Unlink(Link),
    ListRemove(i32, i32),
}

/// The links and list entries of the one selected entity, each with a button that takes it
/// away and a click on the other end that jumps to it, and Auto-link Room for an alarm.
#[component]
pub(crate) fn LinksSection(uid: i32) -> Element {
    let _ = watch();
    let found = with(|editor| {
        let session = &editor.session;
        let rows = session.entity_link_rows(uid)?;
        if rows.outgoing.is_empty() && rows.incoming.is_empty() && rows.devices.is_empty() && rows.lists.is_empty() && !rows.alarm {
            return None;
        }
        let label = |other: i32| format!("{} #{other}", session.entity_label(other));
        // Each row: what it is, the other end, its ports by name with their ids when those
        // differ, and what its remove button does.
        let mut lines: Vec<(&'static str, i32, String, (String, Option<String>), Row)> = Vec::new();
        for link in &rows.outgoing {
            lines.push(("to", link.sink, label(link.sink), session.link_ports_text(&link.source_port, &link.sink_port), Row::Unlink(link.clone())));
        }
        for link in &rows.incoming {
            lines.push(("from", link.source, label(link.source), session.link_ports_text(&link.source_port, &link.sink_port), Row::Unlink(link.clone())));
        }
        for &device in &rows.devices {
            lines.push(("list", device, label(device), (String::new(), None), Row::ListRemove(uid, device)));
        }
        for &list in &rows.lists {
            lines.push(("on", list, label(list), ("its list".to_owned(), None), Row::ListRemove(list, uid)));
        }
        Some((lines, rows.alarm))
    });
    let Some((lines, alarm)) = found else { return rsx! {} };
    let ask = |row: Row| {
        with(|editor| match row {
            Row::Jump(other) => editor.session.jump_to_entity(other),
            Row::Unlink(link) => editor.session.unlink(&link),
            Row::ListRemove(list, device) => editor.session.list_remove(list, device),
        });
        bump();
    };
    rsx! {
        div { class: "mt-2",
            div { class: "font-bold text-xs mb-1", "Links" }
            for (index, (mark, other, label, ports, remove)) in lines.into_iter().enumerate() {
                div { key: "{index}", class: "flex items-center gap-1.5 py-px",
                    span { class: "text-muted text-[10px] flex items-center",
                        match mark {
                            "to" => rsx! { Triangle { towards: "right" } },
                            "from" => rsx! { Triangle { towards: "left" } },
                            _ => rsx! { "{mark}" },
                        }
                    }
                    button { class: "bg-transparent border-none text-accent text-[10px] cursor-pointer p-0", title: "Go to it",
                        onclick: move |_| ask(Row::Jump(other)),
                        "{label}"
                    }
                    if !ports.0.is_empty() {
                        span { class: "text-muted text-[10px]", "{ports.0}" }
                    }
                    if let Some(ids) = ports.1 {
                        span { class: "text-muted text-[9px] font-mono opacity-60", "{ids}" }
                    }
                    button { class: "bg-transparent border-none text-[11px] cursor-pointer px-1 ml-auto", style: "color: #cc4444;", title: "Remove",
                        onclick: move |_| ask(remove.clone()),
                        "\u{d7}"
                    }
                }
            }
            if alarm {
                button { class: "w-full px-1.5 py-1 rounded-sm text-[10px] cursor-pointer border border-subtle bg-elevated hover:bg-hover text-primary mt-1",
                    title: "Add every device in the alarm's room to its list",
                    onclick: move |_| { with(|editor| editor.session.auto_link_room()); bump(); },
                    "Auto-link room"
                }
            }
        }
    }
}

/// What a click in the chooser asks for.
enum Asked {
    SourcePort(String),
    SinkPort(String),
    Link,
    Unlink(Link),
    ListAdd,
    ListRemove,
    Close,
}

fn ask(asked: Asked) {
    with(|editor| {
        let Some(chooser) = editor.session.link_chooser() else { return };
        let session = &mut editor.session;
        match asked {
            Asked::SourcePort(port) => session.choose_source_port(&port),
            Asked::SinkPort(port) => session.choose_sink_port(&port),
            Asked::Link => session.make_link(),
            Asked::Unlink(link) => session.unlink(&link),
            Asked::ListAdd => session.list_add(chooser.source, chooser.sink),
            Asked::ListRemove => session.list_remove(chooser.source, chooser.sink),
            Asked::Close => session.close_link_chooser(),
        }
    });
    bump();
}

/// The link tool's window for a pair: the ports of both, the game's defaults first, and the
/// links the two have; or for a device list, whether the sink is on it. It stands at the top
/// right of the map and leaves the map free. Blitz has no drop-down
/// list, so each side's ports are a list of buttons.
#[component]
pub(crate) fn LinkDialog() -> Element {
    let _ = watch();
    // Each side's ports: the id, the name and the id again where it differs from the name.
    type Shown = Vec<(String, String, Option<String>)>;
    let Some((chooser, source, sink, source_ports, sink_ports, existing)) = with(|editor| {
        let session = &editor.session;
        let chooser = session.link_chooser()?;
        let (source, sink) = (session.entity_label(chooser.source), session.entity_label(chooser.sink));
        let shown = |ports: &[String], is_sink: bool| -> Shown {
            ports.iter().map(|port| { let (name, id) = session.port_shown(port, is_sink); (port.clone(), name, id) }).collect()
        };
        let (source_ports, sink_ports) = (shown(&chooser.source_ports, false), shown(&chooser.sink_ports, true));
        let existing: Vec<(Link, (String, Option<String>))> = chooser.existing.iter().map(|link| (link.clone(), session.link_ports_text(&link.source_port, &link.sink_port))).collect();
        Some((chooser, source, sink, source_ports, sink_ports, existing))
    }) else {
        return rsx! {};
    };
    let chosen_name = |chosen: &Option<String>, ports: &Shown| chosen.as_ref().map_or_else(|| "none".to_owned(), |port| ports.iter().find(|(id, ..)| id == port).map_or_else(|| port.clone(), |(_, name, _)| name.clone()));
    let ready = chooser.source_port.is_some() && chooser.sink_port.is_some() && !chooser.waiting;
    let muted = "text-muted text-[10px]";
    rsx! {
        div { class: "fixed bg-elevated border border-subtle rounded-lg p-3 text-primary text-xs shadow-lg",
            style: "right: 340px; top: 40px; width: 300px; z-index: 300;",
            onmousedown: move |event| event.stop_propagation(),
            div { class: "flex items-center justify-between mb-2",
                span { class: "font-bold text-accent", "Link" }
                button { class: "bg-transparent border-none text-muted hover:text-primary cursor-pointer text-lg leading-none px-1", onclick: move |_| ask(Asked::Close), "\u{d7}" }
            }
            div { class: "flex items-center gap-2 text-[11px] mb-2",
                "{source} #{chooser.source}"
                span { class: "text-muted flex items-center", Triangle { towards: "right" } }
                "{sink} #{chooser.sink}"
            }
            match chooser.kind {
                LinkKind::List => rsx! {
                    if chooser.listed {
                        div { class: muted, "On the device list." }
                        button { class: SMALL, disabled: chooser.waiting, onclick: move |_| ask(Asked::ListRemove), "Remove from list" }
                    } else {
                        button { class: SMALL, disabled: chooser.waiting, onclick: move |_| ask(Asked::ListAdd), "Add to list" }
                    }
                },
                LinkKind::Links => rsx! {
                    if chooser.asking {
                        div { class: muted, "Getting the ports of both devices..." }
                    }
                    for (label, ports, chosen, is_sink) in [("Source port", source_ports.clone(), chooser.source_port.clone(), false), ("Sink port", sink_ports.clone(), chooser.sink_port.clone(), true)] {
                        div { class: "mt-1",
                            div { class: muted, "{label}: {chosen_name(&chosen, &ports)}" }
                            div { class: "overflow-y-auto border border-subtle rounded-sm", style: "max-height: 120px;",
                                for (port, name, id) in ports {
                                    button {
                                        class: if chosen.as_ref() == Some(&port) { "flex w-full items-center gap-1.5 px-2 py-0.5 text-[11px] text-left border-none bg-active text-accent cursor-pointer" } else { "flex w-full items-center gap-1.5 px-2 py-0.5 text-[11px] text-left border-none bg-transparent text-primary cursor-pointer hover:bg-hover" },
                                        onclick: {
                                            let port = port.clone();
                                            move |_| ask(if is_sink { Asked::SinkPort(port.clone()) } else { Asked::SourcePort(port.clone()) })
                                        },
                                        span { "{name}" }
                                        if let Some(id) = id {
                                            span { class: "text-muted text-[10px] font-mono", "{id}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    button { class: "{SMALL} mt-2", disabled: !ready, onclick: move |_| ask(Asked::Link), "Link" }
                    if !existing.is_empty() {
                        div { class: "{muted} mt-2", "Already linked" }
                        for (link, (names, ids)) in existing.clone() {
                            div { class: "flex items-center gap-2",
                                div { class: "flex-1 flex items-center gap-1.5",
                                    span { class: "text-[10px]", "{names}" }
                                    if let Some(ids) = ids {
                                        span { class: "text-muted text-[9px] font-mono", "{ids}" }
                                    }
                                }
                                button { class: SMALL, disabled: chooser.waiting, onclick: move |_| ask(Asked::Unlink(link.clone())), "Unlink" }
                            }
                        }
                    }
                },
            }
            if let Some(error) = chooser.error.clone() {
                div { class: "text-[10px] mt-1", style: "color: rgb(240, 80, 80);", "{error}" }
            }
        }
    }
}
