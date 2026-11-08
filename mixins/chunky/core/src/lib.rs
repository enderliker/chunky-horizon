//! Chunky, for Horizon.
//!
//! This crate is the whole of the port that does not need the game to run: the
//! selection maths, the shapes, the patterns, the region files, the task state
//! machine and the commands. It is a plain library, so `cargo test` exercises
//! all of it without compiling Horizon, and the bundle in `../..` is left with
//! nothing but the wiring between these pieces and the game's hooks.
//!
//! It is a port of [Chunky](https://github.com/pop4959/Chunky) by pop4959, and
//! follows its behaviour closely enough that a task started here walks a world
//! in the same order, reports the same numbers, and leaves the same region
//! files behind. Where Horizon forced a difference — the game is ticked, not a
//! pool of threads — the difference is described where it is made.

pub mod anvil;
pub mod command;
pub mod config;
pub mod geometry;
pub mod hilbert;
pub mod messages;
pub mod pattern;
pub mod selection;
pub mod shape;
pub mod task;
pub mod trim;

/// The mod's id, as the manifest and the game know it.
pub const ID: &str = "chunky";
/// The name shown to players.
pub const NAME: &str = "Chunky";
/// The port's own version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The version of Chunky this port was written against.
pub const UPSTREAM_VERSION: &str = "1.4.45";
/// One line for the mod list.
pub const DESCRIPTION: &str = "Pre-generates chunks, quickly and efficiently.";
/// The Horizon native-hook API the bundle is built for.
pub const HOOKS: u32 = 411;

/// The authors of the original, who wrote everything this is a port of.
pub const UPSTREAM_AUTHORS: &str = "pop4959 and the Chunky contributors";
/// Where the original lives.
pub const UPSTREAM_URL: &str = "https://github.com/pop4959/Chunky";
