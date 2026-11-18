//! The backdrop gallery: what it lists, what it asks the engine to paint and in what order,
//! what is shown behind the map while the mouse is over a choice, and the one edit a choice is.

use super::entity::{opened, refuse};
use super::*;
use crate::backdrop::Thumb;

/// The engine's backdrops, as it lists them.
const NAMES: [&str; 3] = ["Amber", "Default", "Water"];

/// A map's own backdrop as the engine reads it.
fn own(name: &str, chosen: bool, planet: bool) -> Backdrop {
    Backdrop { name: name.into(), chosen, planet, ..Default::default() }
}

/// A backdrop of the fork, painted, with an id to know it by.
fn painted(name: &str, id: u64) -> Backdrop {
    Backdrop { id, name: name.into(), ..Default::default() }
}

fn thumb() -> Thumb {
    Thumb { width: 1, height: 1, pixels: vec![0, 0, 0, 255] }
}

/// A session with the test map open as document 1, whose backdrop the engine reads.
fn open() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = opened(&scene());
    // The test map has no draw list, which is what makes the document the session's.
    session.document = Some(1);
    (session, answers, engine)
}

/// A session with the test map open, its own backdrop read as Water, and nothing asked yet.
fn with_water() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = open();
    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Water", true, false)) });
    session.take_backdrop();
    let _ = gallery_asks(&engine);
    (session, answers, engine)
}

