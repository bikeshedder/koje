//! Driving context containers (M3, M5): `up`, `down`, `save`, `sleep`,
//! `wake`, `recreate`, `shell`, `run`, `open`, live mounts, dotfiles, the
//! host bus and memory limits. Shared by the CLI and kojed (ADR 0060);
//! decisions come from `koje_core`, this crate gathers inputs and executes.

pub mod dotfiles;
pub mod host;
pub mod hostbus;
pub mod kojed;
pub mod memory;
pub mod mounts;
pub mod prompt;
pub mod runtime;
pub mod say;
