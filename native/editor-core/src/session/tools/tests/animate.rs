//! The Animate sprites switch.

use super::*;
use crate::forks::settings::Settings;
use crate::forks::tests::folder;

#[test]
fn sprites_stand_still_until_the_switch_is_turned_on() {
    let (session, engine) = Session::offline(&scene());
    assert!(!session.animate_sprites());
    assert!(asked(&engine).is_empty());
}

#[test]
fn the_switch_is_kept_in_the_settings_for_every_fork() {
    let file = folder("animate").join("settings.json");
    let mut settings = Settings::default();
    settings.used_fork(std::path::Path::new("C:/forks/starlight"), Some("p".into()), None);
    settings.save(&file).unwrap();

    let (mut session, _engine) = Session::offline(&scene());
    session.animate = crate::session::animate::Animate::load(file.clone());
    assert!(!session.animate_sprites());
    session.set_animate_sprites(true);
    let read = Settings::load(&file);
    assert!(read.animate_sprites);
    // What else the file holds stays.
    assert_eq!(read.forks[0].prototypes.as_deref(), Some("p"));

    // A session started after it has it on, whatever its fork.
    let (mut again, _again_engine) = Session::offline(&scene());
    again.animate = crate::session::animate::Animate::load(file.clone());
    assert!(again.animate_sprites());
    again.set_animate_sprites(false);
    assert!(!Settings::load(&file).animate_sprites);
}