fn send(session: &mut Session, answers: &Sender<FromEngine>, reply: FromEngine) {
    answers.send(reply).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// What the gallery asked of the engine since the last call: "list", a picture by its
/// backdrop, or "whole" and the backdrop.
fn gallery_asks(from: &Receiver<ToEngine>) -> Vec<String> {
    from.try_iter()
        .filter_map(|command| match command {
            ToEngine::BackdropNames => Some("list".to_owned()),
            ToEngine::PaintBackdrop(ask) if ask.whole => Some(format!("whole {}", ask.name)),
            ToEngine::PaintBackdrop(ask) => Some(ask.name),
            _ => None,
        })
        .collect()
}

/// Every component the session set since the last call: the entity, the kind, the component
/// and the edit's name.
fn components_set(from: &Receiver<ToEngine>) -> Vec<(i32, &'static str, Option<Value>, &'static str)> {
    from.try_iter()
        .filter_map(|command| match command {
            ToEngine::SetComponent { uid, kind, component, label, .. } => Some((uid, kind, component, label)),
            _ => None,
        })
        .collect()
}

/// The gallery as it lists its choices: each label, with "*" for the current and ">" for the
/// one under the mouse.
fn listed(session: &Session) -> Vec<String> {
    session
        .backdrop_entries()
        .into_iter()
        .map(|entry| format!("{}{}{}", entry.label, if entry.current { " *" } else { "" }, if entry.hovered { " >" } else { "" }))
        .collect()
}

/// Opens the gallery and answers the list.
fn open_listed(session: &mut Session, answers: &Sender<FromEngine>) {
    assert!(session.open_backdrops());
    send(session, answers, FromEngine::BackdropNames(Ok(NAMES.map(str::to_owned).to_vec())));
}

#[test]
fn the_gallery_cannot_open_for_a_grid_file_a_planet_or_a_backdrop_not_read_yet() {
    let (mut session, answers, engine) = open();
    assert_eq!(session.backdrop_unavailable(), Some("The map's backdrop is still loading."));
    assert!(!session.open_backdrops());
    assert_eq!(session.status_line(), "The map's backdrop is still loading.");

    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Default", false, true)) });
    assert!(!session.open_backdrops() && !session.backdrops_open());
    assert_eq!(session.status_line(), "A planet map draws its biome instead of a backdrop.");

    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Default", false, false)) });
    session.doc.as_mut().unwrap().category = "Grid".into();
    assert!(!session.open_backdrops());
    assert!(session.status_line().starts_with("A grid file has no backdrop of its own."));
    // Nothing was asked of the engine, and nothing chosen edits.
    session.choose_backdrop(1);
    assert!(gallery_asks(&engine).is_empty());

    session.doc.as_mut().unwrap().category = "Map".into();
    assert_eq!(session.backdrop_unavailable(), None);
    assert!(session.open_backdrops());
}

#[test]
fn the_gallery_lists_the_game_default_first_marks_the_current_and_asks_each_picture_once_in_order() {
    let (mut session, answers, engine) = with_water();
    assert!(session.open_backdrops());
    assert_eq!(gallery_asks(&engine), ["list"]);
    // Before the list is here there is nothing to show.
    assert!(session.backdrop_entries().is_empty());

    send(&mut session, &answers, FromEngine::BackdropNames(Ok(NAMES.map(str::to_owned).to_vec())));
    assert_eq!(listed(&session), ["Game default", "Amber", "Default", "Water *"]);
    // The game's default shows the picture of the backdrop the game draws then.
    assert_eq!(session.backdrop_entries()[0].shows, "Default");
    // The pictures, in the gallery's order, the game's default's once.
    assert_eq!(gallery_asks(&engine), ["Default", "Amber", "Water"]);

    // Closed and opened again, nothing is asked twice.
    session.cancel_backdrops();
    assert!(session.open_backdrops());
    assert!(gallery_asks(&engine).is_empty());

    // A placeholder until each picture is here.
    assert_eq!(session.backdrop_thumb("Amber"), None);
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Amber".into(), thumb: Ok(thumb()), whole: None });
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Water".into(), thumb: Err("no such file".into()), whole: None });
    assert_eq!(session.backdrop_thumb("Amber"), Some(Ok(&thumb())));
    assert_eq!(session.backdrop_thumb("Water"), Some(Err("no such file")));
    // The last one here says how long they all took.
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Default".into(), thumb: Ok(thumb()), whole: None });
    assert!(session.status_line().starts_with("3 backdrop pictures ready in "), "{}", session.status_line());
}

#[test]
fn a_map_that_names_no_backdrop_has_the_game_default_current() {
    let (mut session, answers, _engine) = open();
    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Default", false, false)) });
    open_listed(&mut session, &answers);
    assert_eq!(listed(&session), ["Game default *", "Amber", "Default", "Water"]);
}

#[test]
fn the_backdrop_under_the_mouse_is_shown_behind_the_map_and_the_map_s_own_when_it_leaves() {
    let (mut session, answers, engine) = with_water();
    open_listed(&mut session, &answers);
    let _ = gallery_asks(&engine);
    let own_id = session.gallery.own.as_ref().unwrap().id;

    // Over Amber: it is asked for whole, ahead of the pictures, and shown once it is here.
    session.hover_backdrop(1);
    assert_eq!(gallery_asks(&engine), ["whole Amber"]);
    assert_eq!(listed(&session)[1], "Amber >");
    assert!(session.take_backdrop().is_none());
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Amber".into(), thumb: Ok(thumb()), whole: Some(painted("Amber", 50)) });
    assert_eq!(session.take_backdrop().map(|shown| shown.id), Some(50));

    // Left: the map's own again. Nothing was edited.
    session.leave_backdrops();
    assert_eq!(session.take_backdrop().map(|shown| shown.id), Some(own_id));
    assert!(components_set(&engine).is_empty());

    // Over Amber again it is kept, and not asked for again.
    session.hover_backdrop(1);
    assert_eq!(session.take_backdrop().map(|shown| shown.id), Some(50));
    assert!(gallery_asks(&engine).is_empty());

    // One that arrives after the mouse moved on is kept and not shown.
    session.hover_backdrop(3);
    assert_eq!(gallery_asks(&engine), ["whole Water"]);
    session.hover_backdrop(2);
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Water".into(), thumb: Ok(thumb()), whole: Some(painted("Water", 51)) });
    assert!(session.take_backdrop().is_none());

    // The map's own read again while the mouse shows another does not take its place.
    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Water", true, false)) });
    assert!(session.take_backdrop().is_none());
}

#[test]
fn only_a_few_backdrops_are_kept_whole() {
    let (mut session, answers, engine) = with_water();
    open_listed(&mut session, &answers);
    for (index, name) in [(1, "Amber"), (2, "Default"), (3, "Water"), (0, "Default")] {
        session.hover_backdrop(index);
        if gallery_asks(&engine).iter().any(|ask| ask.starts_with("whole")) {
            send(&mut session, &answers, FromEngine::BackdropPainted { name: name.into(), thumb: Ok(thumb()), whole: Some(painted(name, 60 + index as u64)) });
        }
    }
    let kept: Vec<&str> = session.gallery.whole.iter().map(|(name, _)| name.as_str()).collect();
    // Default was used last, and was not painted twice: the game's default shows it too.
    assert_eq!(kept, ["Amber", "Water", "Default"]);
}

#[test]
fn a_choice_is_one_edit_of_the_map_s_parallax_and_the_game_default_takes_it_away() {
    let (mut session, answers, engine) = with_water();
    open_listed(&mut session, &answers);
    session.hover_backdrop(1);
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Amber".into(), thumb: Ok(thumb()), whole: Some(painted("Amber", 70)) });
    session.take_backdrop();

    session.choose_backdrop(1);
    assert_eq!(components_set(&engine), [(1, "Parallax", Some(json!({ "type": "Parallax", "parallax": "Amber" })), "Set backdrop")]);
    assert!(!session.backdrops_open());
    // What was chosen stays shown, also as the mouse leaves, until the map's own is read again.
    assert_eq!(session.take_backdrop().map(|shown| shown.id), Some(70));
    session.leave_backdrops();
    assert!(session.take_backdrop().is_none());
    send(&mut session, &answers, FromEngine::Backdrop { document: 1, backdrop: Ok(own("Amber", true, false)) });
    assert_eq!(session.take_backdrop().map(|shown| shown.name.clone()), Some("Amber".into()));

    // The game's default takes the component away.
    assert!(session.open_backdrops());
    assert_eq!(listed(&session)[1], "Amber *");
    session.choose_backdrop(0);
    assert_eq!(components_set(&engine), [(1, "Parallax", None, "Set backdrop")]);
}

#[test]
fn the_current_choice_cancel_and_escape_change_nothing() {
    let (mut session, answers, engine) = with_water();
    open_listed(&mut session, &answers);
    session.choose_backdrop(3);
    assert!(!session.backdrops_open() && components_set(&engine).is_empty());

    assert!(session.open_backdrops());
    session.hover_backdrop(1);
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Amber".into(), thumb: Ok(thumb()), whole: Some(painted("Amber", 80)) });
    session.take_backdrop();
    session.cancel_backdrops();
    assert!(!session.backdrops_open() && components_set(&engine).is_empty());
    assert_eq!(session.take_backdrop().map(|shown| shown.name.clone()), Some("Water".into()));
}

#[test]
fn a_refused_choice_shows_the_map_s_own_again() {
    let (mut session, answers, engine) = with_water();
    open_listed(&mut session, &answers);
    session.hover_backdrop(1);
    send(&mut session, &answers, FromEngine::BackdropPainted { name: "Amber".into(), thumb: Ok(thumb()), whole: Some(painted("Amber", 90)) });
    session.choose_backdrop(1);
    let seq = engine.try_iter().find_map(|command| match command {
        ToEngine::SetComponent { seq, .. } => Some(seq),
        _ => None,
    });
    session.take_backdrop();
    refuse(&mut session, &answers, seq.unwrap(), "Set backdrop", "A planet map draws its biome instead of a backdrop.");
    assert_eq!(session.take_backdrop().map(|shown| shown.name.clone()), Some("Water".into()));
    assert_eq!(session.status_line(), "Set backdrop: the engine refused: A planet map draws its biome instead of a backdrop.");
}

#[test]
fn another_map_opened_closes_the_gallery() {
    let (mut session, answers, _engine) = with_water();
    open_listed(&mut session, &answers);
    let scene: SceneJson = serde_json::from_str(&scene()).unwrap();
    send(&mut session, &answers, FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) });
    assert!(!session.backdrops_open());
    assert_eq!(session.backdrop_unavailable(), Some("The map's backdrop is still loading."));
    // The backdrop of the map before is taken away.
    assert_eq!(session.take_backdrop().map(|shown| shown.layers.len()), Some(0));
}
