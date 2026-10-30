//! What the runtime needs from kojed while it drives a context (G.34):
//! the desktop side of `up`, `down`, `save`, `sleep` and `recreate`. kojed
//! implements it on its own state; tests use fakes.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use koje_core::name::{ContextName, StateName};
use koje_core::state::State;

pub trait Host: Sync {
    /// The context started: its windows are expected now.
    fn started(&self, context: &ContextName) -> Result<()>;
    /// The context stopped.
    fn stopped(&self, context: &ContextName);
    /// Show the context's workspace.
    fn focus(&self, context: &ContextName) -> Result<()>;
    /// The context's Wayland socket (ADR 0022), made if needed; its
    /// directory is mounted.
    fn wayland_socket(&self, context: &ContextName) -> Result<PathBuf>;
    /// The context's directory of links to the host's sound sockets (ADR 0029).
    fn sound_sockets(&self, context: &ContextName) -> Result<PathBuf>;
    /// Stop serving the context's Wayland socket.
    fn release_wayland_socket(&self, context: &ContextName);
    /// The windows of a running context now (ADR 0030).
    fn live_state(&self, context: &ContextName) -> Result<State>;
    /// Start the windows of `state` in a context that just started; the
    /// number of programs started. `name`: the state it is then in.
    fn restore(
        &self,
        context: &ContextName,
        state: State,
        name: Option<StateName>,
    ) -> Result<usize>;
    /// Switch a running context to `state` (M5.9): `(kept, closed, started)`.
    fn switch_state(
        &self,
        context: &ContextName,
        state: State,
        name: Option<StateName>,
    ) -> Result<(usize, usize, usize)>;
    /// The state a running context is in (ADR 0032); `None` for `empty`.
    fn current_state(&self, context: &ContextName) -> Option<StateName>;
    /// Whether a running context lost its windows (the session crashed).
    fn windows_lost(&self, context: &ContextName) -> bool;
    /// End the context's programs in order and wait up to `timeout` for
    /// their windows to go (M5.4); the windows still open.
    fn close_windows(&self, context: &ContextName, timeout: Duration) -> Result<Vec<String>>;
    /// Before the context sleeps: its windows covered (ADR 0059).
    fn hide(&self, context: &ContextName) -> Result<()>;
}
