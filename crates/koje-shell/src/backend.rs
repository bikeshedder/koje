//! The shell's connections to kojed and Hyprland (ADR 0042). They run on a
//! tokio runtime in their own thread, so nothing blocks GTK's main loop, and
//! send what they learn as [`Update`]s over a channel. Every call has a
//! timeout.

use std::time::Duration;

use anyhow::{Context as _, Result};
use async_channel::Sender;
use futures_util::StreamExt as _;
use koje_core::ports::CompositorEvent;
use koje_hyprland::Hyprland;

use crate::model::{RunningContext, Update};

const TIMEOUT: Duration = Duration::from_secs(2);
/// Wait before connecting to the session bus again.
const RETRY: Duration = Duration::from_secs(2);

#[zbus::proxy(
    interface = "io.koje.Daemon1",
    default_service = "io.koje.Daemon",
    default_path = "/io/koje/Daemon"
)]
trait Daemon {
    fn running_contexts(&self) -> zbus::Result<Vec<(String, String, u32, bool)>>;

    #[zbus(signal)]
    fn contexts_changed(&self) -> zbus::Result<()>;
}

/// Start the backend thread. It ends when the receiving side is dropped.
pub fn spawn(tx: Sender<Update>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the shell's backend runtime")?;
    std::thread::Builder::new()
        .name("backend".into())
        .spawn(move || {
            runtime.block_on(async {
                tokio::select! {
                    () = hyprland(tx.clone()) => {}
                    () = kojed_forever(tx) => {}
                }
            });
        })
        .context("starting the shell's backend thread")?;
    Ok(())
}

/// Follow Hyprland's events; the reader reconnects by itself.
async fn hyprland(tx: Sender<Update>) {
    let hypr = Hyprland::from_env();
    let mut events = hypr.events();
    loop {
        let update = match events.next().await {
            CompositorEvent::Connected => {
                match tokio::time::timeout(TIMEOUT, hypr.active_workspace()).await {
                    Ok(Ok(ws)) => Update::Workspace(ws.name),
                    Ok(Err(e)) => {
                        tracing::warn!("asking Hyprland for the active workspace: {e}");
                        continue;
                    }
                    Err(_) => {
                        tracing::warn!("Hyprland did not answer j/activeworkspace");
                        continue;
                    }
                }
            }
            CompositorEvent::WorkspaceFocused { name } => Update::Workspace(name),
            _ => continue,
        };
        if tx.send(update).await.is_err() {
            return;
        }
    }
}

/// Follow kojed; a lost session bus is retried.
async fn kojed_forever(tx: Sender<Update>) {
    loop {
        if let Err(e) = kojed(&tx).await {
            tracing::warn!("following kojed: {e:#}");
        }
        if tx.is_closed() {
            return;
        }
        tokio::time::sleep(RETRY).await;
    }
}

/// Follow kojed: its contexts on start and on every `ContextsChanged`, and
/// whether it runs at all (its bus name's owner).
async fn kojed(tx: &Sender<Update>) -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .context("connecting to the session bus")?;
    let daemon = DaemonProxy::builder(&conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let mut changed = daemon.receive_contexts_changed().await?;
    let mut owner = daemon.inner().receive_owner_changed().await?;
    refresh(&daemon, tx).await;
    loop {
        tokio::select! {
            Some(_) = changed.next() => refresh(&daemon, tx).await,
            Some(new_owner) = owner.next() => {
                if new_owner.is_some() {
                    refresh(&daemon, tx).await;
                } else {
                    let _ = tx.send(Update::KojedGone).await;
                }
            }
            else => return Ok(()),
        }
        if tx.is_closed() {
            return Ok(());
        }
    }
}

/// Ask kojed for the running contexts.
async fn refresh(daemon: &DaemonProxy<'_>, tx: &Sender<Update>) {
    let update = match tokio::time::timeout(TIMEOUT, daemon.running_contexts()).await {
        Ok(Ok(contexts)) => {
            Update::Contexts(contexts.into_iter().map(RunningContext::from).collect())
        }
        Ok(Err(zbus::Error::MethodError(name, ..)))
            if name == "org.freedesktop.DBus.Error.ServiceUnknown"
                || name == "org.freedesktop.DBus.Error.NameHasNoOwner" =>
        {
            Update::KojedGone
        }
        Ok(Err(e)) => {
            tracing::warn!("kojed: RunningContexts: {e}");
            return;
        }
        Err(_) => {
            tracing::warn!("kojed did not answer RunningContexts within {TIMEOUT:?}");
            return;
        }
    };
    let _ = tx.send(update).await;
}
