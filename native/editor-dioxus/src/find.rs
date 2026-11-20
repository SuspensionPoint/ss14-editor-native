//! The entity search (Ctrl+F) and Validate Map. The session does the finding and checking. This
//! file only draws what it found and hands a choice back, in the markup of the web editor's
//! EntitySearchBar.tsx and ValidatorModal.tsx.

use std::collections::HashSet;

use dioxus_native::prelude::*;
use editor_core::search::{Found, MOST_FOUND};
use editor_core::session::Session;
use editor_core::validate::{grouped, Fix, Severity, NOT_CHECKED};

use crate::dialogs::{Chevron, Dot, Modal, TextField, Tick, Triangle};
use crate::{bump, watch, with, Editor};

/// The entity search. Its query stays for the next time it is opened.
#[derive(Default)]
pub(crate) struct FindDialog {
    pub(crate) open: bool,
    query: String,
    /// The row Enter jumps to, which the arrow keys move.
    current: usize,
    /// What the query found, and what it was found for: the query, the document and its
    /// revision, and how many entities it had.
    found: Vec<Found>,
    found_for: Option<(String, i32, i32, usize)>,
}

impl FindDialog {
    pub(crate) fn show(&mut self, _session: &Session) {
        self.open = true;
    }

    /// Searches again if the query or the document changed since.
    fn refresh(&mut self, session: &Session) {
        let key = session.doc.as_ref().map(|doc| (self.query.clone(), doc.id, doc.history.revision, doc.entities.len()));
        if key != self.found_for {
            self.found = session.find_entities(&self.query);
            self.found_for = key;
            self.current = 0;
        }
    }
}

/// Jumps to a result and closes the search.
fn jump(editor: &mut Editor, uid: i32) {
    editor.session.jump_to_entity(uid);
    editor.find.open = false;
}

#[component]
pub(crate) fn FindDialogs() -> Element {
    let _ = watch();
    let (open, validating) = with(|editor| (editor.find.open, editor.validation.is_some()));
    rsx! {
        if open {
            Find {}
        }
        if validating {
            Validation {}
        }
    }
}

#[component]
fn Find() -> Element {
    let _ = watch();
    let (query, found, current) = with(|editor| {
        let Editor { find, session, .. } = editor;
        find.refresh(session);
        (find.query.clone(), find.found.clone(), find.current)
    });
    let count = found.len();
    let results = if count == MOST_FOUND { format!("First {count} results") } else { format!("{count} result{}", if count == 1 { "" } else { "s" }) };
    rsx! {
        Modal { title: "Find entity", width: 520, onclose: move |_| { with(|editor| editor.find.open = false); bump(); },
            TextField { value: query.clone(), placeholder: "Search entities by id, name or uid...", focus: true,
                oninput: move |text: String| { with(|editor| editor.find.query = text); bump(); },
                onenter: move |_| {
                    with(|editor| {
                        if let Some(uid) = editor.find.found.get(editor.find.current).map(|found| found.uid) {
                            jump(editor, uid);
                        }
                    });
                    bump();
                },
                onkey: move |key: Key| {
                    let step: isize = match key {
                        Key::Named(NamedKey::ArrowDown) => 1,
                        Key::Named(NamedKey::ArrowUp) => -1,
                        _ => return,
                    };
                    with(|editor| {
                        let count = editor.find.found.len() as isize;
                        if count > 0 {
                            editor.find.current = (editor.find.current as isize + step).rem_euclid(count) as usize;
                        }
                    });
                    bump();
                },
            }
            if !query.trim().is_empty() {
                div { class: "text-[11px] text-muted mt-1", "{results}" }
            }
            div { class: "mt-1 overflow-y-auto", style: "max-height: 360px;",
                for (index, result) in found.into_iter().enumerate() {
                    {
                        let mut text = result.name.clone();
                        if result.name != result.prototype {
                            text += &format!("  {}", result.prototype);
                        }
                        let place = format!("({}, {})  #{}", result.cell.0, result.cell.1, result.uid);
                        let uid = result.uid;
                        rsx! {
                            button { key: "{uid}",
                                class: if index == current { "flex items-center gap-2 w-full px-3 py-1.5 text-left border-none cursor-pointer text-xs bg-active text-accent" } else { "flex items-center gap-2 w-full px-3 py-1.5 text-left border-none cursor-pointer text-xs bg-transparent text-primary hover:bg-hover" },
                                onclick: move |_| { with(|editor| jump(editor, uid)); bump(); },
                                span { class: "flex-1 truncate", "{text}" }
                                span { class: "text-muted text-[11px]", "{place}" }
                            }
                        }
                    }
                }
            }
            if !query.trim().is_empty() && count == 0 {
                div { class: "px-3 py-2 text-xs text-muted italic", "No results" }
            }
        }
    }
}

