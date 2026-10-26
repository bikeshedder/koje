//! The shell's connections to kojed and Hyprland (ADR 0042). They run on a
//! tokio runtime in their own thread, so nothing blocks GTK's main loop, and
//! send what they learn as [`Update`]s over a channel. Every call has a
//! timeout.

use std::sync::Arc;
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
use crate::model::{Action, ContextEntry, MemoryReading, RunningContext, Update};
use crate::system;

const TIMEOUT: Duration = Duration::from_secs(2);
/// Events come in bursts (a workspace switch focuses a window too): read the
/// desktop once after this quiet time.
const SETTLE: Duration = Duration::from_millis(20);
/// Wait before connecting to the session bus again.
const RETRY: Duration = Duration::from_secs(2);
/// How long a `koje` command may take: `up` may start many programs,
/// `down` waits for programs that ask to save.
const KOJE_TIMEOUT: Duration = Duration::from_secs(300);

#[zbus::proxy(
    interface = "io.koje.Daemon1",
    default_service = "io.koje.Daemon",
    default_path = "/io/koje/Daemon"
)]
pub trait Daemon {
    fn running_contexts(&self) -> zbus::Result<Vec<(String, String, u32, bool)>>;

    fn contexts(&self) -> zbus::Result<Vec<(String, String, String)>>;

    fn memory(&self, context: &str) -> zbus::Result<(String, u64, u64, u64, u64)>;

    fn memory_history(&self, context: &str) -> zbus::Result<(String, u32, Vec<u64>)>;

    fn focus(&self, context: &str) -> zbus::Result<()>;

    fn unstash(&self, context: &str, window: &str, to: &str) -> zbus::Result<String>;

    fn keep_display_change(&self) -> zbus::Result<bool>;

