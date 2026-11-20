//! The engine's thread: what the session asks of it, what it answers, and the loop that does the work.

use std::collections::BTreeSet;

use super::*;
use crate::forks::cache;

pub(super) enum ToEngine {
    /// Open a map file, or with no path start an empty map.
    Open { path: Option<String> },
    Edit { ops: Vec<Value>, label: &'static str, stroke: Option<u64>, coalesce: bool, seq: u64, asked: Instant },
    Undo { seq: u64, asked: Instant },
    Redo { seq: u64, asked: Instant },
    Save { path: String },
    SubFloor { show: bool, asked: Instant },
    Previews { items: Vec<(i32, String)>, directions: bool },
    Details { uid: i32 },
    /// The data fields of an entity's components, for their forms. A question, and not an edit.
    Fields { uid: i32 },
    /// The containers of an entity and what each holds. A question, and not an edit.
    Containers { uid: i32 },
    /// A copy of entities as the game's own text. A question, and not an edit.
    Copy { uids: Vec<i32>, seq: u64 },
    /// The ports entities have now. A question, and not an edit.
    PortsOf { uids: Vec<i32> },
    /// Sets one component of an entity, or with none takes it away, keeping the rest of its
    /// data as the engine has it: an edit of one undo step.
    SetComponent { uid: i32, kind: &'static str, component: Option<Value>, label: &'static str, seq: u64, asked: Instant },
    /// The ids of the fork's backdrops. A question, and not an edit.
    BackdropNames,
    /// One of the fork's backdrops for the gallery, painted on a thread of its own one at a
    /// time in the order asked (`PaintAsk`).
    PaintBackdrop(PaintAsk),
    /// What a fix of the air of each grid of the open map would change. A question, and not an edit.
    AirFixPreview,
}

/// One of the fork's backdrops the gallery wants: its picture, and with `whole` the backdrop
/// itself, to show behind the map.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PaintAsk {
    pub name: String,
    pub whole: bool,
}