#[component]
fn Validation() -> Element {
    let _ = watch();
    let mut open = use_signal(HashSet::<&'static str>::new);
    let Some((findings, links_known)) = with(|editor| {
        // Found again when the map or its air changed while the list is open, as after a Fix.
        let key = editor.session.validation_key();
        if editor.validation.is_some() && key != editor.validated {
            editor.validation = Some(editor.session.validate());
            editor.validated = key;
        }
        editor.validation.clone().map(|findings| (findings, editor.session.links_known()))
    }) else {
        return rsx! {};
    };
    let count = |severity: Severity| findings.iter().filter(|finding| finding.rule.severity() == severity).count();
    let (errors, warnings, notices) = (count(Severity::Error), count(Severity::Warning), count(Severity::Notice));
    let plural = |count: usize, word: &str| format!("{count} {word}{}", if count == 1 { "" } else { "s" });
    let groups: Vec<(&'static str, Severity, Vec<editor_core::validate::Finding>)> = grouped(&findings).into_iter().map(|(rule, found)| (rule.label(), rule.severity(), found.into_iter().cloned().collect())).collect();
    let close = move || {
        with(|editor| editor.validation = None);
        bump();
    };
    let skipped: Vec<String> = NOT_CHECKED.iter().map(|(label, needs)| format!("{label} (needs {needs})")).collect();
    rsx! {
        Modal { title: "Map validation", width: 550, onclose: move |_| close(),
            if findings.is_empty() {
                div { class: "text-center py-8",
                    div { class: "text-success mb-3 flex justify-center", Tick { size: 26 } }
                    div { class: "text-success font-semibold", "No issues found" }
                }
            } else {
                div { class: "mb-4 text-[12px]",
                    if errors > 0 {
                        span { class: "text-danger font-semibold", "{plural(errors, \"error\")}" }
                    }
                    if errors > 0 && warnings > 0 {
                        span { class: "text-muted", ", " }
                    }
                    if warnings > 0 {
                        span { class: "text-warning font-semibold", "{plural(warnings, \"warning\")}" }
                    }
                    if notices > 0 && errors + warnings > 0 {
                        span { class: "text-muted", ", " }
                    }
                    if notices > 0 {
                        span { class: "text-accent font-semibold", "{plural(notices, \"notice\")}" }
                    }
                }
                for (label, severity, found) in groups {
                    {
                        let expanded = open().contains(label);
                        // A button for the whole group, where its findings are fixed together
                        // in one edit: removing extras or laying plating.
                        let all: Vec<editor_core::validate::Finding> = found.iter().filter(|finding| matches!(finding.fix(), Some(Fix::RemoveExtras | Fix::LayPlating))).cloned().collect();
                        let fix_all = (all.len() > 1).then(|| match all[0].fix() {
                            Some(Fix::RemoveExtras) => ("Remove all extras", "Keep one on each cell and delete the rest. One undo puts them all back."),
                            _ => ("Lay plating under all", "Put plating under each one on a grid and anchor it. One undo takes it all away."),
                        });
                        rsx! {
                            div { key: "{label}", class: "mb-3",
                                div { class: "flex items-center gap-2",
                                    button { class: "flex items-center gap-1.5 flex-1 text-left bg-transparent border-none cursor-pointer py-1 px-0 hover:bg-hover rounded",
                                        onclick: move |_| {
                                            let mut shown = open.write();
                                            if !shown.remove(label) {
                                                shown.insert(label);
                                            }
                                        },
                                        span { class: "text-muted flex items-center", Chevron { open: expanded } }
                                        match severity {
                                            Severity::Error => rsx! { span { class: "text-danger flex items-center", Dot { filled: true, size: 9 } } },
                                            Severity::Warning => rsx! { span { class: "text-warning flex items-center", Triangle { towards: "up", size: 5 } } },
                                            Severity::Notice => rsx! { span { class: "text-accent flex items-center", Dot { filled: false, size: 9 } } },
                                        }
                                        span { class: "font-semibold text-primary text-[13px]", "{label}" }
                                        span { class: "text-muted text-[11px]", "({found.len()})" }
                                    }
                                    if let Some((words, does)) = fix_all {
                                        button { class: "bg-active border border-subtle rounded text-accent text-[11px] px-2 py-0.5 cursor-pointer hover:bg-hover",
                                            title: "{does}",
                                            onclick: move |_| {
                                                with(|editor| editor.session.fix_findings(&all));
                                                bump();
                                            },
                                            "{words}"
                                        }
                                    }
                                }
                                if expanded {
                                    div { class: "ml-5 mt-0.5",
                                        for (index, finding) in found.into_iter().enumerate() {
                                            {
                                                let fix = finding.fix();
                                                let message = finding.message.clone();
                                                let fixed = finding.clone();
                                                rsx! {
                                                    div { key: "{index}", class: "flex items-start gap-2 text-[11px] text-primary py-0.5 px-1 rounded hover:bg-hover cursor-pointer",
                                                        // As in the web editor, a click jumps and closes the list.
                                                        onclick: move |_| {
                                                            with(|editor| {
                                                                editor.session.jump_to_finding(&finding);
                                                                editor.validation = None;
                                                            });
                                                            bump();
                                                        },
                                                        span { class: "flex-1", "{message}" }
                                                        // Its fix, at once. The list stays and is found again, except
                                                        // after a Select, which closes it as a jump does.
                                                        if let Some(fix) = fix {
                                                            button { class: "bg-active border border-subtle rounded text-accent text-[11px] px-2 py-0.5 cursor-pointer hover:bg-hover whitespace-nowrap",
                                                                title: "{fix.does()}",
                                                                onclick: move |event| {
                                                                    event.stop_propagation();
                                                                    with(|editor| {
                                                                        editor.session.fix_finding(&fixed);
                                                                        if matches!(fix, Fix::Select(_)) {
                                                                            editor.validation = None;
                                                                        }
                                                                    });
                                                                    bump();
                                                                },
                                                                "{fix.label()}"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // The rules of links, while the engine has not listed the links.
            if !links_known {
                div { class: "text-[11px] text-muted mt-2", "Not checked because the engine couldn't list the links: {skipped.join(\", \")}." }
            }
            div { class: "text-center mt-4",
                button { class: "bg-active border border-subtle rounded text-primary text-[13px] px-6 py-2 cursor-pointer hover:bg-hover", onclick: move |_| close(), "Close" }
            }
        }
    }
}
