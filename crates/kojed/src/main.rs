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
mod stale;
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
            },
        )?
        .build()
        .await
        .with_context(|| {
            format!("owning {BUS_NAME} on the session bus (is kojed running already?)")
        })?;
    tracing::info!("serving {BUS_NAME}");
    tokio::spawn(watch_sound(runtime, Arc::clone(&daemon), conn.clone()));
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

/// M4.11: tell the user when the sound sockets were replaced under running
/// contexts.
async fn watch_sound(runtime: std::path::PathBuf, daemon: api::Shared, conn: zbus::Connection) {
    let sockets = stale::sound_sockets(&runtime);
    let mut watch = stale::Watch::default();
    let mut tick = tokio::time::interval(stale::INTERVAL);
    loop {
        tick.tick().await;
        let replaced = watch.update(sockets.iter().map(|p| (p.clone(), stale::identity(p))));
        if replaced.is_empty() {
            continue;
        }
        tracing::info!(?replaced, "sound sockets replaced");
        let affected = match daemon.lock().await.running_with_sound().await {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("finding the contexts with sound: {e:#}");
                continue;
            }
        };
        if let Some((summary, body)) = stale::message(&affected)
            && let Err(e) = notify(&conn, &summary, &body).await
        {
            tracing::warn!("notifying: {e}");
        }
    }
}

async fn notify(conn: &zbus::Connection, summary: &str, body: &str) -> zbus::Result<()> {
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
        std::collections::HashMap::new();
    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "koje",
            0u32,
            "",
            summary,
            body,
            Vec::<&str>::new(),
            hints,
            -1i32,
        ),
    )
    .await
    .map(drop)
}
