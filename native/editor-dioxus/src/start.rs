//! The start screen's state and actions. It lists the forks the editor knows, with their branches
//! and last maps, the maps opened most recently, and the popular forks offered as one-click
//! downloads. A mapper can add their own fork in two ways: a checkout they already have, or a
//! download from a URL. With no fork known yet, it is a short first-run setup. It shows when the
//! command line gives no fork, and again after Map > Switch fork.... `start_view.rs` is how it
//! looks.
//!
//! A download is a git clone into the editor's own forks folder. It runs on a thread, with its
//! progress and a Cancel button in the card it was started from (`forks::download`). Forks the
//! editor downloaded have Update, and their Remove asks whether to delete the files too. A
//! mapper's own checkout is only ever read.
//!
//! Opening a fork checks it on its own thread (`forks::check`). A refusal stays on the fork's card
//! with its reason. If the fork's code isn't what the editor's engine side was built from, it opens
//! with an engine side built for its code. That comes from the cache, or is built here first,
//! which takes minutes and shows its stages and a Cancel button (`forks::engines`). No engine is
//! running yet on this screen, so the session simply starts on that one. A failed build stays on
//! the card with its first errors. Otherwise the editor takes over the window, and the session
//! makes any pack and textures the cache is missing on the loading screen, before the engine
//! starts.
//!
//! Cards show the avatar of the fork's GitHub owner, fetched once on a thread and kept
//! (`forks::avatars`). Until it arrives, or when offline, a monogram on the fork's colour stands
//! in.
//!
//! A process opens only one fork. The engine is the .NET runtime hosted in this process, and a
//! runtime can't be started twice in one process, nor can the engine's state be cleared. So
//! Switch fork starts a new editor process with just the engine side, which shows this screen,
//! and closes this one (`switch_fork`).
//!
//! To see states that are hard to reach, set `SS14_EDITOR_START_DEMO` to `jobs`, `errors` or both
//! (`jobs,errors`). The screen then fakes them when it is made (`demo`). Point `LOCALAPPDATA` at a
//! folder of its own too, so the real settings aren't touched.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use base64::Engine as _;
use editor_core::files::forward_slashes;
use editor_core::forks::avatars;
use editor_core::forks::cache::{Cache, Prepared};
use editor_core::forks::download::{self, Download, Failure, Git, Progress, Stopper};
use editor_core::forks::engines::{self, elapsed, BuildJob, BuildProgress, Engine};
use editor_core::forks::popular::{self, PopularFork, POPULAR_FORKS};
use editor_core::forks::settings::Settings;
use editor_core::forks::{self, Opening};
use editor_core::session::Session;
use editor_core::shell::ShellArgs;
use editor_core::textures::BakedTextures;

use crate::{set_editor, wake, Editor};

pub(crate) use crate::start_view::StartScreen;

thread_local! {
    static START: RefCell<Option<Start>> = const { RefCell::new(None) };
}

/// Whether something runs whose time on the screen goes up each second.
static TICKING: AtomicBool = AtomicBool::new(false);

/// The start screen's state, while it shows.
pub(crate) struct Start {
    engine_folder: PathBuf,
    pub(crate) settings: Settings,
    /// Each known fork's branch, by its path, read when the screen was made or the fork added.
    pub(crate) branches: HashMap<String, Option<String>>,
    /// Each known fork's `origin` URL, by its path, for its avatar and colour.
    pub(crate) origins: HashMap<String, Option<String>>,
    /// Known forks whose folder is not there.
    pub(crate) missing: HashSet<String>,
    /// What the screen says, each in the card it belongs to.
    pub(crate) messages: Vec<Message>,
    pub(crate) checking: Option<Checking>,
    /// The folder dialog of Use a checkout I have, while it is open.
    pub(crate) adding: Option<Receiver<Option<PathBuf>>>,
    /// The URL and branch typed in Download from a URL.
    pub(crate) form: Form,
    /// Whether git can be run, None until asked.
    pub(crate) git: Option<bool>,
    git_answer: Option<Receiver<bool>>,
    /// A download, update or deletion running on a thread of its own.
    pub(crate) job: Option<Job>,
    /// The downloaded fork whose Remove asks whether to delete its files too.
    pub(crate) removing: Option<String>,
    /// An engine side being built for the fork being opened.
    pub(crate) building: Option<Building>,
    /// The repository chosen in a popular fork's card that has more than one, by its name.
    pub(crate) choices: HashMap<&'static str, usize>,
    /// The owners' avatars as PNG data addresses, by owner in small letters.
    pub(crate) avatars: HashMap<String, String>,
    avatars_asked: HashSet<String>,
    avatars_found: (Sender<(String, Option<String>)>, Receiver<(String, Option<String>)>),
    /// Jobs faked for a look at the screen (`demo`).
    pub(crate) demo_jobs: Vec<(Place, JobView)>,
}

