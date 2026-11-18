//! How the start screen looks (`start.rs` holds its state and actions): a welcome, the mapper's
//! forks as cards, the most recently opened maps, the popular forks, and the two ways to add a
//! fork of their own. With no fork known yet it shows the welcome, the first-run steps, the
//! popular forks and the two ways to add one. Anything running or gone wrong shows on the card it
//! belongs to.

use std::path::Path;

use dioxus_native::prelude::*;
use editor_core::files::forward_slashes;
use editor_core::forks::download::{self, Download};
use editor_core::forks::popular::{self, PopularFork, POPULAR_FORKS};
use editor_core::forks::{self};

use crate::dialogs::{Dot, Radio, TextField, Triangle};
use crate::start::{self, with_start, Form, JobView, Message, Place, Start};
use crate::{bump, watch};

/// A fork's badge: its owner's avatar, or its monogram on its colour.
#[derive(Clone, Debug, PartialEq)]
struct BadgeView {
    letters: String,
    colour: String,
    picture: Option<String>,
}

/// A known fork's card.
#[derive(Clone, Debug, PartialEq)]
struct ForkCard {
    path: String,
    name: String,
    branch: Option<String>,
    last_map: Option<String>,
    /// Downloaded by the editor: it has Update, and Remove may delete its files.
    managed: bool,
    /// Its folder is not there.
    missing: bool,
    /// Its Remove asks whether to delete the files.
    asking: bool,
    badge: BadgeView,
    job: Option<JobView>,
    message: Option<Message>,
}

/// A popular fork's card.
#[derive(Clone, Debug, PartialEq)]
struct PopularCard {
    fork: &'static PopularFork,
    badge: BadgeView,
    /// The repository chosen, for a fork with more than one.
    chosen: usize,
    /// The known fork that is one of its repositories, by its path.
    added: Option<String>,
    job: Option<JobView>,
    message: Option<Message>,
}

/// One recent map's row.
#[derive(Clone, Debug, PartialEq)]
struct MapRow {
    map: String,
    fork: String,
    fork_name: String,
    badge: BadgeView,
}

/// The two ways to add a fork of one's own.
#[derive(Clone, Debug, PartialEq)]
struct AddView {
    form: Form,
    git: Option<bool>,
    adding: bool,
    checkout_message: Option<Message>,
    url_job: Option<JobView>,
    url_message: Option<Message>,
}

/// Everything the screen shows, read from the state at once.
struct View {
    forks: Vec<ForkCard>,
    popular: Vec<PopularCard>,
    maps: Vec<MapRow>,
    add: AddView,
    top: Vec<Message>,
    busy: bool,
}

fn badge(start: &Start, name: &str, url: Option<&str>) -> BadgeView {
    let owner = url.and_then(popular::github_owner);
    BadgeView {
        letters: popular::monogram(name),
        colour: popular::colour_for(name, url).to_owned(),
        picture: owner.and_then(|owner| start.avatars.get(&owner.to_lowercase()).cloned()),
    }
}