pub(super) enum FromEngine {
    Progress(String),
    /// A stage of loading finished on the engine's thread, for the loading screen.
    Reached(Step),
    /// The fork's baked textures, made or found in the cache, before the engine starts.
    Textures(Arc<BakedTextures>),
    /// A map would not open, with the engine's error. The open document stays.
    NotOpened(String),
    /// The engine is up, with what it tells once: the fork's tiles, prototypes, decals and
    /// ports, and the layer of each of its draw depths.
    Ready { document_kernel_ms: u64, view_kernel_ms: u64, warm_up_ms: u64, tiles: Vec<Tile>, prototypes: Vec<Prototype>, decal_prototypes: Vec<DecalPrototype>, ports: Ports, depths: DepthGroups },
    /// A map opened, with its links as the engine listed them, or why it would not.
    Opened { scene: Box<SceneJson>, list: Vec<u8>, timing: EngineTiming, path: Option<String>, links: Result<LinksAnswer, String> },
    Changed { list: Option<Vec<u8>>, delta: Option<Box<Delta>>, what: &'static str, seq: u64, asked: Instant, timing: Timing },
    /// The engine would not do it (an edit that makes no sense there). The session goes on.
    Refused { seq: u64, what: &'static str, error: String },
    Saved { path: String, ok: bool, failure: Option<String>, milliseconds: f64, bytes: u64, revision: i32 },
    Previews { list: Option<Vec<u8>>, keys: Vec<i32>, directions: bool },
    Details { uid: i32, components: Value },
    /// The data fields of an entity's components, or why the engine would not tell them.
    Fields { uid: i32, answer: Result<FieldsAnswer, String> },
    /// An entity's containers, or why the engine would not tell them.
    Containers { uid: i32, containers: Result<Vec<Container>, String> },
    /// The answer to a copy: the text, how many entities are inside none of the others, how
    /// many in all, and the cell they are measured from. A refused copy is `Refused`.
    Copied { seq: u64, text: String, roots: usize, entities: usize, from: (i32, i32) },
    /// The ports of the entities asked about, or why the engine would not tell them.
    PortsOf { uids: Vec<i32>, ports: Result<Vec<EntityPorts>, String> },
    /// A document's backdrop, read on a thread of its own after the document was opened and
    /// after a change to it.
    Backdrop { document: i32, backdrop: Result<Backdrop, String> },
    /// The ids of the fork's backdrops, or why the engine would not tell them.
    BackdropNames(Result<Vec<String>, String>),
    /// One of the fork's backdrops painted for the gallery: its picture, and the backdrop
    /// itself if it was asked for whole.
    BackdropPainted { name: String, thumb: Result<Thumb, String>, whole: Option<Backdrop> },
    /// The air a document's grids store: all of them after it opened, and those a change
    /// touched after the change (`asked`). Or why the engine would not tell it.
    Air { document: i32, asked: Option<Vec<i32>>, answer: Result<AirAnswer, String> },
    /// What a fix of the air of each grid of a document would change, or why the engine would not tell.
    AirFixPreview { document: i32, previews: Result<Vec<super::FixPreview>, String> },
    Failed(String),
}

/// Where the time of one change went, in milliseconds, up to the engine's answer being ready.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    /// Waiting for the engine's thread to be free (it does one thing at a time).
    pub queued: f64,
    /// The engine's own work, of which `view` is the client world following the change and
    /// settling, and `record` is writing down what it now draws.
    pub engine: f64,
    pub view: f64,
    pub record: f64,
    /// When the answer was ready.
    pub(super) ready: Option<Instant>,
}

#[derive(Deserialize)]
struct Opened {
    scene: SceneJson,
    timing: EngineTiming,
}

#[derive(Deserialize)]
struct Edited {
    delta: Delta,
    timing: EngineTiming,
}

#[derive(Deserialize)]
struct Layered {
    timing: EngineTiming,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    ok: bool,
    failure: Option<String>,
    milliseconds: f64,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TileJson {
    id: String,
    sprite: Option<String>,
    /// An older engine does not say, and the tile is then one that cannot be turned.
    #[serde(default)]
    allows_rotation: bool,
    /// An older engine does not say, and the tile is then one a crowbar cannot lift.
    #[serde(default)]
    can_crowbar: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopiedJson {
    text: String,
    roots: usize,
    entities: usize,
    from_x: i32,
    from_y: i32,
}

#[derive(Deserialize)]
struct DetailsJson {
    components: String,
}

/// Commands as they arrive, with a look ahead so that a drag does not queue up: of several
/// positions of the same drag waiting, only the last is worth the engine's time.
struct Inbox<'a> {
    commands: Receiver<ToEngine>,
    ahead: VecDeque<ToEngine>,
    handled: &'a AtomicU64,
}

impl Inbox<'_> {
    /// Whether nothing waits to be handed on.
    fn quiet(&mut self) -> bool {
        if !self.ahead.is_empty() {
            return false;
        }
        match self.commands.try_recv() {
            Ok(command) => {
                self.ahead.push_back(command);
                false
            }
            Err(_) => true,
        }
    }

