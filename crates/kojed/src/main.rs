//! `kojed` – background service.
//!
//! Responsibilities (see the concept, section "Architektur"):
//! - follow the compositor's events (Hyprland, or GNOME Shell through koje's
//!   extension, ADR 0059; [`compositor`]) and assign each new window to its context
//!   (PID → cgroup → container label) – M4.4,
//! - set border colors per context – M4.5,
//! - apply layouts and keep autosaving layouts in sync,
//! - write the crash recovery file,
//! - watch memory usage and enforce soft/hard limits,
//! - serve `koje` and the UI over D-Bus (ADR 0027).
//!
//! One current-thread tokio runtime (ADR 0026).

mod api;
mod compositor;
mod daemon;
mod desktop;
mod displays;
mod gsettings;
mod overlays;
mod players;
mod procfs;
mod screens;
mod sound;
mod wayland;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use koje_core::api::{BUS_NAME, OBJECT_PATH};
use koje_core::paths::Paths;
use koje_core::ports::CompositorEvent;
use koje_hyprland::Hyprland;
use koje_podman::Podman;
use tracing_subscriber::EnvFilter;

/// What kojed sets in Hyprland with `keyword`: GNOME's input settings (ADR
/// 0044, M8.43), the overlays' keys (M7.1) and `[desktop]` (ADR 0056) from
/// `config.toml`. Notify the returned ones after Hyprland reset its keywords.
/// Nothing on GNOME: it applies its own settings (ADR 0059).
fn keywords(hyprland: bool, global_config: std::path::PathBuf) -> Vec<Arc<tokio::sync::Notify>> {
    if !hyprland {
        return Vec::new();
    }
    let input = Arc::new(tokio::sync::Notify::new());
    tokio::spawn(gsettings::run(Hyprland::from_env(), input.clone()));
    let overlays = Arc::new(tokio::sync::Notify::new());
    tokio::spawn(overlays::run(
        Hyprland::from_env(),
        global_config.clone(),
        overlays.clone(),
    ));
    let desktop = Arc::new(tokio::sync::Notify::new());
    tokio::spawn(desktop::run(
        Hyprland::from_env(),
        global_config,
        desktop.clone(),
    ));
    vec![input, overlays, desktop]
}