    fn revert_display_change(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn display_change_pending(&self, seconds: u32) -> zbus::Result<()>;

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
                // NetworkManager, UPower, logind; Quick Settings leaves them
                // out without it.
                let system_bus = match zbus::Connection::system().await {
                    Ok(c) => Some(c),
                    Err(e) => {
                        tracing::warn!("no system bus: {e}");
                        None
                    }
                };
                let wake = Arc::new(tokio::sync::Notify::new());
                tokio::select! {
                    () = desktop(&hypr, tx.clone()) => {}
                    () = act(&hypr, system_bus.as_ref(), &wake, actions, tx.clone()) => {}
                    () = system::watch(&hypr, system_bus.clone(), tx.clone(), wake.clone()) => {}
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
            Line::Event(line) => {
                if let Some(monitor) = hypr::overview_landed(&line) {
                    let _ = tx.send(Update::OverviewLanded(monitor.to_owned())).await;
                }
                hypr::changes_desktop(&line)
            }
        };
        if !changed {
            continue;
        }
        // Let the rest of the burst arrive; a reconnect in it is no event.
        while let Ok(line) = tokio::time::timeout(SETTLE, events.next_line()).await {
            tracing::trace!(?line, "in the same burst");
            if let Line::Event(line) = &line
                && let Some(monitor) = hypr::overview_landed(line)
            {
                let _ = tx.send(Update::OverviewLanded(monitor.to_owned())).await;
            }
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

/// The contexts' programs (`koje apps --json`) and their favorites.
async fn refresh_apps(tx: Sender<Update>, contexts: Vec<String>) {
    for context in contexts {
        send_favorites(&tx, &context).await;
        let args = ["apps", "-c", &context, "--json"].map(str::to_owned);
        match koje_output(crate::KOJE, &args).await {
            Ok(out) => match serde_json::from_str(&out) {
                Ok(apps) => {
                    let _ = tx.send(Update::Apps { context, apps }).await;
                }
                Err(e) => tracing::warn!("koje apps -c {context} --json: {e}"),
            },
            Err(e) => tracing::debug!("koje apps -c {context}: {e}"),
        }
    }
}

/// `koje favorites --set`, then the favorites as written.
async fn set_favorites(tx: Sender<Update>, context: String, ids: Vec<String>) {
    let mut args = vec!["favorites".to_owned(), "-c".to_owned(), context.clone()];
    args.push("--set".to_owned());
    args.extend(ids);
    if let Err(e) = run_koje(crate::KOJE, &args).await {
        tracing::warn!("koje favorites -c {context}: {e}");
    }
    send_favorites(&tx, &context).await;
}

/// A context's own favorites for the dash, as its `context.toml` has them
/// (ADR 0053).
async fn send_favorites(tx: &Sender<Update>, context: &str) {
    let read = || -> anyhow::Result<Option<Vec<String>>> {
        let paths = koje_core::paths::Paths::from_env()?;
        let name = koje_core::name::ContextName::new(context)?;
        Ok(koje_core::context::load(&paths, &name)?.favorites)
    };
    match read() {
        Ok(favorites) => {
            let context = context.to_owned();
            let _ = tx
                .send(Update::ContextFavorites { context, favorites })
                .await;
        }
        Err(e) => tracing::warn!("the favorites of {context}: {e:#}"),
    }
}

/// Carry out what clicks ask for.
async fn act(
    hypr: &Hyprland,
    system_bus: Option<&zbus::Connection>,
    wake: &tokio::sync::Notify,
    mut actions: UnboundedReceiver<Action>,
    tx: Sender<Update>,
) {
    while let Some(action) = actions.recv().await {
        tracing::debug!(?action, "click");
        if let Action::Quick(change) = action {
            if let Some(osd) = system::change(hypr, system_bus, change).await {
                let _ = tx.send(Update::Osd(osd)).await;
            }
            wake.notify_one();
            continue;
        }
        if let Some(command) = action.command() {
            match tokio::time::timeout(TIMEOUT, hypr.apply(&[command])).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("Hyprland: {e}"),
                Err(_) => tracing::warn!("Hyprland did not answer within {TIMEOUT:?}"),
            }
            continue;
        }
        if let Some((context, busy, commands)) = action.koje() {
            // Its own task: a `koje down` that waits for programs must not
            // hold up other clicks.
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx
                    .send(Update::Busy {
                        context: context.clone(),
                        busy,
                    })
                    .await;
                let mut error = None;
                for args in &commands {
                    if let Err(e) = run_koje(crate::KOJE, args).await {
                        error = Some(e);
                        break;
                    }
                }
                let _ = tx.send(Update::Done { context, error }).await;
            });
            continue;
        }
        match action {
            Action::FocusContext(context) => {
                if let Err(e) = focus(&context).await {
                    tracing::warn!("kojed: Focus {context}: {e:#}");
                }
            }
            Action::Overview(args) => {
                let request = format!("dispatch koje:overview {args}");
                match tokio::time::timeout(TIMEOUT, hypr.request(&request)).await {
                    Ok(Ok(reply)) if reply.trim() == "ok" => {}
                    Ok(Ok(reply)) => tracing::warn!("koje:overview: {}", reply.trim()),
                    Ok(Err(e)) => tracing::warn!("koje:overview: {e}"),
                    Err(_) => tracing::warn!("Hyprland did not answer within {TIMEOUT:?}"),
                }
            }
            // Their own tasks: `koje` may take a while.
            Action::RefreshApps(contexts) => {
                tokio::spawn(refresh_apps(tx.clone(), contexts));
            }
            Action::SetFavorites { context, ids } => {
                tokio::spawn(set_favorites(tx.clone(), context, ids));
            }
            Action::Launch { context, id } => {
                // Its own task: starting may take a while.
                tokio::spawn(async move {
                    let args = ["apps", "-c", &context, "--start", &id].map(str::to_owned);
                    if let Err(e) = run_koje(crate::KOJE, &args).await {
                        tracing::warn!("starting {id} in {context}: {e}");
                    }
                });
            }
            Action::Unstash { context, address } => {
                if let Err(e) = unstash(&context, &address).await {
                    tracing::warn!("kojed: Unstash {address}: {e:#}");
                }
            }
            Action::EndSession(e) => end_session(hypr, e).await,
            Action::Suspend => run_quietly("systemctl", &["suspend"]).await,
            Action::KeepDisplayChange | Action::RevertDisplayChange => {
                let keep = action == Action::KeepDisplayChange;
                if let Err(e) = display_change(keep).await {
                    tracing::warn!("kojed: display change: {e:#}");
                }
            }
            _ => {}
        }
    }
    // The GTK side is gone; keep the others running until it says so.
    std::future::pending::<()>().await;
}

