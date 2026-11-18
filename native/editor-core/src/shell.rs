//! What an interface needs besides the session: the command line, the web editor's colours and
//! the names of the layer switches.

use std::path::{Path, PathBuf};

use crate::forks::cache::Cache;
use crate::forks::download::{Git, Stopper};
use crate::forks::engines::{self, Engine, Sources, Stage};
use crate::forks::fingerprint::EngineStamp;
use crate::session::Session;
use crate::textures::BakedTextures;

/// `<engine folder> <fork repo> <kernel pack .zip> <textures file> [map .yml]`
pub struct ShellArgs {
    /// The engine side the session runs on: the one given, or one built for the fork's code
    /// (`choose_engine`).
    pub engine_folder: PathBuf,
    /// The engine side the command line gave, which the editor came with.
    pub home_engine: PathBuf,
    pub fork_repo: String,
    pub kernel_pack: String,
    pub textures_file: PathBuf,
    /// The map to open; without one the session starts with an empty map.
    pub map: Option<String>,
}

/// What the command line asks of an interface with a start screen.
pub enum Launch {
    /// A fork with its pack and textures, as the start scripts give them with a map.
    Fork(ShellArgs),
    /// The engine side alone: the start screen chooses the fork.
    StartScreen { engine_folder: PathBuf },
}

const USAGE: &str = "usage: <shell> <engine folder> <fork repo> <kernel pack .zip> <textures file> [map .yml]\n       <shell> --stamp-engine <engine folder> <fork repo>\n       <shell> --build-engine <engine folder> <fork repo>
       <shell> --bake <fork repo> <textures file>";

impl ShellArgs {
    /// Reads the command line, which must give a fork.
    pub fn parse() -> ShellArgs {
        match ShellArgs::parse_launch() {
            Launch::Fork(args) => args,
            Launch::StartScreen { .. } => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        }
    }

    /// Reads the command line: a fork, or the engine side alone for the start screen.
    pub fn parse_launch() -> Launch {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("--stamp-engine") {
            stamp_engine(&args[1..]);
        }
        if args.first().map(String::as_str) == Some("--build-engine") {
            build_engine(&args[1..]);
        }
        if args.first().map(String::as_str) == Some("--bake") {
            bake(&args[1..]);
        }
        match args.len() {
            1 => Launch::StartScreen { engine_folder: PathBuf::from(&args[0]) },
            4.. => Launch::Fork(ShellArgs {
                engine_folder: PathBuf::from(&args[0]),
                home_engine: PathBuf::from(&args[0]),
                fork_repo: args[1].clone(),
                kernel_pack: args[2].clone(),
                textures_file: PathBuf::from(&args[3]),
                map: args.get(4).cloned(),
            }),
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        }
    }

    /// The safety rule for the fork given, and the engine side it opens with: the one given, or
    /// one built for its code before, which the session then runs on. Refused with the reason,
    /// with how to build one when it needs one, or allowed with a warning or none.
    pub fn choose_engine(&mut self) -> Result<Option<String>, String> {
        let fork = Path::new(&self.fork_repo);
        let name = crate::forks::default_name(fork);
        match engines::choose(fork, &name, &self.home_engine, &Cache::standard(), Sources::locate(&self.home_engine))? {
            (Engine::Ready(folder), warning) => {
                self.engine_folder = folder;
                Ok(warning)
            }
            (Engine::Build(job), _) => {
                let exe = std::env::current_exe().map_or_else(|_| "the editor".to_owned(), |exe| exe.display().to_string());
                Err(format!(
                    "{} The editor can build an engine side for {name}'s code (it takes a few minutes) with: {exe} --build-engine {} {}",
                    job.reason,
                    self.home_engine.display(),
                    fork.display()
                ))
            }
        }
    }

    pub fn start_session(&self, wake: impl Fn() + Send + Sync + 'static) -> Session {
        Session::start(self.engine_folder.clone(), self.fork_repo.clone(), self.kernel_pack.clone(), self.map.clone(), wake)
    }

    pub fn open_textures(&self) -> BakedTextures {
        BakedTextures::open(&self.textures_file).unwrap_or_else(|e| panic!("{e}"))
    }
}