/// The compositor from the environment, and the contexts' Wayland sockets
/// for it.
async fn connect(runtime: &std::path::Path) -> Result<(compositor::Desktop, wayland::Sockets)> {
    let kind = compositor::select(
        std::env::var("KOJE_COMPOSITOR").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
    )
    .map_err(anyhow::Error::msg)?;
    tracing::info!(compositor = ?kind, "compositor chosen");
    Ok(match kind {
        compositor::Kind::Hyprland => (
            compositor::Desktop::Hyprland(Hyprland::from_env()),
            wayland::Sockets::start(runtime.to_owned(), || {
                Ok(koje_hyprland::instance::wayland_from_env()?)
            })?,
        ),
        compositor::Kind::Gnome => (
            compositor::Desktop::Gnome(
                koje_gnome::Gnome::session()
                    .await
                    .context("connecting to the session bus")?,
            ),
            wayland::Sockets::shared(runtime.to_owned(), wayland::display_from_env(runtime)),
        ),
    })
}

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
    let (desktop, sockets) = connect(&runtime).await?;
    let hyprland = matches!(desktop, compositor::Desktop::Hyprland(_));
    let mut events = desktop.events();
    let screens_paths = paths.clone();
    let global_config = paths.global_config();
    let mut daemon = daemon::Daemon::new(desktop, Podman::new(), paths);
    // The contexts before the first client asks (errors are logged; a
    // compositor that is not up yet gets its rules on `Connected`).
    if let Err(e) = daemon.reload().await {
        tracing::warn!("{e:#}");
    }
    let daemon: api::Shared = Arc::new(tokio::sync::Mutex::new(daemon));
    // The media keys' player (ADR 0041): kojed runs as the contexts' user.
    let uid = std::os::unix::fs::MetadataExt::uid(
        &std::fs::metadata("/proc/self").context("reading /proc/self")?,
    );
    let players = players::Players::new(uid);
    // GNOME Settings' Displays panel on Hyprland (ADR 0046); on GNOME, Mutter
    // serves it.
    let displays = hyprland
        .then(|| displays::Displays::new(Hyprland::from_env(), screens_paths.clone(), &runtime));
    let conn = zbus::connection::Builder::session()
        .context("connecting to the session bus")?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            api::Api {
                daemon: Arc::clone(&daemon),
                sockets: sockets.clone(),
                players: players.clone(),
                runtime: runtime.clone(),
                displays: displays.clone(),
            },
        )?
        .build()
        .await
        .with_context(|| {
            format!("owning {BUS_NAME} on the session bus (is kojed running already?)")
        })?;
    tracing::info!("serving {BUS_NAME}");
    if let Some(displays) = &displays {
        // GNOME Settings' Displays panel (ADR 0046, M8.51).
        displays.serve(&conn).await?;
        // GNOME's monitors.xml while Hyprland runs (ADR 0046, M8.52).
        tokio::spawn(screens::run(
            Hyprland::from_env(),
            screens_paths,
            runtime.clone(),
            Arc::clone(displays),
        ));
    }
    tokio::spawn(relink_sound(runtime));
    let reapply = keywords(hyprland, global_config);
    tokio::spawn(tick(
        Arc::clone(&daemon),
        sockets.clone(),
        players,
        conn.clone(),
    ));
    loop {
        let event = events.next().await;
        if event == CompositorEvent::Connected {
            // A new compositor (or kojed's start): new security contexts.
            sockets.reset();
        }
        if matches!(
            event,
            CompositorEvent::Connected | CompositorEvent::ConfigReloaded
        ) {
            // Both reset Hyprland's `keyword` settings.
            for r in &reapply {
                r.notify_one();
            }
        }
        // The bar (`koje bar`) follows contexts, the active one, window counts.
        let bar_relevant = !matches!(event, CompositorEvent::WindowTitle { .. });
        let (changes, blipped) = {
            let mut d = daemon.lock().await;
            (d.handle(event).await, d.take_blip())
        };
        if let Some(address) = blipped {
            tokio::spawn(unblip(Arc::clone(&daemon), address));
        }
        if bar_relevant && let Err(e) = api::contexts_changed(&conn).await {
            tracing::debug!("emitting ContextsChanged: {e}");
        }
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

/// How long a focused window's border flashes (M8.19).
const BLIP: std::time::Duration = std::time::Duration::from_millis(200);

async fn unblip(daemon: api::Shared, address: String) {
    tokio::time::sleep(BLIP).await;
    daemon.lock().await.unblip(&address).await;
}

/// How often kojed takes the live states (autosave, recovery file).
pub const TICK: std::time::Duration = std::time::Duration::from_secs(5);

/// M5.7, M5.8: autosave of geometry and the recovery files; M6: memory
/// limits (notify, shut down at the hard limit); the buses of contexts
/// started since for the media keys (ADR 0041).
async fn tick(
    daemon: api::Shared,
    sockets: wayland::Sockets,
    players: players::Players,
    conn: zbus::Connection,
) {
    use koje_core::memory::MemoryLevel;
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let (alerts, running) = {
            let mut d = daemon.lock().await;
            d.tick().await;
            (d.check_memory().await, d.running().await)
        };
        players.sync(&running).await;
        // The bar's memory meter (M8.21) follows the readings.
        if let Err(e) = api::memory_changed(&conn).await {
            tracing::debug!("emitting MemoryChanged: {e}");
        }
        for a in alerts {
            let (ctx, used, limit) = (&a.ctx, a.used.human(), a.limit);
            match a.level {
                MemoryLevel::Normal => {}
                MemoryLevel::NearSoft => {
                    notify_logged(
                        &conn,
                        &format!("{ctx} uses {used}"),
                        &format!("90 % of its soft limit {limit}"),
                    )
                    .await;
                }
                MemoryLevel::OverSoft => {
                    notify_logged(
                        &conn,
                        &format!("{ctx} uses {used}"),
                        &format!("over its soft limit {limit}"),
                    )
                    .await;
                }
                MemoryLevel::OverHard => {
                    tracing::warn!(context = %ctx, used, %limit, "hard limit reached, shutting down");
                    tokio::spawn(hard_stop(
                        Arc::clone(&daemon),
                        sockets.clone(),
                        conn.clone(),
                        a,
                    ));
                }
            }
        }
    }
}

/// How long programs get to end before a hard-limit stop (as `koje down`).
const HARD_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The hard limit (ADR 0035): save, end the programs in order, stop, notify.
async fn hard_stop(
    daemon: api::Shared,
    sockets: wayland::Sockets,
    conn: zbus::Connection,
    alert: daemon::MemoryAlert,
) {
    let ctx = &alert.ctx;
    let saved = daemon.lock().await.save_before_shutdown(ctx).await;
    if let Err(e) = daemon.lock().await.close_windows(ctx).await {
        tracing::warn!(context = %ctx, "closing windows: {e:#}");
    }
    let deadline = tokio::time::Instant::now() + HARD_STOP_WAIT;
    while tokio::time::Instant::now() < deadline
        && !daemon.lock().await.open_windows(ctx).is_empty()
    {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let stopped = daemon.lock().await.stop_container(ctx).await;
    sockets.release(ctx);
    let _ = api::contexts_changed(&conn).await;
    let saved = match &saved {
        Ok(state) => format!("windows saved into state {state}"),
        Err(e) => format!("windows not saved: {e:#}"),
    };
    let body = match stopped {
        Ok(()) => format!("{saved}; start it again with: koje up {ctx}"),
        Err(e) => format!("{saved}; stopping failed: {e:#}"),
    };
    notify_logged(
        &conn,
        &format!(
            "{ctx} reached its hard limit {} and was shut down",
            alert.limit
        ),
        &body,
    )
    .await;
}

async fn notify_logged(conn: &zbus::Connection, summary: &str, body: &str) {
    if let Err(e) = notify(conn, summary, body).await {
        tracing::warn!("notifying ({summary}): {e}");
    }
}

/// A desktop notification on the session bus.
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
