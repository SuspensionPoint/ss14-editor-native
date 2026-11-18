//! The popular forks the start screen offers for a one click download, and what the screen
//! shows of any fork: its owner on GitHub for an avatar, a monogram and a colour for its badge.

use super::download::tidy_url;
use super::Fnv;

/// One repository of a popular fork.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repository {
    pub url: &'static str,
    /// What the choice between a fork's repositories says of it. Empty when it has one.
    pub label: &'static str,
    /// A word naming it beside the fork's name when it is not the first, as "original".
    pub tag: &'static str,
}

/// A fork the start screen offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopularFork {
    pub name: &'static str,
    /// The first is offered first.
    pub repositories: &'static [Repository],
    pub description: &'static str,
    pub licence: &'static str,
    /// Whether it is known to build on an engine fork of its own, which the editor may have to
    /// build its engine side for, or may refuse when its engine patches do not fit.
    pub own_engine: bool,
    /// The colour of its card's badge, as `#rrggbb`.
    pub colour: &'static str,
    /// The upstream all the others build on.
    pub upstream: bool,
}

const fn one(url: &'static str) -> Repository {
    Repository { url, label: "", tag: "" }
}

/// The upstream first, then the forks by name.
pub const POPULAR_FORKS: [PopularFork; 6] = [
    PopularFork {
        name: "Space Wizards",
        repositories: &[one("https://github.com/space-wizards/space-station-14")],
        description: "Space Station 14 itself, the upstream every fork here builds on.",
        licence: "MIT code, CC-BY-SA-3.0 assets",
        own_engine: false,
        colour: "#4488ff",
        upstream: true,
    },
    PopularFork {
        name: "Delta-V",
        repositories: &[one("https://github.com/DeltaV-Station/Delta-v")],
        description: "A medium roleplay fork.",
        licence: "AGPL-3.0 / MIT",
        own_engine: false,
        colour: "#c8553d",
        upstream: false,
    },
    PopularFork {
        name: "Funky Station",
        // forky-station, the active one, rebased onto upstream. The original funky-station is
        // left out, as it is no longer where the work happens.
        repositories: &[one("https://github.com/funky-station/forky-station")],
        description: "Funky Station's active repository (forky-station), rebased onto upstream.",
        licence: "MIT",
        own_engine: true,
        colour: "#d0489b",
        upstream: false,
    },
    PopularFork {
        name: "Goob Station",
        repositories: &[one("https://github.com/Goob-Station/Goob-Station")],
        description: "A large, busy fork with lots of its own content.",
        licence: "AGPL-3.0-or-later, with REUSE headers",
        own_engine: true,
        colour: "#3f9e6b",
        upstream: false,
    },
    PopularFork {
        name: "RMC-14 (Rouny's Marine Corps 14)",
        repositories: &[one("https://github.com/RMC-14/RMC-14")],
        description: "Colonial Marines for Space Station 14, a remake of CM-SS13.",
        licence: "MIT code, CC-BY-SA-3.0 assets",
        own_engine: false,
        colour: "#7a8a3a",
        upstream: false,
    },
    PopularFork {
        name: "Starlight",
        repositories: &[one("https://github.com/ss14Starlight/space-station-14")],
        description: "A fork with its own plumbing.",
        licence: "Mostly MIT, some commits under the Starlight Fork License",
        own_engine: true,
        colour: "#7a5ad8",
        upstream: false,
    },
];

/// The popular fork a URL is one of the repositories of, with the repository's place.
pub fn popular_by_url(url: &str) -> Option<(&'static PopularFork, usize)> {
    let url = tidy_url(url);
    POPULAR_FORKS.iter().find_map(|fork| fork.repositories.iter().position(|repository| tidy_url(repository.url).eq_ignore_ascii_case(&url)).map(|at| (fork, at)))
}

/// The name a download of a popular fork's repository is shown by: the fork's, with the
/// repository's tag after it when it is not the first.
pub fn popular_name(fork: &PopularFork, at: usize) -> String {
    match fork.repositories.get(at).map(|repository| repository.tag).filter(|tag| !tag.is_empty()) {
        Some(tag) => format!("{} ({tag})", fork.name),
        None => fork.name.to_owned(),
    }
}

/// The owner of a GitHub repository by its URL, web or ssh, as "space-wizards". None for
/// another host.
pub fn github_owner(url: &str) -> Option<String> {
    let url = tidy_url(url);
    let rest = ["https://github.com/", "http://github.com/", "https://www.github.com/", "ssh://git@github.com/", "git@github.com:"]
        .iter()
        .find_map(|start| url.get(..start.len()).filter(|head| head.eq_ignore_ascii_case(start)).map(|_| &url[start.len()..]))?;
    let owner = rest.split('/').next()?;
    // GitHub's names: letters, digits and hyphens.
    let fits = !owner.is_empty() && owner.len() <= 39 && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    fits.then(|| owner.to_owned())
}

/// The letters on a fork's badge when it has no avatar: a short name in capitals as it is
/// ("RMC"), else the first letters of the first two words ("Space Wizards" is "SW",
/// "Delta-V" is "DV"), else the first letter.
pub fn monogram(name: &str) -> String {
    let words: Vec<&str> = name.split([' ', '-', '_', '.', '(']).filter(|word| word.chars().next().is_some_and(char::is_alphabetic)).collect();
    let Some(first) = words.first() else {
        return name.chars().find(|c| c.is_alphanumeric()).map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
    };
    if (2..=3).contains(&first.len()) && first.chars().all(|c| c.is_ascii_uppercase()) {
        return (*first).to_owned();
    }
    words.iter().take(2).filter_map(|word| word.chars().next()).flat_map(char::to_uppercase).collect()
}

/// The colours a fork that is not a popular one gets its badge in, by its name.
const OTHER_COLOURS: [&str; 6] = ["#2f7fa8", "#8a5cc0", "#b0763a", "#3a9a8f", "#b04a6a", "#5a6ad0"];

/// The colour of a fork's badge: a popular fork's own, else one of a few picked by its name,
/// the same each time.
pub fn colour_for(name: &str, url: Option<&str>) -> &'static str {
    if let Some((fork, _)) = url.and_then(popular_by_url) {
        return fork.colour;
    }
    // A hash that stays the same from run to run.
    let mut hash = Fnv::default();
    hash.add(name.to_lowercase().as_bytes());
    OTHER_COLOURS[(hash.0 % OTHER_COLOURS.len() as u64) as usize]
}

/// Whether a known fork's URL or folder is one of a popular fork's repositories.
pub fn is_popular_repository(fork: &PopularFork, url: Option<&str>) -> bool {
    url.is_some_and(|url| popular_by_url(url).is_some_and(|(found, _)| found.name == fork.name))
}

#[cfg(test)]
#[path = "../tests/forks/popular.rs"]
mod tests;
