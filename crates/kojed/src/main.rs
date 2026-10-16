//! `kojed` – background service.
//!
//! Responsibilities (see the concept, section "Architektur"):
//! - follow Hyprland's event socket and assign each new window to its context
//!   (PID → cgroup → container label),
//! - set border colors per context,
//! - apply layouts and keep autosaving layouts in sync,
//! - write the crash recovery file,
//! - watch memory usage and enforce soft/hard limits,
//! - serve `koje` and the UI over D-Bus or a Unix socket.
//!
//! Nothing of this exists yet; the first real code arrives with milestone M4.
//! Whether the service is async (tokio) or thread-based is still open.

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match koje_core::paths::Paths::from_env() {
        Ok(paths) => tracing::info!(config = %paths.global_config().display(), "kojed starting"),
        Err(err) => {
            tracing::error!("{err}");
            return ExitCode::FAILURE;
        }
    }

    tracing::error!("kojed is not implemented yet – planned for milestone M4 (see docs/plan/)");
    ExitCode::FAILURE
}