impl View {
    fn of(start: &Start) -> View {
        let jobs = start::jobs(start);
        let job_at = |place: &Place| jobs.iter().find(|(at, _)| at == place).map(|(_, job)| job.clone());
        let message_at = |place: &Place| start.messages.iter().find(|message| &message.place == place).cloned();
        let forks: Vec<ForkCard> = start
            .settings
            .forks
            .iter()
            .map(|fork| {
                let place = Place::Fork(fork.path.clone());
                ForkCard {
                    path: fork.path.clone(),
                    name: fork.name.clone(),
                    branch: start.branches.get(&fork.path).cloned().flatten(),
                    last_map: fork.last_map.clone(),
                    managed: fork.managed,
                    missing: start.missing.contains(&fork.path),
                    asking: start.removing.as_deref() == Some(fork.path.as_str()),
                    badge: badge(start, &fork.name, start::url_of(start, &fork.path).as_deref()),
                    job: job_at(&place),
                    message: message_at(&place),
                }
            })
            .collect();
        let popular = POPULAR_FORKS
            .iter()
            .map(|fork| {
                let place = Place::Popular(fork.name);
                // A fork whose folder is gone cannot open, so it does not count.
                let added = start.settings.forks.iter().filter(|known| !start.missing.contains(&known.path)).find(|known| popular::is_popular_repository(fork, start::url_of(start, &known.path).as_deref())).map(|known| known.path.clone());
                PopularCard {
                    fork,
                    badge: badge(start, fork.name, Some(fork.repositories[0].url)),
                    chosen: start.choices.get(fork.name).copied().unwrap_or(0),
                    added,
                    job: job_at(&place),
                    message: message_at(&place),
                }
            })
            .collect();
        let maps = start
            .settings
            .recent_maps
            .iter()
            .map(|recent| {
                let known = start.settings.fork(&recent.fork);
                let fork_name = known.map_or_else(|| forks::default_name(Path::new(&recent.fork)), |fork| fork.name.clone());
                MapRow { map: recent.map.clone(), fork: recent.fork.clone(), badge: badge(start, &fork_name, start::url_of(start, &recent.fork).as_deref()), fork_name }
            })
            .collect();
        let add = AddView {
            form: start.form.clone(),
            git: start.git,
            adding: start.adding.is_some(),
            checkout_message: message_at(&Place::Checkout),
            url_job: job_at(&Place::Url),
            url_message: message_at(&Place::Url),
        };
        // Words for a card that is not shown, as a fork taken off the list, go over the sections.
        let shown = |place: &Place| match place {
            Place::Top => false,
            Place::Fork(path) => forks.iter().any(|fork| &fork.path == path),
            _ => true,
        };
        let top = start.messages.iter().filter(|message| !shown(&message.place)).cloned().collect();
        View { forks, popular, maps, add, top, busy: start::busy(start) }
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// The folder a map is in, with forward slashes.
fn folder_of(path: &str) -> String {
    Path::new(path).parent().map_or_else(String::new, forward_slashes)
}

#[component]
pub(crate) fn StartScreen() -> Element {
    let _ = watch();
    let Some(view) = with_start(|start| View::of(start)) else {
        return rsx! {};
    };
    let View { forks, popular, maps, add, top, busy } = view;
    let first_run = forks.is_empty();
    let fork_count = forks.len();

    rsx! {
        div { class: "start",
            div { class: "start-page",
                div { class: "start-header",
                    Logo {}
                    div {
                        div { class: "start-title", "SS14 Map Editor" }
                        div { class: "start-purpose", "Edit Space Station 14 maps with your fork's own prototypes, textures and code." }
                    }
                }
                for message in top {
                    div { class: "start-top-note",
                        NoteBox { message }
                    }
                }
                if first_run {
                    div { class: "start-section",
                        SectionLabel { text: "GETTING STARTED" }
                        div { class: "start-steps",
                            Step { number: 1, title: "Choose a fork", words: "Download one of the popular forks below, or use a checkout you already have." }
                            Step { number: 2, title: "Download", words: "Git fetches the latest commit and its engine. Expect a few minutes and about a gigabyte." }
                            Step { number: 3, title: "First open", words: "Prototypes pack in seconds and textures bake in about a minute. If the fork's code differs from the editor's, its engine side is built too, which takes a few minutes with the .NET SDK." }
                        }
                    }
                } else {
                    div { class: "start-section",
                        SectionLabel { text: "YOUR FORKS", count: fork_count }
                        div { class: "start-grid",
                            for card in forks {
                                ForkCardView { key: "{card.path}", card, busy }
                            }
                        }
                    }
                    div { class: "start-section",
                        SectionLabel { text: "RECENT MAPS" }
                        if maps.is_empty() {
                            div { class: "start-empty", "Maps you open or save appear here, so you can reopen them in one click." }
                        } else {
                            div { class: "start-maps",
                                for row in maps {
                                    MapLine { key: "{row.map}", row, busy }
                                }
                            }
                        }
                    }
                }
                div { class: "start-section",
                    SectionLabel { text: "POPULAR FORKS" }
                    div { class: "start-section-words", "One click downloads a fork's default branch with git (latest commit only) into the editor's own folder." }
                    div { class: "start-grid",
                        for card in popular {
                            PopularCardView { key: "{card.fork.name}", card, busy, git: add.git }
                        }
                    }
                }
                div { class: "start-section",
                    SectionLabel { text: "ADD YOUR OWN" }
                    AddYourOwn { add, busy }
                }
            }
        }
    }
}

/// The app's mark: four floor tiles, two lit.
#[component]
fn Logo() -> Element {
    rsx! {
        div { class: "start-logo",
            div { class: "start-logo-tile lit" }
            div { class: "start-logo-tile" }
            div { class: "start-logo-tile" }
            div { class: "start-logo-tile lit" }
        }
    }
}

/// A section's label in capitals, with a count and a rule after it.
#[component]
fn SectionLabel(text: String, #[props(default)] count: Option<usize>) -> Element {
    rsx! {
        div { class: "start-label",
            span { "{text}" }
            if let Some(count) = count {
                span { class: "start-label-count", "{count}" }
            }
            div { class: "start-label-rule" }
        }
    }
}

/// One step of the first run.
#[component]
fn Step(number: u32, title: String, words: String) -> Element {
    rsx! {
        div { class: "start-step",
            div { class: "start-step-number", "{number}" }
            div {
                div { class: "start-step-title", "{title}" }
                div { class: "start-step-words", "{words}" }
            }
        }
    }
}

/// A fork's badge, this many pixels each way.
#[component]
fn Badge(badge: BadgeView, #[props(default = 44)] size: u32) -> Element {
    let BadgeView { letters, colour, picture } = badge;
    let font = if letters.chars().count() > 2 { size * 3 / 10 } else { size * 2 / 5 };
    let radius = (size / 5).max(3);
    rsx! {
        div { class: "start-badge", style: "width: {size}px; height: {size}px; border-radius: {radius}px; background: {colour}; font-size: {font}px;",
            if let Some(picture) = picture {
                img { src: "{picture}", style: "width: {size}px; height: {size}px; border-radius: {radius}px;" }
            } else {
                "{letters}"
            }
        }
    }
}

/// What runs in a card: its title, time so far, stage, a bar and Cancel.
#[component]
fn JobPanel(job: JobView) -> Element {
    let JobView { title, stage, percent, time, can_stop, stopping } = job;
    rsx! {
        div { class: "start-job",
            div { class: "start-job-head",
                span { class: "start-job-title", if stopping { "Stopping..." } else { "{title}" } }
                span { class: "start-job-time", "{time} so far" }
                if can_stop {
                    button { class: "start-button small", disabled: stopping,
                        onclick: move |_| {
                            with_start(start::stop);
                            bump();
                        },
                        "Cancel"
                    }
                }
            }
            div { class: "start-job-stage", "{stage}" }
            div { class: "start-bar",
                if let Some(percent) = percent {
                    div { class: "start-bar-fill", style: "width: {percent}%;" }
                } else {
                    div { class: "start-bar-unknown" }
                }
            }
        }
    }
}

/// How many lines of a message show before Show all.
const FIRST_LINES: usize = 4;

/// Words in a card: a failure in the danger colour, or a note, with Dismiss.
#[component]
fn NoteBox(message: Message) -> Element {
    let Message { place, text, refused, expanded } = message;
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let count = lines.len();
    let shown: Vec<String> = if expanded { lines } else { lines.into_iter().take(FIRST_LINES).collect() };
    let (dismissed, toggled) = (place.clone(), place);
    rsx! {
        div { class: if refused { "start-note error" } else { "start-note info" },
            for (at, line) in shown.into_iter().enumerate() {
                div { key: "{at}", class: if at == 0 { "start-note-line first" } else { "start-note-line" }, "{line}" }
            }
            div { class: "start-note-actions",
                if count > FIRST_LINES {
                    button { class: "start-link",
                        onclick: move |_| {
                            with_start(|start| start::expand(start, &toggled));
                            bump();
                        },
                        if expanded { "Show fewer lines" } else { "Show all {count} lines" }
                    }
                }
                button { class: "start-link",
                    onclick: move |_| {
                        with_start(|start| start::dismiss(start, &dismissed));
                        bump();
                    },
                    "Dismiss"
                }
            }
        }
    }
}

#[component]
fn ForkCardView(card: ForkCard, busy: bool) -> Element {
    let ForkCard { path, name, branch, last_map, managed, missing, asking, badge, job, message } = card;
    let (open, open_last, update, remove, delete, keep) = (path.clone(), path.clone(), path.clone(), path.clone(), path.clone(), path.clone());
    let last = last_map.clone();
    let can_open = !busy && !missing;
    let card_class = if job.is_some() { "start-card working" } else { "start-card" };
    rsx! {
        div { class: card_class,
            div { class: "start-card-top",
                Badge { badge }
                div { class: "start-card-title",
                    div { class: "start-name", "{name}" }
                    div { class: "start-name-row",
                        if missing {
                            span { class: "start-tag missing", "FOLDER MISSING" }
                        } else if managed {
                            span { class: "start-tag downloaded", "DOWNLOADED" }
                        } else {
                            span { class: "start-tag", "YOUR CHECKOUT" }
                        }
                        if let Some(branch) = branch {
                            div { class: "start-branch",
                                span { class: "start-branch-mark" }
                                span { class: "start-branch-name", "{branch}" }
                            }
                        } else {
                            span { class: "start-muted", "Not a git checkout" }
                        }
                    }
                }
                // Out of the way of the actions, as it is used least.
                button { class: "start-button quiet small start-corner", disabled: busy || asking,
                    onclick: move |_| {
                        with_start(|start| start::ask_remove(start, &remove));
                        bump();
                    },
                    "Remove"
                }
            }
            div { class: "start-details",
                div { class: "start-path", "{path}" }
                div { class: "start-last",
                    span { class: "start-last-label", "LAST MAP" }
                    if let Some(map) = &last_map {
                        span { class: "start-last-name", "{file_name(map)}" }
                    } else {
                        span { class: "start-muted", "None yet" }
                    }
                }
            }
            if let Some(job) = job {
                JobPanel { job }
            } else if let Some(message) = message {
                NoteBox { message }
            }
            if asking {
                div { class: "start-ask",
                    div { class: "start-ask-words", "Remove {name} from the list? You can also delete its download from disk, or keep it and add it back later with Use a checkout I have." }
                    div { class: "start-actions",
                        button { class: "start-button danger", disabled: busy,
                            onclick: move |_| {
                                with_start(|start| start::begin_delete(start, delete.clone()));
                                bump();
                            },
                            "Delete the files"
                        }
                        button { class: "start-button", disabled: busy,
                            onclick: move |_| {
                                with_start(|start| start::remove_from_list(start, &keep));
                                bump();
                            },
                            "Keep the files"
                        }
                        div { class: "start-spacer" }
                        button { class: "start-button quiet", disabled: busy,
                            onclick: move |_| {
                                with_start(|start| start.removing = None);
                                bump();
                            },
                            "Cancel"
                        }
                    }
                }
            } else {
                div { class: "start-actions",
                    if last.is_some() {
                        button { class: "start-button primary", disabled: !can_open,
                            onclick: move |_| {
                                with_start(|start| start::begin_check(start, open_last.clone(), last.clone()));
                                bump();
                            },
                            "Open last map"
                        }
                    }
                    button { class: if last_map.is_some() { "start-button" } else { "start-button primary" }, disabled: !can_open,
                        onclick: move |_| {
                            with_start(|start| start::begin_check(start, open.clone(), None));
                            bump();
                        },
                        "Open"
                    }
                    if managed {
                        button { class: "start-button", disabled: !can_open,
                            onclick: move |_| {
                                with_start(|start| start::begin_update(start, update.clone()));
                                bump();
                            },
                            "Update"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PopularCardView(card: PopularCard, busy: bool, git: Option<bool>) -> Element {
    let PopularCard { fork, badge, chosen, added, job, message } = card;
    let choosing = fork.repositories.len() > 1 && added.is_none();
    let can_download = !busy && git != Some(false);
    let card_class = if job.is_some() { "start-card working" } else { "start-card" };
    rsx! {
        div { class: card_class,
            div { class: "start-card-top",
                Badge { badge }
                div { class: "start-card-title",
                    div { class: "start-name-row",
                        span { class: "start-name", "{fork.name}" }
                        if added.is_some() {
                            span { class: "start-tag added", "ADDED" }
                        } else if fork.upstream {
                            span { class: "start-tag upstream", "UPSTREAM" }
                        }
                    }
                    div { class: "start-muted", "{fork.licence}" }
                }
            }
            div { class: "start-desc", "{fork.description}" }
            if fork.own_engine {
                div { class: "start-engine-note",
                    span { class: "start-engine-dot", Dot { filled: true, size: 6 } }
                    span { "Has its own engine fork. The first open may build an engine side for it with the .NET SDK, which takes minutes. If the editor's engine patches don't fit that engine, it won't open." }
                }
            }
            if choosing {
                div { class: "start-choices",
                    for (at, repository) in fork.repositories.iter().enumerate() {
                        button { key: "{repository.url}", class: if at == chosen { "start-choice chosen" } else { "start-choice" }, disabled: busy,
                            onclick: move |_| {
                                with_start(|start| start.choices.insert(fork.name, at));
                                bump();
                            },
                            Radio { chosen: at == chosen }
                            span { "{repository.label}" }
                        }
                    }
                }
            }
            if let Some(job) = job {
                JobPanel { job }
            } else if let Some(message) = message {
                NoteBox { message }
            }
            div { class: "start-actions",
                if let Some(path) = added {
                    button { class: "start-button primary", disabled: busy,
                        onclick: move |_| {
                            with_start(|start| start::begin_check(start, path.clone(), None));
                            bump();
                        },
                        "Open"
                    }
                    span { class: "start-muted", "In Your forks" }
                } else {
                    button { class: "start-button primary", disabled: !can_download,
                        onclick: move |_| {
                            with_start(|start| start::begin_popular(start, fork));
                            bump();
                        },
                        "Download"
                    }
                    if git == Some(false) {
                        span { class: "start-muted", "Needs git" }
                    }
                }
            }
        }
    }
}

/// A folder, drawn: a tab over a box.
#[component]
fn FolderMark() -> Element {
    rsx! {
        div { class: "start-folder",
            div { class: "start-folder-tab" }
            div { class: "start-folder-body" }
        }
    }
}

/// A download, drawn: a triangle pointing down onto a line.
#[component]
fn DownloadMark() -> Element {
    rsx! {
        div { class: "start-download",
            div { class: "start-download-stem" }
            Triangle { towards: "down", size: 7 }
            div { class: "start-download-base" }
        }
    }
}

/// Use a checkout I have, and Download from a URL, side by side.
#[component]
fn AddYourOwn(add: AddView, busy: bool) -> Element {
    let AddView { form, git, adding, checkout_message, url_job, url_message } = add;
    let Form { url, branch } = form;
    let folder = forward_slashes(&Download::new(download::forks_folder(), &url, &branch).folder());
    let can_download = !busy && git != Some(false) && !url.trim().is_empty();
    let url_class = if url_job.is_some() { "start-card working" } else { "start-card" };
    rsx! {
        div { class: "start-two",
            div { class: "start-card",
                div { class: "start-card-top",
                    div { class: "start-icon", FolderMark {} }
                    div { class: "start-card-title",
                        div { class: "start-name", "Use a checkout I have" }
                        div { class: "start-muted", "A fork you cloned yourself" }
                    }
                }
                div { class: "start-desc", "Choose the fork's folder, the one that holds Resources, RobustToolbox and Content.Shared. The editor only reads it. It never fetches, pulls or changes anything there." }
                if let Some(message) = checkout_message {
                    NoteBox { message }
                }
                div { class: "start-actions",
                    button { class: "start-button primary", disabled: busy,
                        onclick: move |_| {
                            with_start(start::ask_folder);
                            bump();
                        },
                        if adding { "Waiting for the folder dialog..." } else { "Choose folder..." }
                    }
                }
            }
            div { class: url_class,
                div { class: "start-card-top",
                    div { class: "start-icon", DownloadMark {} }
                    div { class: "start-card-title",
                        div { class: "start-name", "Download from a URL" }
                        div { class: "start-muted", "Any fork's git repository" }
                    }
                }
                div { class: "start-fields",
                    div { class: "start-field-label", "GIT URL" }
                    TextField { value: url.clone(), placeholder: "https://github.com/owner/space-station-14", class: "start-input", disabled: busy,
                        oninput: move |text: String| {
                            with_start(|start| start.form.url = text);
                            bump();
                        },
                        onenter: move |_| {
                            with_start(start::begin_form_download);
                            bump();
                        },
                    }
                    div { class: "start-field-label", "BRANCH" }
                    TextField { value: branch.clone(), placeholder: "Leave empty for the default branch", class: "start-input", disabled: busy,
                        oninput: move |text: String| {
                            with_start(|start| start.form.branch = text);
                            bump();
                        },
                        onenter: move |_| {
                            with_start(start::begin_form_download);
                            bump();
                        },
                    }
                    div { class: "start-path",
                        if url.trim().is_empty() { "Cloned with its engine into the editor's own folder. Press Enter to start." } else { "Saves to {folder}" }
                    }
                }
                match git {
                    Some(false) => rsx! { div { class: "start-note error", div { class: "start-note-line first", "{download::NO_GIT}" } } },
                    None => rsx! { div { class: "start-muted", "Looking for git..." } },
                    Some(true) => rsx! {},
                }
                if let Some(job) = url_job {
                    JobPanel { job }
                } else if let Some(message) = url_message {
                    NoteBox { message }
                }
                div { class: "start-actions",
                    button { class: "start-button primary", disabled: !can_download,
                        onclick: move |_| {
                            with_start(start::begin_form_download);
                            bump();
                        },
                        "Download"
                    }
                }
            }
        }
    }
}

#[component]
fn MapLine(row: MapRow, busy: bool) -> Element {
    let MapRow { map, fork, fork_name, badge } = row;
    let folder = folder_of(&map);
    rsx! {
        div { class: if busy { "start-map disabled" } else { "start-map" },
            onclick: move |_| {
                if !busy {
                    with_start(|start| start::begin_check(start, fork.clone(), Some(map.clone())));
                    bump();
                }
            },
            Badge { badge, size: 22 }
            span { class: "start-map-name", "{file_name(&map)}" }
            span { class: "start-map-folder", "{folder}" }
            span { class: "start-map-fork", "{fork_name}" }
            span { class: "start-map-go", Triangle { towards: "right", size: 4 } }
        }
    }
}