/// The card a message or a job shows in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Place {
    /// Over the sections, for what belongs to no card.
    Top,
    /// A known fork's card, by its path.
    Fork(String),
    /// A popular fork's card, by its name.
    Popular(&'static str),
    /// Use a checkout I have.
    Checkout,
    /// Download from a URL.
    Url,
}

/// Words in a card, and whether they tell of something refused or failed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    pub(crate) place: Place,
    pub(crate) text: String,
    pub(crate) refused: bool,
    /// All its lines shown, not the first few.
    pub(crate) expanded: bool,
}

/// What runs, as a card shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JobView {
    pub(crate) title: String,
    pub(crate) stage: String,
    pub(crate) percent: Option<u32>,
    /// As "40 s".
    pub(crate) time: String,
    pub(crate) can_stop: bool,
    pub(crate) stopping: bool,
}

/// An engine side built on a thread of its own, for a fork that passed its check, which opens
/// once it is built.
pub(crate) struct Building {
    fork: String,
    name: String,
    map: Option<String>,
    prepared: Prepared,
    warning: Option<String>,
    progress: Arc<Mutex<BuildProgress>>,
    stopper: Stopper,
    stopping: bool,
    answer: Receiver<Result<PathBuf, Failure>>,
}

/// What Download from a URL holds: the URL and the branch as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Form {
    pub(crate) url: String,
    pub(crate) branch: String,
}

/// Work with git or on the disk, on a thread of its own.
pub(crate) struct Job {
    kind: JobKind,
    place: Place,
    /// The fork's name, for the words on the screen.
    name: String,
    progress: Arc<Mutex<Progress>>,
    stopper: Stopper,
    /// Cancel was pressed, and the thread has not ended yet.
    stopping: bool,
    started: Instant,
    answer: Receiver<Result<String, Failure>>,
}

#[derive(Clone)]
enum JobKind {
    Download { folder: PathBuf, url: String },
    Update { fork: String },
    Delete { fork: String },
}

/// A fork being checked before it opens.
pub(crate) struct Checking {
    fork: String,
    name: String,
    map: Option<String>,
    started: Instant,
    answer: Receiver<Result<Opening, String>>,
}

pub(crate) fn with_start<R>(f: impl FnOnce(&mut Start) -> R) -> Option<R> {
    START.with(|start| start.borrow_mut().as_mut().map(f))
}

/// Shows the start screen, with words over the sections if a fork was refused.
pub(crate) fn show(engine_folder: PathBuf, refused: Option<String>) {
    let settings = Settings::load(&Settings::file());
    let mut start = Start {
        engine_folder,
        settings,
        branches: HashMap::new(),
        origins: HashMap::new(),
        missing: HashSet::new(),
        messages: Vec::new(),
        checking: None,
        adding: None,
        form: Form::default(),
        git: None,
        git_answer: None,
        job: None,
        removing: None,
        building: None,
        choices: HashMap::new(),
        avatars: HashMap::new(),
        avatars_asked: HashSet::new(),
        avatars_found: mpsc::channel(),
        demo_jobs: Vec::new(),
    };
    for path in start.settings.forks.iter().map(|fork| fork.path.clone()).collect::<Vec<_>>() {
        read_fork(&mut start, &path);
    }
    if let Some(why) = refused {
        say(&mut start, Place::Top, why, true);
    }
    ask_git(&mut start);
    fetch_avatars(&mut start);
    if let Ok(demo) = std::env::var("SS14_EDITOR_START_DEMO") {
        self::demo(&mut start, &demo);
    }
    start_ticker();
    START.with(|cell| *cell.borrow_mut() = Some(start));
}

/// Reads what a fork's card shows of its folder: its branch, its origin and whether it is there.
fn read_fork(start: &mut Start, path: &str) {
    let folder = Path::new(path);
    start.branches.insert(path.to_owned(), forks::branch(folder));
    start.origins.insert(path.to_owned(), forks::origin_url(folder));
    if folder.is_dir() {
        start.missing.remove(path);
    } else {
        start.missing.insert(path.to_owned());
    }
}

