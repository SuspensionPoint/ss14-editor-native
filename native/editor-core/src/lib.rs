//! The editor's core, everything but the interface: the session and its tools, the mirror of the
//! open map, forks and their cache, the hosted engine, baked textures and the GPU map renderer.
//! Nearly all the logic lives here, and its tests run without an engine.

pub mod air;
pub mod animation;
pub mod air_checks;
pub mod air_draw;
pub mod backdrop;
pub mod colour;
pub mod draw_depths;
pub mod drawlist;
pub mod engine;
pub mod fields;
pub mod files;
pub mod forks;
pub mod links;
pub mod loading;
pub mod locale;
pub mod model;
pub mod palette;
pub mod prefab;
pub mod problems;
pub mod renderer;
pub mod rooms;
pub mod search;
pub mod session;
pub mod shell;
pub mod textures;
pub mod thumbs;
pub mod validate;
