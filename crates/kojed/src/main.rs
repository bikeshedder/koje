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

mod api;
mod daemon;
mod procfs;
mod sound;
mod wayland;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use koje_core::api::{BUS_NAME, OBJECT_PATH};
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
    let runtime = std::path::PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?,
    );
    let sockets = wayland::Sockets::start(runtime.clone(), || {
        Ok(koje_hyprland::instance::wayland_from_env()?)
    })?;
    let hyprland = Hyprland::from_env();
    let mut events = hyprland.events();
    let mut daemon = daemon::Daemon::new(hyprland, Podman::new(), paths);
    // The contexts before the first client asks (errors are logged; a
    // compositor that is not up yet gets its rules on `Connected`).
    if let Err(e) = daemon.reload().await {
        tracing::warn!("{e:#}");
    }
    let daemon: api::Shared = Arc::new(tokio::sync::Mutex::new(daemon));
    let conn = zbus::connection::Builder::session()
        .context("connecting to the session bus")?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            api::Api {
                daemon: Arc::clone(&daemon),
                sockets: sockets.clone(),
                runtime: runtime.clone(),
            },
        )?
        .build()
        .await
        .with_context(|| {
            format!("owning {BUS_NAME} on the session bus (is kojed running already?)")
        })?;
    tracing::info!("serving {BUS_NAME}");
    tokio::spawn(relink_sound(runtime));
    loop {
        let event = events.next().await;
        if event == koje_core::ports::CompositorEvent::Connected {
            // A new compositor (or kojed's start): new security contexts.
            sockets.reset();
        }
        let changes = daemon.lock().await.handle(event).await;
        match changes {
            Ok(changes) => {
                for change in &changes {
                    if let Err(e) = api::emit(&conn, change).await {
                        tracing::warn!("emitting a signal: {e}");
                    }
                }
            }
            Err(e) => tracing::warn!("{e:#}"),
        }
    }
}

/// M4.11 (ADR 0029): keep the contexts' sound links on the current sockets.
async fn relink_sound(runtime: std::path::PathBuf) {
    let mut tick = tokio::time::interval(sound::INTERVAL);
    loop {
        tick.tick().await;
        for dir in sound::dirs(&runtime) {
            match sound::sync(&runtime, &dir) {
                Ok(made) if !made.is_empty() => {
                    tracing::info!(?made, "sound sockets replaced, links updated");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(dir = %dir.display(), "updating sound links: {e}"),
            }
        }
    }
}