/// A stashed window back into its context's active layout, through kojed.
async fn unstash(context: &str, address: &str) -> Result<()> {
    let call = async {
        let conn = zbus::Connection::session().await?;
        DaemonProxy::new(&conn)
            .await?
            .unstash(context, address, "")
            .await?;
        anyhow::Ok(())
    };
    tokio::time::timeout(TIMEOUT, call)
        .await
        .context("kojed did not answer")?
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

/// Log out (Hyprland exits; `koje session start` cleans up), restart or
/// power off (M8.47).
async fn end_session(hypr: &Hyprland, e: crate::end_session::EndSession) {
    match e.command() {
        Some((program, args)) => run_quietly(program, &args).await,
        None => {
            if let Err(e) = hypr.request("dispatch exit").await {
                tracing::warn!("logging out: {e}");
            }
        }
    }
}

async fn run_quietly(program: &str, args: &[&str]) {
    match tokio::process::Command::new(program)
        .args(args)
        .status()
        .await
    {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!("{program} {}: {s}", args.join(" ")),
        Err(e) => tracing::warn!("{program}: {e}"),
    }
}

/// Keep or undo the display change GNOME Settings made (M8.53).
async fn display_change(keep: bool) -> Result<()> {
    let call = async {
        let conn = zbus::Connection::session().await?;
        let daemon = DaemonProxy::new(&conn).await?;
        if keep {
            daemon.keep_display_change().await?;
        } else {
            daemon.revert_display_change().await?;
        }
        anyhow::Ok(())
    };
    tokio::time::timeout(TIMEOUT, call)
        .await
        .context("kojed did not answer")?
}

/// Run `koje <args>` (`koje limit`, `up`, `down`, …): the CLI checks, writes
/// and tells kojed, as when typed. The error is its message.
async fn run_koje(koje: &str, args: &[String]) -> Result<(), String> {
    koje_output(koje, args).await.map(drop)
}

/// [`run_koje`], with what it printed.
async fn koje_output(koje: &str, args: &[String]) -> Result<String, String> {
    let run = tokio::process::Command::new(koje)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(KOJE_TIMEOUT, run).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("running {koje}: {e}")),
        Err(_) => {
            return Err(format!(
                "koje {} did not finish within {KOJE_TIMEOUT:?}",
                args.first().map_or("", String::as_str)
            ));
        }
    };
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim().trim_start_matches("Error: ");
    Err(if err.is_empty() {
        format!(
            "koje {} failed ({})",
            args.first().map_or("", String::as_str),
            out.status
        )
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
    let mut displays = daemon.receive_display_change_pending().await?;
    let mut owner = daemon.inner().receive_owner_changed().await?;
    let mut running = refresh(&daemon, tx, Vec::new()).await;
    loop {
        tokio::select! {
            Some(_) = changed.next() => running = refresh(&daemon, tx, running).await,
            Some(_) = readings.next() => memory(&daemon, tx, &running).await,
            Some(signal) = displays.next() => {
                if let Ok(args) = signal.args() {
                    let _ = tx.send(Update::DisplayChangePending(args.seconds)).await;
                }
            }
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
    if running.is_some() {
        // Every context with its status; an older kojed has no `Contexts`.
        match tokio::time::timeout(TIMEOUT, daemon.contexts()).await {
            Ok(Ok(all)) => {
                let all = all.into_iter().map(ContextEntry::from).collect();
                let _ = tx.send(Update::AllContexts(all)).await;
            }
            Ok(Err(e)) => tracing::debug!("kojed: Contexts: {e}"),
            Err(_) => tracing::warn!("kojed did not answer Contexts within {TIMEOUT:?}"),
        }
    }
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

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|&s| s.to_owned()).collect()
    }

    #[tokio::test]
    async fn commands_go_to_koje() {
        let tmp = tempfile::tempdir().unwrap();
        let koje = fake_koje(tmp.path(), "koje", "exit 0");
        let limit = args(&["limit", "kp", "--soft", "6G", "--hard", "none"]);
        assert_eq!(run_koje(&koje, &limit).await, Ok(()));
        let logged = std::fs::read_to_string(tmp.path().join("args")).unwrap();
        assert_eq!(logged.trim(), "limit kp --soft 6G --hard none");
    }

    #[tokio::test]
    async fn kojes_error_is_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let koje = fake_koje(
            tmp.path(),
            "koje",
            "echo 'Error: the hard limit 2G is below the soft limit 6G' >&2; exit 1",
        );
        let limit = args(&["limit", "kp", "--soft", "6G", "--hard", "2G"]);
        assert_eq!(
            run_koje(&koje, &limit).await,
            Err("the hard limit 2G is below the soft limit 6G".into())
        );
        let silent = fake_koje(tmp.path(), "silent", "exit 3");
        assert!(
            run_koje(&silent, &limit)
                .await
                .unwrap_err()
                .starts_with("koje limit failed")
        );
        assert!(
            run_koje("/nonexistent/koje", &limit)
                .await
                .unwrap_err()
                .starts_with("running /nonexistent/koje")
        );
    }
}