    fn next(&mut self) -> Option<ToEngine> {
        let mut command = match self.ahead.pop_front() {
            Some(command) => command,
            None => self.commands.recv().ok()?,
        };
        while let ToEngine::Edit { stroke: Some(stroke), coalesce: true, .. } = &command {
            let later = match self.ahead.pop_front() {
                Some(later) => later,
                None => match self.commands.try_recv() {
                    Ok(later) => later,
                    Err(_) => break,
                },
            };
            match &later {
                ToEngine::Edit { stroke: Some(next), coalesce: true, .. } if next == stroke => {
                    // The earlier position is passed over, which finishes it.
                    self.handled.fetch_add(1, Ordering::Release);
                    command = later;
                }
                _ => {
                    self.ahead.push_front(later);
                    break;
                }
            }
        }
        Some(command)
    }
}

/// Hands an answer to the session and wakes the interface. Shared with the threads that do
/// slow work beside the engine's own.
pub(super) type Reply = Arc<dyn Fn(FromEngine) + Send + Sync>;

/// Makes the fork's pack and textures where the cache has not got them, before the engine
/// starts, and opens the textures. The pack is made first, since the engine needs it. Returns
/// where the pack is.
pub(super) fn prepare(engine_folder: &Path, fork: &str, prepared: &Prepared, send: &Reply) -> Result<String, String> {
    if prepared.pack_stage.is_some() {
        cache::make_pack(engine_folder, Path::new(fork), &prepared.pack)?;
        send(FromEngine::Reached(Step::Pack));
    }
    if prepared.textures_stage.is_some() {
        cache::make_textures(Path::new(fork), engine_folder, &prepared.textures)?;
        send(FromEngine::Reached(Step::Bake));
    }
    let textures = BakedTextures::open(&prepared.textures)?;
    send(FromEngine::Textures(Arc::new(textures)));
    Ok(crate::files::forward_slashes(&prepared.pack))
}

pub(super) fn engine_thread(
    folder: &Path,
    fork: &str,
    pack: &str,
    map: Option<String>,
    commands: Receiver<ToEngine>,
    handled: &AtomicU64,
    send: &Reply,
) -> Result<(), String> {
    let engine = Engine::load(folder)?;
    send(FromEngine::Reached(Step::Runtime));
    engine.start(fork, Some(pack));
    send(FromEngine::Progress("Starting both engine kernels".into()));
    // The kernels start side by side and the warm-up follows in the same call, so the
    // engine tells no stage of them apart.
    let ready = engine.wait_ready()?;
    send(FromEngine::Reached(Step::Kernels));
    let tiles: Vec<TileJson> = engine.call("tiles", &json!({}))?;
    let prototypes = prototypes_at_start(engine.call("prototypes", &json!({}))?);
    let decal_prototypes = decals_at_start(engine.call("decals", &json!({})))?;
    // The ports are static, and asked once. An engine that does not tell them leaves none,
    // and links can still be made with the ports `ports.of` tells.
    let ports = engine.call::<PortsJson>("ports", &json!({})).map(Ports::from_json).unwrap_or_default();
    // An engine that does not tell its draw depths leaves Starlight's.
    let depths = engine.call::<Vec<DrawDepth>>("drawdepths", &json!({})).map(|depths| DepthGroups::from_depths(&depths)).unwrap_or_default();
    send(FromEngine::Ready {
        document_kernel_ms: ready["documentKernelMs"].as_u64().unwrap_or(0),
        view_kernel_ms: ready["viewKernelMs"].as_u64().unwrap_or(0),
        warm_up_ms: ready["warmUpMs"].as_u64().unwrap_or(0),
        tiles: tiles_at_start(tiles),
        prototypes,
        decal_prototypes,
        ports,
        depths,
    });

    let mut document: Option<i32> = None;
    // The open document's map entity, whose components hold its backdrop.
    let mut map_uid = 0;
    // The thread that paints backdrops for the gallery, once one is asked for.
    let mut painter: Option<Sender<PaintAsk>> = None;
    let mut revision = 0;
    // Which grid each entity stands on, to tell the grids whose air a change may have changed.
    let mut grids_of = GridsOf::default();
    // The grids whose air is read again once nothing else waits, so that the steps of a drag
    // do not wait for it.
    let mut air_owed: BTreeSet<i32> = BTreeSet::new();
    // The stroke the last edit belonged to, and whether it has an undo step of its own yet.
    let mut stroke_open: Option<(u64, bool)> = None;
    // The prototype kinds whose ids the engine has told with the fields, which it is not asked again.
    let mut ids_told: BTreeSet<String> = BTreeSet::new();
    let mut inbox = Inbox { commands, ahead: VecDeque::from([ToEngine::Open { path: map }]), handled };

    loop {
        if let (Some(document), false) = (document, air_owed.is_empty()) {
            if inbox.quiet() {
                read_air(engine, document, Some(std::mem::take(&mut air_owed).into_iter().collect()), send);
            }
        }
        let Some(command) = inbox.next() else { break };
        // Counted as finished however this turn of the loop ends, after its answer is sent.
        let _finished = Finished(handled);
        match command {
            ToEngine::Open { path } => {
                let opened: Result<Opened, String> = match &path {
                    Some(path) => engine.call("open", &json!({ "path": path })),
                    None => engine.call("new", &json!({})),
                };
                match opened {
                    Ok(opened) => {
                        send(FromEngine::Reached(Step::Map));
                        if let Some(old) = document.take() {
                            let _: Result<Value, String> = engine.call("close", &json!({ "doc": old }));
                        }
                        let id = opened.scene.document.id;
                        let list = engine.with_draw_list(id, <[u8]>::to_vec).ok_or("the engine recorded no draw list")?;
                        document = Some(id);
                        map_uid = opened.scene.document.map_uid;
                        revision = opened.scene.history.revision;
                        stroke_open = None;
                        // Every link once. The deltas keep the session's mirror of them after.
                        let links = engine.call::<LinksAnswer>("links", &json!({ "doc": id }));
                        grids_of = GridsOf::new(&opened.scene);
                        air_owed.clear();
                        send(FromEngine::Opened { scene: Box::new(opened.scene), list, timing: opened.timing, path, links });

                        read_backdrop(engine, id, send);
                        read_air(engine, id, None, send);
                    }
                    Err(error) => send(FromEngine::NotOpened(error)),
                }
            }
            ToEngine::Edit { ops, label, stroke, seq, asked, .. } => {
                let Some(document) = document else { continue };
                let begun = Instant::now();
                let merge = matches!((stroke, stroke_open), (Some(stroke), Some((open, true))) if stroke == open);
                match engine.call::<Edited>("edit", &json!({ "doc": document, "label": label, "merge": merge, "ops": ops })) {
                    Ok(edited) => {
                        let stepped = edited.delta.history.revision != revision;
                        revision = edited.delta.history.revision;
                        stroke_open = stroke.map(|stroke| (stroke, merge || stepped));
                        let list = engine.with_draw_list(document, <[u8]>::to_vec);
                        let timing = timing(asked, begun, &edited.timing);
                        let backdrop_changed = touches(&edited.delta, map_uid);
                        let air = grids_of.touched(&edited.delta);
                        send(FromEngine::Changed { list, delta: Some(Box::new(edited.delta)), what: label, seq, asked, timing });
                        if backdrop_changed {
                            read_backdrop(engine, document, send);
                        }
                        air_owed.extend(air);
                    }
                    Err(error) => send(FromEngine::Refused { seq, what: label, error }),
                }
            }
            ToEngine::SetComponent { uid, kind, component, label, seq, asked } => {
                let Some(document) = document else { continue };
                let begun = Instant::now();
                stroke_open = None;
                // The entity's data as the engine has it now, with the one component changed.
                let edited = engine
                    .call::<Vec<DetailsJson>>("details", &json!({ "doc": document, "uids": [uid] }))
                    .and_then(|details| details.into_iter().next().ok_or_else(|| format!("the engine has no entity {uid}")))
                    .and_then(|entity| serde_json::from_str::<Vec<Value>>(&entity.components).map_err(|error| error.to_string()))
                    .map(|components| Value::Array(with_component(components, kind, component)).to_string())
                    .and_then(|components| {
                        let op = json!({ "kind": "components", "uid": uid, "components": components });
                        engine.call::<Edited>("edit", &json!({ "doc": document, "label": label, "merge": false, "ops": [op] }))
                    });
                match edited {
                    Ok(edited) => {
                        revision = edited.delta.history.revision;
                        let list = engine.with_draw_list(document, <[u8]>::to_vec);
                        let timing = timing(asked, begun, &edited.timing);
                        let backdrop_changed = touches(&edited.delta, map_uid);
                        let air = grids_of.touched(&edited.delta);
                        send(FromEngine::Changed { list, delta: Some(Box::new(edited.delta)), what: label, seq, asked, timing });
                        if backdrop_changed {
                            read_backdrop(engine, document, send);
                        }
                        air_owed.extend(air);
                    }
                    Err(error) => send(FromEngine::Refused { seq, what: label, error }),
                }
            }
            ToEngine::BackdropNames => send(FromEngine::BackdropNames(backdrop::names(&engine))),
            ToEngine::PaintBackdrop(ask) => {
                let painter = painter.get_or_insert_with(|| {
                    let (asks, waiting) = mpsc::channel();
                    let send = send.clone();
                    let _ = std::thread::Builder::new().name("backdrop gallery".into()).spawn(move || paint_backdrops(engine, waiting, send));
                    asks
                });
                let _ = painter.send(ask);
            }
            ToEngine::Undo { seq, asked } | ToEngine::Redo { seq, asked } => {
                let Some(document) = document else { continue };
                let (op, what) = if matches!(command, ToEngine::Undo { .. }) { ("undo", "Undo") } else { ("redo", "Redo") };
                let begun = Instant::now();
                stroke_open = None;
                match engine.call::<Edited>(op, &json!({ "doc": document })) {
                    Ok(edited) => {
                        revision = edited.delta.history.revision;
                        let list = engine.with_draw_list(document, <[u8]>::to_vec);
                        let timing = timing(asked, begun, &edited.timing);
                        let backdrop_changed = touches(&edited.delta, map_uid);
                        let air = grids_of.touched(&edited.delta);
                        send(FromEngine::Changed { list, delta: Some(Box::new(edited.delta)), what, seq, asked, timing });
                        if backdrop_changed {
                            read_backdrop(engine, document, send);
                        }
                        air_owed.extend(air);
                    }
                    Err(error) => send(FromEngine::Refused { seq, what, error }),
                }
            }
            ToEngine::Save { path } => {
                let Some(document) = document else { continue };
                match engine.call::<Saved>("save", &json!({ "doc": document, "path": path })) {
                    Ok(saved) => send(FromEngine::Saved { path, ok: saved.ok, failure: saved.failure, milliseconds: saved.milliseconds, bytes: saved.bytes, revision }),
                    Err(error) => send(FromEngine::Saved { path, ok: false, failure: Some(error), milliseconds: 0.0, bytes: 0, revision }),
                }
            }
            ToEngine::SubFloor { show, asked } => {
                let begun = Instant::now();
                let layered = engine.call::<Layered>("layers", &json!({ "doc": document.unwrap_or(0), "subFloor": show }));
                // Before a map is open there is nothing to show for it.
                if let (Some(document), Ok(layered)) = (document, layered) {
                    let list = engine.with_draw_list(document, <[u8]>::to_vec);
                    send(FromEngine::Changed { list, delta: None, what: "T-Ray view changed", seq: 0, asked, timing: timing(asked, begun, &layered.timing) });
                }
            }
            ToEngine::Previews { items, directions } => {
                let keys: Vec<i32> = items.iter().map(|item| item.0).collect();
                let wanted: Vec<Value> = items.iter().map(|(key, prototype)| json!({ "key": key, "prototype": prototype })).collect();
                let list = match engine.call::<Value>("previews", &json!({ "items": wanted, "directions": directions })) {
                    Ok(_) => engine.with_draw_list(PREVIEWS, <[u8]>::to_vec),
                    Err(_) => None,
                };
                send(FromEngine::Previews { list, keys, directions });
            }
            ToEngine::Details { uid } => {
                let Some(document) = document else { continue };
                if let Ok(details) = engine.call::<Vec<DetailsJson>>("details", &json!({ "doc": document, "uids": [uid] })) {
                    let components = details.first().and_then(|entity| serde_json::from_str(&entity.components).ok()).unwrap_or(Value::Null);
                    send(FromEngine::Details { uid, components });
                }
            }
            ToEngine::Fields { uid } => {
                let Some(document) = document else { continue };
                let answer = engine.call::<FieldsAnswer>("component.fields", &json!({ "doc": document, "uid": uid, "known": ids_told }));
                if let Ok(answer) = &answer {
                    ids_told.extend(answer.ids.keys().cloned());
                }
                send(FromEngine::Fields { uid, answer });
            }
            ToEngine::Containers { uid } => {
                let Some(document) = document else { continue };
                let answer = engine.call::<Vec<EntityContainers>>("containers", &json!({ "doc": document, "uids": [uid] }));
                let containers = answer.map(|entities| entities.into_iter().find(|entity| entity.uid == uid).map(|entity| entity.containers).unwrap_or_default());
                send(FromEngine::Containers { uid, containers });
            }
            ToEngine::Copy { uids, seq } => {
                let Some(document) = document else { continue };
                match engine.call::<CopiedJson>("copy", &json!({ "doc": document, "uids": uids })) {
                    Ok(copied) => send(FromEngine::Copied { seq, text: copied.text, roots: copied.roots, entities: copied.entities, from: (copied.from_x, copied.from_y) }),
                    Err(error) => send(FromEngine::Refused { seq, what: "Copy", error }),
                }
            }
            ToEngine::PortsOf { uids } => {
                let Some(document) = document else { continue };
                let ports = engine.call::<Vec<EntityPorts>>("ports.of", &json!({ "doc": document, "uids": uids }));
                send(FromEngine::PortsOf { uids, ports });
            }
            ToEngine::AirFixPreview => {
                let Some(document) = document else { continue };
                let previews = engine.call::<Vec<super::FixPreview>>("air.fix.preview", &json!({ "doc": document }));
                send(FromEngine::AirFixPreview { document, previews });
            }
        }
    }
    Ok(())
}

/// The engine's list of tiles as the session keeps it.
fn tiles_at_start(tiles: Vec<TileJson>) -> Vec<Tile> {
    tiles.into_iter().map(|tile| Tile { id: tile.id, sprite: tile.sprite, allows_rotation: tile.allows_rotation, can_crowbar: tile.can_crowbar }).collect()
}

/// The engine's list of entity prototypes as the session keeps it: every one of them, where
/// the engine listed it, with the text it is searched by. Those the game keeps out of its
/// spawn menu stay, since a map may hold entities of them. The names of components, tags and
/// categories are shared here, on the engine's thread, where the time it takes holds up no frame.
fn prototypes_at_start(mut prototypes: Vec<Prototype>) -> Vec<Prototype> {
    for prototype in &mut prototypes {
        prototype.search = format!("{} {}", prototype.id, prototype.name).to_lowercase();
    }
    share_names(&mut prototypes);
    prototypes
}

/// What to make of the engine's answer to `decals` at start. An engine build from before the
/// kernel listed decals does not know the command: it has none to offer, and starts all the
/// same. Any other failure stops the start, as one of `tiles` or `prototypes` does.
fn decals_at_start(answer: Result<Vec<DecalPrototype>, String>) -> Result<Vec<DecalPrototype>, String> {
    match answer {
        Err(error) if error.starts_with("Unknown engine command") => Ok(Vec::new()),
        answer => answer,
    }
}

/// Reads what the game draws behind a document's map. The first time, the game's generator
/// paints its stars, which takes seconds: on a thread of its own, and not this one, which
/// edits wait for.
fn read_backdrop(engine: Engine, document: i32, send: &Reply) {
    let send = send.clone();
    let _ = std::thread::Builder::new().name("backdrop".into()).spawn(move || {
        let backdrop = backdrop::load(&engine, document, &backdrop::cache_folder());
        send(FromEngine::Backdrop { document, backdrop });
    });
}

/// Reads the air of a document's grids, or with `grids` of those alone, on the engine's own
/// thread, after the change that may have changed it has been answered, and finds their rooms
/// there too. A change that touched no grid asks nothing.
fn read_air(engine: Engine, document: i32, grids: Option<Vec<i32>>, send: &Reply) {
    if grids.as_ref().is_some_and(Vec::is_empty) {
        return;
    }
    let mut answer = match &grids {
        Some(grids) => engine.call::<AirAnswer>("air", &json!({ "doc": document, "grids": grids })),
        None => engine.call::<AirAnswer>("air", &json!({ "doc": document })),
    };
    if let Ok(answer) = answer.as_mut() {
        answer.grids.iter_mut().for_each(GridAirJson::find_rooms);
    }
    send(FromEngine::Air { document, asked: grids, answer });
}

/// Whether a change reports new component data for the map entity, where its backdrop is kept.
fn touches(delta: &Delta, map_uid: i32) -> bool {
    delta.details.iter().any(|details| details.uid == map_uid)
}

/// An entity's components as "details" gives them, with those of one kind taken out and
/// `component` put in their place, if there is one.
fn with_component(mut components: Vec<Value>, kind: &str, component: Option<Value>) -> Vec<Value> {
    components.retain(|old| old["type"] != kind);
    components.extend(component);
    components
}

/// The gallery's painter: one backdrop at a time, in the order asked, except that one wanted
/// whole goes first, since the mouse is over it.
fn paint_backdrops(engine: Engine, asks: Receiver<PaintAsk>, send: Reply) {
    let cache = backdrop::cache_folder();
    let mut queue = VecDeque::new();
    loop {
        if queue.is_empty() {
            match asks.recv() {
                Ok(ask) => queue_ask(&mut queue, ask),
                Err(_) => return,
            }
        }
        for ask in asks.try_iter() {
            queue_ask(&mut queue, ask);
        }
        let Some(ask) = queue.pop_front() else { continue };
        let (thumb, whole) = match backdrop::load_named(&engine, &ask.name, &cache) {
            Ok(backdrop) => (Ok(backdrop::thumbnail(&backdrop, backdrop::THUMB_SIZE)), ask.whole.then_some(backdrop)),
            Err(error) => (Err(error), None),
        };
        send(FromEngine::BackdropPainted { name: ask.name, thumb, whole });
    }
}

/// Puts a backdrop in the painter's queue: one wanted whole at the front, and any other at the
/// back, once.
fn queue_ask(queue: &mut VecDeque<PaintAsk>, ask: PaintAsk) {
    let waiting = queue.iter().position(|queued| queued.name == ask.name);
    if ask.whole {
        if let Some(at) = waiting {
            queue.remove(at);
        }
        queue.push_front(ask);
    } else if waiting.is_none() {
        queue.push_back(ask);
    }
}

struct Finished<'a>(&'a AtomicU64);

impl Drop for Finished<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

fn timing(asked: Instant, begun: Instant, engine: &EngineTiming) -> Timing {
    Timing {
        queued: (begun - asked).as_secs_f64() * 1000.0,
        engine: begun.elapsed().as_secs_f64() * 1000.0,
        view: engine.view_ms,
        record: engine.record_ms,
        ready: Some(Instant::now()),
    }
}

#[cfg(test)]
#[path = "../tests/session/engine_thread.rs"]
mod tests;
