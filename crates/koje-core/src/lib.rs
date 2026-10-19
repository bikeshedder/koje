//! Domain model and pure logic for `koje`.
//!
//! This crate must stay free of I/O against Hyprland or Podman. Everything that
//! talks to the outside world goes through the traits in [`ports`], so the
//! logic here can be unit-tested with fakes.
//!
//! Terminology (see `AGENTS.md`): a **context** is the unit koje manages; a
//! **state** is a named snapshot of a context's windows and layouts; a
//! **layout** is a named arrangement of a context's windows.

pub mod access;
pub mod color;
pub mod config;
pub mod container;
pub mod context;
pub mod error;
pub mod fsutil;
pub mod hostbus;
pub mod layout;
pub mod layouts;
pub mod lifecycle;
pub mod memory;
pub mod mount;
pub mod name;
pub mod open;
pub mod paths;
pub mod ports;
pub mod recovery;
pub mod size;
pub mod spec;
pub mod state;
pub mod states;

pub use error::{Error, Result};
