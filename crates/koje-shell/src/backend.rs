//! The shell's connections to kojed and Hyprland (ADR 0042). They run on a
//! tokio runtime in their own thread, so nothing blocks GTK's main loop, and
//! send what they learn as [`Update`]s over a channel. Every call has a
//! timeout.

use std::time::Duration;

use anyhow::{Context as _, Result};
use async_channel::Sender;
use futures_util::StreamExt as _;
use koje_core::ports::Compositor as _;
use koje_hyprland::Hyprland;
use koje_hyprland::events::Line;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::hypr;
use crate::model::{Action, RunningContext, Update};

const TIMEOUT: Duration = Duration::from_secs(2);
/// Events come in bursts (a workspace switch focuses a window too): read the
/// desktop once after this quiet time.
const SETTLE: Duration = Duration::from_millis(20);
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

/// Start the backend thread: updates go to `tx`, clicks come from
/// `actions`. It ends when the receiving side of `tx` is dropped.
pub fn spawn(tx: Sender<Update>, actions: UnboundedReceiver<Action>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the shell's backend runtime")?;
    std::thread::Builder::new()
        .name("backend".into())
        .spawn(move || {
            runtime.block_on(async {
                let hypr = Hyprland::from_env();
                tokio::select! {
                    () = desktop(&hypr, tx.clone()) => {}
                    () = act(&hypr, actions) => {}
                    () = kojed_forever(tx) => {}
                }
            });
        })
        .context("starting the shell's backend thread")?;
    Ok(())
}

/// Follow Hyprland's events and send the desktop after each burst that may
/// change it; the reader reconnects by itself.
async fn desktop(hypr: &Hyprland, tx: Sender<Update>) {
    let mut events = hypr.events();
    loop {
        let changed = match events.next_line().await {
            Line::Connected => true,
            Line::Event(line) => hypr::changes_desktop(&line),
        };
        if !changed {
            continue;
        }
        // Let the rest of the burst arrive; a reconnect in it is no event.
        while let Ok(line) = tokio::time::timeout(SETTLE, events.next_line()).await {
            tracing::trace!(?line, "in the same burst");
        }
        match tokio::time::timeout(TIMEOUT, hypr::desktop(hypr)).await {
            Ok(Ok(d)) => {
                if tx.send(Update::Desktop(d)).await.is_err() {
                    return;
                }
            }
            Ok(Err(e)) => tracing::warn!("reading the desktop from Hyprland: {e:#}"),
            Err(_) => tracing::warn!("Hyprland did not answer within {TIMEOUT:?}"),
        }
    }
}

/// Carry out what clicks ask for.
async fn act(hypr: &Hyprland, mut actions: UnboundedReceiver<Action>) {
    while let Some(action) = actions.recv().await {
        tracing::debug!(?action, "click");
        match tokio::time::timeout(TIMEOUT, hypr.apply(&[action.command()])).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("Hyprland: {e}"),
            Err(_) => tracing::warn!("Hyprland did not answer within {TIMEOUT:?}"),
        }
    }
    // The GTK side is gone; keep the others running until it says so.
    std::future::pending::<()>().await;
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
