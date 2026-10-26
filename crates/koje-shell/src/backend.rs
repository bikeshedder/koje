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
use crate::menu::HistoryReading;
use crate::model::{Action, MemoryReading, RunningContext, Update};

const TIMEOUT: Duration = Duration::from_secs(2);
/// Events come in bursts (a workspace switch focuses a window too): read the
/// desktop once after this quiet time.
const SETTLE: Duration = Duration::from_millis(20);
/// Wait before connecting to the session bus again.
const RETRY: Duration = Duration::from_secs(2);
/// `koje limit` reads and writes a few files and may set a kernel limit.
const LIMIT_TIMEOUT: Duration = Duration::from_secs(10);

#[zbus::proxy(
    interface = "io.koje.Daemon1",
    default_service = "io.koje.Daemon",
    default_path = "/io/koje/Daemon"
)]
trait Daemon {
    fn running_contexts(&self) -> zbus::Result<Vec<(String, String, u32, bool)>>;

    fn memory(&self, context: &str) -> zbus::Result<(String, u64, u64, u64, u64)>;

    fn memory_history(&self, context: &str) -> zbus::Result<(String, u32, Vec<u64>)>;

    fn focus(&self, context: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn contexts_changed(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn memory_changed(&self) -> zbus::Result<()>;
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
                    () = act(&hypr, actions, tx.clone()) => {}
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
async fn act(hypr: &Hyprland, mut actions: UnboundedReceiver<Action>, tx: Sender<Update>) {
    while let Some(action) = actions.recv().await {
        tracing::debug!(?action, "click");
        if let Some(command) = action.command() {
            match tokio::time::timeout(TIMEOUT, hypr.apply(&[command])).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("Hyprland: {e}"),
                Err(_) => tracing::warn!("Hyprland did not answer within {TIMEOUT:?}"),
            }
            continue;
        }
        match action {
            Action::FocusContext(context) => {
                if let Err(e) = focus(&context).await {
                    tracing::warn!("kojed: Focus {context}: {e:#}");
                }
            }
            Action::SetLimits {
                context,
                soft,
                hard,
            } => {
                let error = set_limits(crate::KOJE, &context, &soft, &hard).await.err();
                let _ = tx.send(Update::LimitsChanged { context, error }).await;
            }
            Action::FocusWorkspace(_) | Action::FocusWindow(_) => {}
        }
    }
    // The GTK side is gone; keep the others running until it says so.
    std::future::pending::<()>().await;
}

/// Show a context's workspace through kojed.
async fn focus(context: &str) -> Result<()> {
    let call = async {
        let conn = zbus::Connection::session().await?;
        DaemonProxy::new(&conn).await?.focus(context).await?;
        anyhow::Ok(())
    };
    tokio::time::timeout(TIMEOUT, call)
        .await
        .context("kojed did not answer")?
}

/// `koje limit <context> --soft … --hard …`: it checks the values against
/// `config.toml`, keeps the file's comments, sets the kernel limit of a
/// running context and tells kojed. The error is its message.
async fn set_limits(koje: &str, context: &str, soft: &str, hard: &str) -> Result<(), String> {
    let run = tokio::process::Command::new(koje)
        .args(["limit", context, "--soft", soft, "--hard", hard])
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(LIMIT_TIMEOUT, run).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("running {koje}: {e}")),
        Err(_) => {
            return Err(format!(
                "koje limit did not finish within {LIMIT_TIMEOUT:?}"
            ));
        }
    };
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim().trim_start_matches("Error: ");
    Err(if err.is_empty() {
        format!("koje limit failed ({})", out.status)
    } else {
        err.to_owned()
    })
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

/// Follow kojed: its contexts on start and on every `ContextsChanged`, their
/// memory on every `MemoryChanged`, and whether it runs at all (its bus
/// name's owner).
async fn kojed(tx: &Sender<Update>) -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .context("connecting to the session bus")?;
    let daemon = DaemonProxy::builder(&conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let mut changed = daemon.receive_contexts_changed().await?;
    let mut readings = daemon.receive_memory_changed().await?;
    let mut owner = daemon.inner().receive_owner_changed().await?;
    let mut running = refresh(&daemon, tx, Vec::new()).await;
    loop {
        tokio::select! {
            Some(_) = changed.next() => running = refresh(&daemon, tx, running).await,
            Some(_) = readings.next() => memory(&daemon, tx, &running).await,
            Some(new_owner) = owner.next() => {
                if new_owner.is_some() {
                    running = refresh(&daemon, tx, running).await;
                } else {
                    running.clear();
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

/// Ask kojed for the running contexts, then for their memory (another
/// context may be active now); the running contexts' names, else `old`.
async fn refresh(daemon: &DaemonProxy<'_>, tx: &Sender<Update>, old: Vec<String>) -> Vec<String> {
    match contexts(daemon, tx).await {
        Some(running) => {
            memory(daemon, tx, &running).await;
            running
        }
        None => old,
    }
}

/// The memory of every running context and the active one's history.
async fn memory(daemon: &DaemonProxy<'_>, tx: &Sender<Update>, running: &[String]) {
    let mut readings = Vec::new();
    for name in running {
        match tokio::time::timeout(TIMEOUT, daemon.memory(name)).await {
            Ok(Ok(reading)) => readings.push(MemoryReading::from(reading)),
            Ok(Err(e)) => tracing::debug!("kojed: Memory {name}: {e}"),
            Err(_) => tracing::warn!("kojed did not answer Memory within {TIMEOUT:?}"),
        }
    }
    let _ = tx.send(Update::Memory(readings)).await;
    match tokio::time::timeout(TIMEOUT, daemon.memory_history("")).await {
        Ok(Ok(history)) => {
            let _ = tx
                .send(Update::History(HistoryReading::from(history)))
                .await;
        }
        Ok(Err(e)) => tracing::debug!("kojed: MemoryHistory: {e}"),
        Err(_) => tracing::warn!("kojed did not answer MemoryHistory within {TIMEOUT:?}"),
    }
}

/// Ask kojed for the running contexts: their names, `None` if it did not
/// answer.
async fn contexts(daemon: &DaemonProxy<'_>, tx: &Sender<Update>) -> Option<Vec<String>> {
    let (update, running) = match tokio::time::timeout(TIMEOUT, daemon.running_contexts()).await {
        Ok(Ok(contexts)) => {
            let contexts: Vec<RunningContext> =
                contexts.into_iter().map(RunningContext::from).collect();
            let names = contexts.iter().map(|c| c.name.clone()).collect();
            (Update::Contexts(contexts), Some(names))
        }
        Ok(Err(zbus::Error::MethodError(name, ..)))
            if name == "org.freedesktop.DBus.Error.ServiceUnknown"
                || name == "org.freedesktop.DBus.Error.NameHasNoOwner" =>
        {
            (Update::KojedGone, None)
        }
        Ok(Err(e)) => {
            tracing::warn!("kojed: RunningContexts: {e}");
            return None;
        }
        Err(_) => {
            tracing::warn!("kojed did not answer RunningContexts within {TIMEOUT:?}");
            return None;
        }
    };
    let _ = tx.send(update).await;
    running
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake `koje` that logs its arguments and answers as `script` says.
    /// Written by a child process, so no fd of this test process points at
    /// it while other tests fork ("text file busy").
    fn fake_koje(dir: &std::path::Path, name: &str, script: &str) -> String {
        let path = dir.join(name);
        let body = format!(
            "#!/bin/sh\necho \"$@\" > {}/args\n{script}\n",
            dir.display()
        );
        let ok = std::process::Command::new("sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .map(|mut i| i.write_all(body.as_bytes()))
                    .transpose()?;
                child.wait()
            })
            .unwrap();
        assert!(ok.success());
        path.display().to_string()
    }

    #[tokio::test]
    async fn limits_go_to_koje_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let koje = fake_koje(tmp.path(), "koje", "exit 0");
        assert_eq!(set_limits(&koje, "kp", "6G", "none").await, Ok(()));
        let args = std::fs::read_to_string(tmp.path().join("args")).unwrap();
        assert_eq!(args.trim(), "limit kp --soft 6G --hard none");
    }

    #[tokio::test]
    async fn koje_limits_error_is_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let koje = fake_koje(
            tmp.path(),
            "koje",
            "echo 'Error: the hard limit 2G is below the soft limit 6G' >&2; exit 1",
        );
        assert_eq!(
            set_limits(&koje, "kp", "6G", "2G").await,
            Err("the hard limit 2G is below the soft limit 6G".into())
        );
        let silent = fake_koje(tmp.path(), "silent", "exit 3");
        assert!(
            set_limits(&silent, "kp", "6G", "2G")
                .await
                .unwrap_err()
                .starts_with("koje limit failed")
        );
        assert!(
            set_limits("/nonexistent/koje", "kp", "6G", "2G")
                .await
                .unwrap_err()
                .starts_with("running /nonexistent/koje")
        );
    }
}