/// Puts words in a card, in place of what it said before.
fn say(start: &mut Start, place: Place, text: String, refused: bool) {
    start.messages.retain(|message| message.place != place);
    start.messages.push(Message { place, text, refused, expanded: false });
}

/// Takes the words out of a card.
pub(crate) fn dismiss(start: &mut Start, place: &Place) {
    start.messages.retain(|message| &message.place != place);
}

/// Shows all the lines of a card's words, or the first few again.
pub(crate) fn expand(start: &mut Start, place: &Place) {
    if let Some(message) = start.messages.iter_mut().find(|message| &message.place == place) {
        message.expanded = !message.expanded;
    }
}

/// Wakes the screen each second while something runs, so that its time goes up when git and
/// dotnet are quiet. One thread for the life of the process.
fn start_ticker() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("start screen ticker".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_secs(1));
            if TICKING.load(Ordering::Relaxed) {
                wake();
            }
        });
    });
}

/// Asks once, on a thread, whether git can be run.
fn ask_git(start: &mut Start) {
    let (send, answer) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("git check".into()).spawn(move || {
        let _ = send.send(download::git_installed());
        wake();
    });
    if spawned.is_ok() {
        start.git_answer = Some(answer);
    }
}

/// The URL a known fork came from: its download's, else its checkout's origin.
pub(crate) fn url_of(start: &Start, path: &str) -> Option<String> {
    let known = start.settings.fork(path);
    known.and_then(|known| known.url.clone()).or_else(|| start.origins.get(path).cloned().flatten())
}

/// Fetches on a thread the avatars of the owners the cards show that were not asked for yet:
/// the popular forks' and the known forks' on GitHub.
fn fetch_avatars(start: &mut Start) {
    let mut owners: Vec<String> = POPULAR_FORKS.iter().flat_map(|fork| fork.repositories.iter().filter_map(|repository| popular::github_owner(repository.url))).collect();
    let paths: Vec<String> = start.settings.forks.iter().map(|fork| fork.path.clone()).collect();
    owners.extend(paths.iter().filter_map(|path| url_of(start, path)).filter_map(|url| popular::github_owner(&url)));
    let wanted: Vec<String> = owners.into_iter().filter(|owner| start.avatars_asked.insert(owner.to_lowercase())).collect();
    if wanted.is_empty() {
        return;
    }
    // One thread each, a few at most: GitHub takes a second or two for each.
    for owner in wanted {
        let send = start.avatars_found.0.clone();
        let _ = std::thread::Builder::new().name("avatar".into()).spawn(move || {
            let picture = avatars::avatar(&avatars::avatars_folder(), &owner).map(|bytes| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)));
            if send.send((owner.to_lowercase(), picture)).is_ok() {
                wake();
            }
        });
    }
}

/// The name and branch of a fork, as the menu bar shows them.
fn label(name: &str, branch: Option<&str>) -> String {
    match branch {
        Some(branch) => format!("{name} ({branch})"),
        None => name.to_owned(),
    }
}

/// Opens the editor on the fork the command line gave, with the pack and textures it gave,
/// as the start scripts do with a map. The fork is noted in the settings, so that the start
/// screen lists it later.
pub(crate) fn open_given(args: &ShellArgs, notice: Option<String>) {
    let file = Settings::file();
    let fork = Path::new(&args.fork_repo);
    let engine_key = engines::key_of(&Cache::standard(), &args.engine_folder);
    let noted = |settings: &mut Settings| {
        settings.used_fork(fork, None, None);
        settings.used_engine(fork, engine_key.clone());
    };
    let settings = Settings::update(&file, noted).unwrap_or_else(|_| {
        let mut settings = Settings::load(&file);
        noted(&mut settings);
        settings
    });
    let known = settings.forks[0].clone();
    let textures = Arc::new(args.open_textures());
    let mut session = args.start_session(wake);
    session.set_textures(textures.clone());
    session.set_maps_folder(known.maps_folder());
    let fork_label = label(&known.name, forks::branch(fork).as_deref());
    set_editor(Editor::new(session, textures, args.home_engine.clone(), fork_label, notice));
}

