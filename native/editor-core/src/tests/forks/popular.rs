use super::*;

#[test]
fn the_popular_forks_are_the_upstream_then_the_owner_s_five_by_name() {
    let names: Vec<&str> = POPULAR_FORKS.iter().map(|fork| fork.name).collect();
    assert_eq!(names, ["Space Wizards", "Delta-V", "Funky Station", "Goob Station", "RMC-14 (Rouny's Marine Corps 14)", "Starlight"]);
    let mut by_name = names[1..].to_vec();
    by_name.sort_by_key(|name| name.to_lowercase());
    assert_eq!(by_name, names[1..]);
    let upstream: Vec<&str> = POPULAR_FORKS.iter().filter(|fork| fork.upstream).map(|fork| fork.name).collect();
    assert_eq!(upstream, ["Space Wizards"]);
    let own_engine: Vec<&str> = POPULAR_FORKS.iter().filter(|fork| fork.own_engine).map(|fork| fork.name).collect();
    assert_eq!(own_engine, ["Funky Station", "Goob Station", "Starlight"]);
}

#[test]
fn every_popular_fork_has_what_its_card_shows() {
    for fork in &POPULAR_FORKS {
        assert!(!fork.repositories.is_empty(), "{}", fork.name);
        assert!(!fork.description.is_empty() && !fork.description.contains('\n'), "{}", fork.name);
        assert!(!fork.licence.is_empty(), "{}", fork.name);
        assert!(fork.colour.len() == 7 && fork.colour.starts_with('#') && fork.colour[1..].chars().all(|c| c.is_ascii_hexdigit()), "{}", fork.name);
        for repository in fork.repositories {
            assert!(repository.url.starts_with("https://github.com/"), "{}", repository.url);
            assert!(github_owner(repository.url).is_some(), "{}", repository.url);
            // A fork with a choice labels each repository.
            assert_eq!(fork.repositories.len() > 1, !repository.label.is_empty(), "{}", repository.url);
        }
    }
}

#[test]
fn funky_station_is_its_active_repository_alone() {
    let funky = POPULAR_FORKS.iter().find(|fork| fork.name == "Funky Station").unwrap();
    assert_eq!(funky.repositories, [one("https://github.com/funky-station/forky-station")]);
    assert_eq!(popular_name(funky, 0), "Funky Station");
}

#[test]
fn a_url_is_found_among_the_popular_repositories_in_any_form() {
    let (fork, at) = popular_by_url("https://github.com/goob-station/goob-station.git/").unwrap();
    assert_eq!((fork.name, at), ("Goob Station", 0));
    let (fork, at) = popular_by_url(" https://github.com/funky-station/forky-station ").unwrap();
    assert_eq!((fork.name, at), ("Funky Station", 0));
    assert!(popular_by_url("https://github.com/funky-station/funky-station").is_none());
    assert!(popular_by_url("https://github.com/someone/space-station-14").is_none());
    let starlight = POPULAR_FORKS.iter().find(|fork| fork.name == "Starlight").unwrap();
    assert!(is_popular_repository(starlight, Some("https://github.com/ss14Starlight/space-station-14.git")));
    assert!(!is_popular_repository(starlight, Some("https://github.com/space-wizards/space-station-14")));
    assert!(!is_popular_repository(starlight, None));
}

#[test]
fn the_owner_on_github_is_read_from_web_and_ssh_urls() {
    assert_eq!(github_owner("https://github.com/space-wizards/space-station-14").as_deref(), Some("space-wizards"));
    assert_eq!(github_owner("https://GitHub.com/RMC-14/RMC-14.git").as_deref(), Some("RMC-14"));
    assert_eq!(github_owner("git@github.com:DeltaV-Station/Delta-v.git").as_deref(), Some("DeltaV-Station"));
    assert_eq!(github_owner("ssh://git@github.com/ss14Starlight/space-station-14").as_deref(), Some("ss14Starlight"));
    assert_eq!(github_owner("https://gitlab.com/someone/fork"), None);
    assert_eq!(github_owner("C:/forks/starlight"), None);
    assert_eq!(github_owner("https://github.com/../evil"), None);
    assert_eq!(github_owner("https://github.com/a b/c"), None);
}

#[test]
fn a_badge_without_an_avatar_shows_a_monogram() {
    assert_eq!(monogram("Space Wizards"), "SW");
    assert_eq!(monogram("Delta-V"), "DV");
    assert_eq!(monogram("RMC-14 (Rouny's Marine Corps 14)"), "RMC");
    assert_eq!(monogram("Starlight"), "S");
    assert_eq!(monogram("Goob Station"), "GS");
    assert_eq!(monogram("funky station (original)"), "FS");
    assert_eq!(monogram("14"), "1");
    assert_eq!(monogram(""), "?");
}

#[test]
fn a_fork_s_colour_is_its_popular_one_or_stays_the_same_for_its_name() {
    assert_eq!(colour_for("anything", Some("https://github.com/DeltaV-Station/Delta-v")), "#c8553d");
    let mine = colour_for("my-fork", None);
    assert_eq!(colour_for("My-Fork", Some("https://example.com/my-fork")), mine);
    assert!(OTHER_COLOURS.contains(&mine));
}