/// `--stamp-engine <engine folder> <fork repo>`: writes the code fingerprint of the checkout an
/// engine side was just published from beside it, and exits. The start scripts run it after
/// they publish.
fn stamp_engine(args: &[String]) -> ! {
    let [engine, fork] = args else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    match EngineStamp::write(Path::new(engine), Path::new(fork)) {
        Ok(stamp) => {
            println!("wrote the code fingerprint of {} beside {engine}", stamp.from);
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// `--bake <fork repo> <textures file>`: bakes every texture of a fork whose engine submodule is
/// set up into one file, as the start screen does for its cache, and exits. The start scripts
/// run it before they open a map by its path.
fn bake(args: &[String]) -> ! {
    let [fork, output] = args else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let output = Path::new(output);
    if let Some(folder) = output.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        if let Err(error) = std::fs::create_dir_all(folder) {
            eprintln!("couldn't create {}: {error}", folder.display());
            std::process::exit(1);
        }
    }
    match crate::textures::bake(Path::new(fork), output) {
        Ok(report) => {
            println!("baked {} images ({} failed) into {}, {:.1} MB", report.images, report.failed, output.display(), report.file_bytes as f64 / 1024.0 / 1024.0);
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("baking the textures failed: {error}");
            std::process::exit(1);
        }
    }
}

/// `--build-engine <engine folder> <fork repo>`: builds the engine side for the fork's code into
/// the cache, as the start screen does, saying each stage, and exits. Nothing is built when the
/// engine side given or one built before fits.
fn build_engine(args: &[String]) -> ! {
    let [home, fork] = args else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let (home, fork) = (Path::new(home), Path::new(fork));
    if let Some(why) = crate::forks::not_a_fork(fork) {
        eprintln!("{why}");
        std::process::exit(1);
    }
    let name = crate::forks::default_name(fork);
    let job = match engines::choose(fork, &name, home, &Cache::standard(), Sources::locate(home)) {
        Ok((Engine::Ready(folder), _)) => {
            println!("{name} already opens with the engine side in {}, so there's nothing to build.", folder.display());
            std::process::exit(0);
        }
        Ok((Engine::Build(job), _)) => job,
        Err(refused) => {
            eprintln!("{refused}");
            std::process::exit(1);
        }
    };
    println!("{}", job.reason);
    let begun = std::time::Instant::now();
    let mut last: Option<(Stage, std::time::Instant)> = None;
    let built = engines::build(&Git::default(), &job, &Stopper::default(), &mut |progress| {
        if last.is_none_or(|(stage, _)| stage != progress.stage) {
            if let Some((stage, since)) = last {
                println!("  {stage:?} took {:.1} s", since.elapsed().as_secs_f64());
            }
            last = Some((progress.stage, std::time::Instant::now()));
            println!("{}", progress.text(&name));
        }
    });
    match built {
        Ok(folder) => {
            if let Some((stage, since)) = last {
                println!("  {stage:?} took {:.1} s", since.elapsed().as_secs_f64());
            }
            println!("built the engine side for {name} in {:.1} s: {}", begun.elapsed().as_secs_f64(), folder.display());
            std::process::exit(0);
        }
        Err(failure) => {
            eprintln!("{}", failure.message());
            std::process::exit(1);
        }
    }
}

/// The web editor's colours that the map and its overlay use, as (red, green, blue), from its
/// `App.css`.
pub mod colours {
    /// Behind the map.
    pub const CANVAS: (u8, u8, u8) = (17, 17, 34);
    pub const ACCENT: (u8, u8, u8) = (68, 136, 255);
}

/// The layer switches in the order the web editor lists them, with Plumbing and Disposals,
/// which it has not, after its own.
pub const LAYER_NAMES: [&str; 10] = ["SubFloor", "Floor Obj", "Structures", "Objects", "Doors", "Markers", "Decals", "Plumbing", "Disposals", "T-Ray (SubFloor)"];

/// Where T-Ray is in [`LAYER_NAMES`]: the panels set it apart from the layers above it.
pub const T_RAY_LAYER: usize = 9;

pub fn layer(layers: &mut crate::session::Layers, index: usize) -> &mut bool {
    match index {
        0 => &mut layers.subfloor,
        1 => &mut layers.floor_objects,
        2 => &mut layers.structures,
        3 => &mut layers.objects,
        4 => &mut layers.doors,
        5 => &mut layers.markers,
        6 => &mut layers.decals,
        7 => &mut layers.plumbing,
        8 => &mut layers.disposals,
        _ => &mut layers.t_ray,
    }
}