/// Takes what the screen's threads found: a folder Use a checkout I have chose, the end of a
/// download, update or deletion, avatars, git's answer, and the end of a check.
/// True once a fork that passed its check has opened the editor in the screen's place.
pub(crate) fn poll() -> bool {
    let Some(opened) = START.with(|cell| {
        let mut cell = cell.borrow_mut();
        let start = cell.as_mut()?;
        poll_adding(start);
        poll_job(start);
        poll_found(start);
        let ticking = start.job.is_some() || start.checking.is_some() || start.building.is_some();
        TICKING.store(ticking, Ordering::Relaxed);
        if let Some(built) = poll_building(start) {
            let start = cell.take()?;
            return Some((start, built));
        }
        let checking = start.checking.take()?;
        let place = Place::Fork(checking.fork.clone());
        let opening = match checking.answer.try_recv() {
            Err(TryRecvError::Empty) => {
                start.checking = Some(checking);
                return None;
            }
            Err(TryRecvError::Disconnected) => {
                say(start, place, format!("Checking {} stopped before it finished.", checking.name), true);
                return None;
            }
            Ok(Err(refused)) => {
                say(start, place, refused, true);
                return None;
            }
            Ok(Ok(opening)) => opening,
        };
        let Opening { prepared, warning, engine } = opening;
        let ready = Ready { fork: checking.fork, map: checking.map, prepared, warning, engine: PathBuf::new() };
        match engine {
            Engine::Ready(folder) => Some((cell.take()?, Ready { engine: folder, ..ready })),
            Engine::Build(job) => {
                begin_build(start, *job, ready);
                None
            }
        }
    }) else {
        return false;
    };
    TICKING.store(false, Ordering::Relaxed);
    let (start, ready) = opened;
    open_ready(start, ready);
    true
}

/// Takes the avatars fetched and git's answer.
fn poll_found(start: &mut Start) {
    while let Ok((owner, picture)) = start.avatars_found.1.try_recv() {
        if let Some(picture) = picture {
            start.avatars.insert(owner, picture);
        }
    }
    if let Some(answer) = start.git_answer.take() {
        match answer.try_recv() {
            Ok(git) => start.git = Some(git),
            Err(TryRecvError::Empty) => start.git_answer = Some(answer),
            Err(TryRecvError::Disconnected) => start.git = Some(false),
        }
    }
}

/// A fork ready to open: its pack and textures, a warning, and the engine side it opens with.
struct Ready {
    fork: String,
    map: Option<String>,
    prepared: Prepared,
    warning: Option<String>,
    engine: PathBuf,
}

/// Builds the engine side for a fork on a thread of its own, with its stages shown.
fn begin_build(start: &mut Start, job: BuildJob, ready: Ready) {
    let progress = Arc::new(Mutex::new(BuildProgress::new()));
    let stopper = Stopper::default();
    let (send, answer) = mpsc::channel();
    let (shown, stop) = (progress.clone(), stopper.clone());
    let name = job.name.clone();
    let spawned = std::thread::Builder::new().name("engine build".into()).spawn(move || {
        let mut woken = Instant::now();
        let built = engines::build(&Git::default(), &job, &stop, &mut |now| {
            if let Ok(mut shown) = shown.lock() {
                *shown = now.clone();
            }
            if woken.elapsed() > Duration::from_millis(100) {
                woken = Instant::now();
                wake();
            }
        });
        let _ = send.send(built);
        wake();
    });
    let place = Place::Fork(ready.fork.clone());
    match spawned {
        Ok(_) => {
            dismiss(start, &place);
            start.building = Some(Building { fork: ready.fork, name, map: ready.map, prepared: ready.prepared, warning: ready.warning, progress, stopper, stopping: false, answer });
        }
        Err(error) => say(start, place, format!("Couldn't start building the engine side: {error}"), true),
    }
}

/// Takes the end of an engine build: the fork ready to open, or the failure in its card.
fn poll_building(start: &mut Start) -> Option<Ready> {
    let building = start.building.take()?;
    let built = match building.answer.try_recv() {
        Err(TryRecvError::Empty) => {
            start.building = Some(building);
            return None;
        }
        Err(TryRecvError::Disconnected) => Err(Failure::Failed("It stopped before it finished.".to_owned())),
        Ok(built) => built,
    };
    let place = Place::Fork(building.fork.clone());
    match built {
        Ok(engine) => Some(Ready { fork: building.fork, map: building.map, prepared: building.prepared, warning: building.warning, engine }),
        Err(Failure::Stopped) => {
            say(start, place, format!("Building the engine side for {} was stopped. Open the fork again to restart the build.", building.name), false);
            None
        }
        Err(Failure::Failed(why)) => {
            say(start, place, why, true);
            None
        }
    }
}

