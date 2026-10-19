//! `kojed` – background service.
//!
//! Responsibilities (see the concept, section "Architektur"):
//! - follow Hyprland's event socket and assign each new window to its context
//!   (PID → cgroup → container label) – M4.4,
//! - set border colors per context – M4.5,
//! - apply layouts and keep autosaving layouts in sync,
//! - write the crash recovery file,
//! - watch memory usage and enforce soft/hard limits,
//! - serve `koje` and the UI over D-Bus (ADR 0027).
//!
//! One current-thread tokio runtime (ADR 0026).

mod daemon;

use anyhow::{Context as _, Result};
use koje_core::paths::Paths;
use koje_hyprland::Hyprland;
use koje_podman::Podman;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let paths = Paths::from_env().context("finding koje's directories")?;
    tracing::info!(config = %paths.global_config().display(), "kojed starting");
    let hyprland = Hyprland::from_env();
    let mut events = hyprland.events();
    let mut daemon = daemon::Daemon::new(hyprland, Podman::new(), paths);
    loop {
        let event = events.next().await;
        if let Err(e) = daemon.handle(event).await {
            tracing::warn!("{e:#}");
        }
    }
}