fn poll_adding(start: &mut Start) {
    let Some(adding) = start.adding.take() else { return };
    match adding.try_recv() {
        Err(TryRecvError::Empty) => start.adding = Some(adding),
        Err(TryRecvError::Disconnected) | Ok(None) => {}
        Ok(Some(folder)) => {
            if let Some(why) = forks::not_a_fork(&folder) {
                say(start, Place::Checkout, why, true);
                return;
            }
            dismiss(start, &Place::Checkout);
            let changed = change(start, |settings| {
                settings.add_fork(&folder);
            });
            read_fork(start, &forward_slashes(&folder));
            fetch_avatars(start);
            if let Err(why) = changed {
                say(start, Place::Top, why, true);
            }
        }
    }
}

/// Takes the end of a download, update or deletion.
fn poll_job(start: &mut Start) {
    let Some(job) = start.job.take() else { return };
    let ended = match job.answer.try_recv() {
        Err(TryRecvError::Empty) => {
            start.job = Some(job);
            return;
        }
        Err(TryRecvError::Disconnected) => Err(Failure::Failed("It stopped before it finished.".to_owned())),
        Ok(ended) => ended,
    };
    let (name, place) = (job.name, job.place);
    match (job.kind, ended) {
        (JobKind::Download { folder, url }, Ok(_)) => {
            let changed = change(start, |settings| {
                settings.add_downloaded(&folder, &url, &name);
            });
            let path = forward_slashes(&folder);
            read_fork(start, &path);
            fetch_avatars(start);
            if place == Place::Url {
                start.form = Form::default();
            }
            match changed {
                Ok(()) => say(start, Place::Fork(path),"Downloaded. The first open packs its prototypes and bakes its textures, which takes up to a minute.".to_owned(), false),
                Err(why) => say(start, place, why, true),
            }
        }
        (JobKind::Download { .. }, Err(Failure::Stopped)) => say(start, place, format!("The download of {name} was stopped and the partial download deleted."), false),
        (JobKind::Download { .. }, Err(Failure::Failed(why))) => say(start, place, format!("{name} could not be downloaded. {why}"), true),
        (JobKind::Update { fork }, Ok(said)) => {
            read_fork(start, &fork);
            say(start, place, capitalised(&said), false);
        }
        (JobKind::Update { .. }, Err(Failure::Stopped)) => say(start, place, "The update stopped partway. Run Update again to finish it.".to_owned(), false),
        (JobKind::Update { .. }, Err(Failure::Failed(why))) => say(start, place, format!("Update failed. {why}"), true),
        (JobKind::Delete { fork }, Ok(_)) => {
            let changed = change(start, |settings| settings.remove_fork(&fork));
            dismiss(start, &place);
            match changed {
                Ok(()) => say(start, Place::Top, format!("Removed {name} and deleted its files."), false),
                Err(why) => say(start, Place::Top, why, true),
            }
        }
        (JobKind::Delete { .. }, Err(failure)) => say(start, place, format!("Some of its files couldn't be deleted, so it stays in the list. {}", failure.message()), true),
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether anything runs that the other buttons wait for.
pub(crate) fn busy(start: &Start) -> bool {
    start.job.is_some() || start.checking.is_some() || start.building.is_some() || start.adding.is_some() || !start.demo_jobs.is_empty()
}

/// Runs work on a thread of its own, with what git says shown as it comes.
fn begin_job(start: &mut Start, kind: JobKind, place: Place, name: String, doing: &str, work: impl FnOnce(&Stopper, &mut dyn FnMut(&Progress)) -> Result<String, Failure> + Send + 'static) {
    if busy(start) {
        return;
    }
    let progress = Arc::new(Mutex::new(Progress::new(doing)));
    let stopper = Stopper::default();
    let (send, answer) = mpsc::channel();
    let (shown, stop) = (progress.clone(), stopper.clone());
    let spawned = std::thread::Builder::new().name("fork download".into()).spawn(move || {
        // Git writes its progress many times a second. The screen is woken a few times a second.
        let mut woken = Instant::now();
        let ended = work(&stop, &mut |now| {
            if let Ok(mut shown) = shown.lock() {
                *shown = now.clone();
            }
            if woken.elapsed() > Duration::from_millis(100) {
                woken = Instant::now();
                wake();
            }
        });
        let _ = send.send(ended);
        wake();
    });
    match spawned {
        Ok(_) => {
            dismiss(start, &place);
            start.removing = None;
            start.job = Some(Job { kind, place, name, progress, stopper, stopping: false, started: Instant::now(), answer });
        }
        Err(error) => say(start, place, format!("Couldn't start: {error}"), true),
    }
}

/// Downloads a popular fork's chosen repository.
pub(crate) fn begin_popular(start: &mut Start, fork: &'static PopularFork) {
    let at = start.choices.get(fork.name).copied().unwrap_or(0).min(fork.repositories.len() - 1);
    begin_download(start, fork.repositories[at].url.to_owned(), String::new(), Place::Popular(fork.name));
}

/// Download from a URL: the form's URL and branch.
pub(crate) fn begin_form_download(start: &mut Start) {
    let Form { url, branch } = start.form.clone();
    begin_download(start, url, branch, Place::Url);
}

/// A URL and branch cloned into the editor's forks folder, its progress in the card it was
/// started from.
fn begin_download(start: &mut Start, url: String, branch: String, place: Place) {
    if busy(start) {
        return;
    }
    if let Some(why) = download::not_a_url(&url, &branch) {
        say(start, place, why, true);
        return;
    }
    start.git = Some(download::git_installed());
    if start.git == Some(false) {
        say(start, place, download::NO_GIT.to_owned(), true);
        return;
    }
    let root = download::forks_folder();
    if let Some(known) = start.settings.fork(&forward_slashes(&Download::first_folder(&root, &url, &branch))) {
        let text = format!("This fork and branch are already downloaded, as {} in Your forks. Use its Update button to bring it up to date.", known.name);
        say(start, place, text, true);
        return;
    }
    let job = Download::new(root, &url, &branch);
    let name = download::display_name(&url);
    let kind = JobKind::Download { folder: job.folder(), url: job.url.clone() };
    begin_job(start, kind, place, name, "Downloading", move |stopper, report| download::download(&Git::default(), &job, stopper, report).map(|()| String::new()));
}

/// A known fork the editor downloaded, inside its own forks folder, or None. Only these are
/// updated or deleted.
fn downloaded(start: &Start, fork: &str) -> Option<String> {
    start.settings.fork(fork).filter(|known| known.managed && download::is_in(&download::forks_folder(), Path::new(&known.path))).map(|known| known.name.clone())
}

/// Update: fetches a downloaded fork and moves it forward. Never a mapper's own checkout.
pub(crate) fn begin_update(start: &mut Start, fork: String) {
    let Some(name) = downloaded(start, &fork) else { return };
    let folder = PathBuf::from(&fork);
    let place = Place::Fork(fork.clone());
    begin_job(start, JobKind::Update { fork }, place, name, "Fetching", move |stopper, report| download::update(&Git::default(), &folder, stopper, report));
}

/// Removes a downloaded fork from the list and deletes its files.
pub(crate) fn begin_delete(start: &mut Start, fork: String) {
    let Some(name) = downloaded(start, &fork) else { return };
    let folder = PathBuf::from(&fork);
    let place = Place::Fork(fork.clone());
    begin_job(start, JobKind::Delete { fork }, place, name, "Deleting the files", move |_, _| download::remove_tree(&folder).map(|()| String::new()).map_err(Failure::Failed));
}

/// Remove: a mapper's own checkout goes off the list at once. A download asks first whether
/// its files go too.
pub(crate) fn ask_remove(start: &mut Start, fork: &str) {
    match start.settings.fork(fork).is_some_and(|known| known.managed) {
        true => start.removing = Some(fork.to_owned()),
        false => remove_from_list(start, fork),
    }
}

/// Takes a fork off the list. Its files are not touched.
pub(crate) fn remove_from_list(start: &mut Start, fork: &str) {
    let changed = change(start, |settings| settings.remove_fork(fork));
    start.removing = None;
    dismiss(start, &Place::Fork(fork.to_owned()));
    if let Err(why) = changed {
        say(start, Place::Top, why, true);
    }
}

/// Cancel: stops the download, update or build that runs.
pub(crate) fn stop(start: &mut Start) {
    if let Some(job) = start.job.as_mut() {
        job.stopping = true;
        job.stopper.stop();
    } else if let Some(building) = start.building.as_mut() {
        building.stopping = true;
        building.stopper.stop();
    }
    // A faked one goes when its Cancel is pressed.
    start.demo_jobs.clear();
}

/// Changes the settings as the file has them now, not the copy read when the screen opened:
/// maps and favourites noted since, here or by another window of the editor, are kept. The
/// screen's copy becomes what was saved, or takes the change alone if saving failed.
fn change(start: &mut Start, apply: impl Fn(&mut Settings)) -> Result<(), String> {
    match Settings::update(&Settings::file(), &apply) {
        Ok(saved) => {
            start.settings = saved;
            Ok(())
        }
        Err(why) => {
            apply(&mut start.settings);
            Err(format!("Couldn't save the settings: {why}"))
        }
    }
}

/// A fork passed its check and has its engine side: the settings note it, the cache lets go of
/// what no known fork uses, and the editor starts on it in the screen's place.
fn open_ready(mut start: Start, ready: Ready) {
    let Ready { fork: fork_path, map, prepared, warning, engine } = ready;
    let fork = Path::new(&fork_path);
    let cache = Cache::standard();
    let engine_key = engines::key_of(&cache, &engine);
    let _ = change(&mut start, |settings| {
        settings.used_fork(fork, Some(prepared.prototypes_key.clone()), Some(prepared.textures_key.clone()));
        settings.used_engine(fork, engine_key.clone());
    });
    cache.prune(&start.settings.cached_keys());
    let known = start.settings.forks[0].clone();
    let mut session = Session::start_prepared(engine, fork_path.clone(), prepared, map, wake);
    session.set_maps_folder(known.maps_folder());
    let branch = start.branches.get(&known.path).cloned().flatten();
    set_editor(Editor::new(session, Arc::new(BakedTextures::empty()), start.engine_folder, label(&known.name, branch.as_deref()), warning));
}

/// Checks a fork on a thread of its own, to open it with a map or an empty one.
pub(crate) fn begin_check(start: &mut Start, fork: String, map: Option<String>) {
    if busy(start) {
        return;
    }
    let known = start.settings.fork(&fork).cloned();
    let name = known.as_ref().map_or_else(|| forks::default_name(Path::new(&fork)), |known| known.name.clone());
    let last = known.map_or((None, None), |known| (known.prototypes, known.textures));
    let (send, answer) = mpsc::channel();
    let (engine, path, called) = (start.engine_folder.clone(), fork.clone(), name.clone());
    let spawned = std::thread::Builder::new().name("fork check".into()).spawn(move || {
        let _ = send.send(forks::check(Path::new(&path), &called, &engine, (last.0.as_deref(), last.1.as_deref()), &Cache::standard()));
        wake();
    });
    let place = Place::Fork(fork.clone());
    match spawned {
        Ok(_) => {
            dismiss(start, &place);
            start.removing = None;
            start.checking = Some(Checking { fork, name, map, started: Instant::now(), answer });
        }
        Err(error) => say(start, place, format!("Couldn't start the check: {error}"), true),
    }
}

/// Opens the system's folder dialog for Use a checkout I have, on a thread of its own.
pub(crate) fn ask_folder(start: &mut Start) {
    if start.adding.is_some() || busy(start) {
        return;
    }
    let begin = start.settings.forks.iter().find(|fork| !fork.managed).and_then(|fork| Path::new(&fork.path).parent().map(Path::to_path_buf));
    let (send, answer) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("folder dialog".into()).spawn(move || {
        let mut dialog = rfd::FileDialog::new().set_title("Choose a fork checkout folder");
        if let Some(begin) = begin {
            dialog = dialog.set_directory(begin);
        }
        let _ = send.send(dialog.pick_folder());
        wake();
    });
    if spawned.is_ok() {
        dismiss(start, &Place::Checkout);
        start.adding = Some(answer);
    }
}

/// What runs now, each in its card.
pub(crate) fn jobs(start: &Start) -> Vec<(Place, JobView)> {
    let mut jobs = start.demo_jobs.clone();
    if let Some(job) = &start.job {
        let progress = job.progress.lock().map(|progress| progress.clone()).unwrap_or_default();
        let title = match &job.kind {
            JobKind::Download { .. } => format!("Downloading {}", job.name),
            JobKind::Update { .. } => format!("Updating {}", job.name),
            JobKind::Delete { .. } => format!("Deleting {}'s files", job.name),
        };
        let stage = match (&job.kind, progress.step.is_empty()) {
            (JobKind::Delete { .. }, _) => "A large fork takes a minute.".to_owned(),
            (_, true) => "Starting git...".to_owned(),
            (_, false) => progress.text(),
        };
        let can_stop = !matches!(job.kind, JobKind::Delete { .. });
        jobs.push((job.place.clone(), JobView { title, stage, percent: progress.percent, time: elapsed(job.started.elapsed()), can_stop, stopping: job.stopping }));
    }
    if let Some(building) = &start.building {
        let progress = building.progress.lock().map(|progress| progress.clone()).unwrap_or_default();
        let view = JobView {
            title: "Building the engine side".to_owned(),
            stage: capitalised(&progress.doing(&building.name)),
            percent: progress.percent(),
            time: elapsed(progress.started.elapsed()),
            can_stop: true,
            stopping: building.stopping,
        };
        jobs.push((Place::Fork(building.fork.clone()), view));
    }
    if let Some(checking) = &start.checking {
        let view = JobView {
            title: format!("Checking {}", checking.name),
            stage: "Reading its code, prototypes and textures. This takes a few seconds.".to_owned(),
            percent: None,
            time: elapsed(checking.started.elapsed()),
            can_stop: false,
            stopping: false,
        };
        jobs.push((Place::Fork(checking.fork.clone()), view));
    }
    jobs
}

/// Fakes states for a look at the screen: `jobs` puts a download in the first popular card
/// and an engine build in the first fork's, `errors` puts failures in the second fork's card,
/// a popular card and the URL card, and a note in the first fork's.
fn demo(start: &mut Start, which: &str) {
    let first = start.settings.forks.first().map(|fork| fork.path.clone());
    let second = start.settings.forks.get(1).map(|fork| fork.path.clone());
    if which.contains("jobs") {
        let download = JobView {
            title: "Downloading Goob Station".to_owned(),
            stage: "Downloading RobustToolbox: receiving objects, 45%, 412.30 MiB | 9.80 MiB/s".to_owned(),
            percent: Some(45),
            time: "1 min".to_owned(),
            can_stop: true,
            stopping: false,
        };
        start.demo_jobs.push((Place::Popular("Goob Station"), download));
        if let Some(first) = &first {
            let build = JobView {
                title: "Building the engine side".to_owned(),
                stage: "Building, which takes minutes. 7 projects built".to_owned(),
                percent: None,
                time: "2 min".to_owned(),
                can_stop: true,
                stopping: false,
            };
            start.demo_jobs.push((Place::Fork(first.clone()), build));
        }
    }
    if which.contains("errors") {
        if let Some(second) = second {
            let text = "The engine side for Delta-v could not be built. dotnet publish failed:\nContent.Shared/Foo.cs(12,5): error CS0246: The type or namespace name 'Bar' could not be found\nContent.Shared/Foo.cs(30,9): error CS0103: The name 'Baz' does not exist in the current context\nContent.Server/Qux.cs(8,1): error CS1002: ; expected\nContent.Client/Quux.cs(44,17): error CS0117: 'Robust' does not contain a definition for 'Thing'\nContent.Client/Corge.cs(2,3): error CS0234: The type or namespace name 'Grault' does not exist\nThe whole log is in build.log beside it.";
            say(start, Place::Fork(second), text.to_owned(), true);
        }
        if let Some(first) = first.filter(|_| !which.contains("jobs")) {
            say(start, Place::Fork(first), "Already up to date.".to_owned(), false);
        }
        say(start, Place::Popular("Delta-V"), "Delta-V could not be downloaded. fatal: unable to access 'https://github.com/DeltaV-Station/Delta-v/': Could not resolve host: github.com".to_owned(), true);
        start.form = Form { url: "https://example.com/not-a-fork".to_owned(), branch: String::new() };
        say(start, Place::Url, "not-a-fork could not be downloaded. It has no Resources/Prototypes, RobustToolbox, Content.Shared, so it is not a fork of Space Station 14.".to_owned(), true);
    }
}

/// Notes the open map in the settings when it is a file not noted yet: opened, or saved under
/// a new name. It becomes the fork's last map and the first of the recent maps.
pub(crate) fn note_map(editor: &mut Editor) {
    let path = editor.session.doc.as_ref().and_then(|doc| doc.path.clone());
    if path == editor.recorded_map {
        return;
    }
    editor.recorded_map = path.clone();
    let Some(path) = path else { return };
    let fork = editor.session.fork_repo.clone();
    if let Err(why) = Settings::update(&Settings::file(), |settings| settings.opened_map(Path::new(&fork), &path)) {
        editor.notice = Some(format!("Couldn't add the map to recent maps: {why}"));
    }
}

/// Map > Switch fork...: a new process of the editor with the engine side alone shows the
/// start screen, and this one closes. The unsaved changes were asked about before.
pub(crate) fn switch_fork(editor: &mut Editor) {
    let started = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(&editor.engine_folder).spawn());
    match started {
        Ok(_) => editor.quitting = true,
        Err(error) => editor.notice = Some(format!("Couldn't restart the editor for the other fork: {error}")),
    }
}
