//! The settings app's backend: a tokio thread that reads koje's files and
//! kojed every few seconds and runs the `koje` commands the pages ask for
//! (ADR 0042: nothing blocks the GTK main loop).

use std::time::Duration;

use async_channel::Sender;
use koje_core::config::GlobalConfig;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::state::State;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::backend::DaemonProxy;
use crate::settings::{ContextItem, Snapshot, Status};

/// How often the pages are refreshed without a change of their own.
const EVERY: Duration = Duration::from_secs(3);
const KOJED_TIMEOUT: Duration = Duration::from_secs(2);
/// `koje up` may start many programs; `down` waits for them.
const KOJE_TIMEOUT: Duration = Duration::from_secs(300);

/// A `koje` command for a context: its arguments and, for `edit --stdin`,
/// what goes to its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub context: String,
    pub args: Vec<String>,
    pub input: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Snapshot(Box<Snapshot>),
    /// A command started (`true`) or ended.
    Busy {
        context: String,
        busy: bool,
    },
    /// A command failed: what `koje` said.
    Failed {
        context: String,
        error: String,
    },
}

pub fn spawn(tx: Sender<Event>, mut commands: UnboundedReceiver<Command>) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    std::thread::Builder::new()
        .name("settings-backend".into())
        .spawn(move || {
            runtime.block_on(async move {
                let conn = zbus::Connection::session().await.ok();
                let wake = std::sync::Arc::new(tokio::sync::Notify::new());
                let reader = {
                    let (tx, wake) = (tx.clone(), wake.clone());
                    async move {
                        loop {
                            let snapshot = snapshot(conn.as_ref()).await;
                            if tx.send(Event::Snapshot(Box::new(snapshot))).await.is_err() {
                                return;
                            }
                            tokio::select! {
                                () = tokio::time::sleep(EVERY) => {}
                                () = wake.notified() => {}
                            }
                        }
                    }
                };
                let runner = async {
                    while let Some(cmd) = commands.recv().await {
                        let (tx, wake) = (tx.clone(), wake.clone());
                        tokio::spawn(async move {
                            let context = cmd.context.clone();
                            let _ = tx
                                .send(Event::Busy {
                                    context: context.clone(),
                                    busy: true,
                                })
                                .await;
                            let result = run(&cmd).await;
                            let _ = tx
                                .send(Event::Busy {
                                    context: context.clone(),
                                    busy: false,
                                })
                                .await;
                            if let Err(error) = result {
                                let _ = tx.send(Event::Failed { context, error }).await;
                            }
                            wake.notify_one();
                        });
                    }
                };
                tokio::select! {
                    () = reader => {}
                    () = runner => {}
                }
            });
        })?;
    Ok(())
}

/// Run `koje` with the command's arguments; its error message when it fails.
async fn run(cmd: &Command) -> Result<(), String> {
    use tokio::io::AsyncWriteExt as _;
    let mut child = tokio::process::Command::new(crate::KOJE)
        .args(&cmd.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("running koje: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(input) = &cmd.input {
            let _ = stdin.write_all(input.as_bytes()).await;
        }
        drop(stdin);
    }
    let out = tokio::time::timeout(KOJE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "koje did not finish in time".to_owned())?
        .map_err(|e| format!("running koje: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.trim().trim_start_matches("Error: ").to_owned())
}

/// Every context with its config, status, memory, states and windows.
async fn snapshot(conn: Option<&zbus::Connection>) -> Snapshot {
    let Ok(paths) = Paths::from_env() else {
        return Snapshot::default();
    };
    let global = GlobalConfig::load(&paths.global_config()).unwrap_or_default();
    let proxy = match conn {
        Some(c) => DaemonProxy::new(c).await.ok(),
        None => None,
    };
    let statuses: Option<Vec<(String, String, String)>> = match &proxy {
        Some(p) => tokio::time::timeout(KOJED_TIMEOUT, p.contexts())
            .await
            .ok()
            .and_then(Result::ok),
        None => None,
    };
    let names = koje_core::context::names(&paths).unwrap_or_default();
    let mut contexts = Vec::new();
    for name in names {
        let config = koje_core::context::load(&paths, &name).map_err(|e| e.to_string());
        let status = match &statuses {
            Some(list) => list
                .iter()
                .find(|(n, ..)| *n == name.as_str())
                .map_or(Status::Stopped, |(_, _, s)| Status::parse(s)),
            None => Status::Unknown,
        };
        let roles = config
            .as_ref()
            .map(|c| c.states.clone())
            .unwrap_or_default();
        let saved = koje_core::states::list(&paths, &name, &roles).unwrap_or_default();
        let (memory, live) = match (&proxy, status.up()) {
            (Some(p), true) => (memory(p, &name).await, live(p, &name).await),
            _ => (None, stored(&paths, &name, &roles.default)),
        };
        contexts.push(ContextItem {
            name: name.to_string(),
            config,
            status,
            memory,
            states: saved,
            live,
        });
    }
    Snapshot {
        contexts,
        global,
        kojed: statuses.is_some(),
    }
}

async fn memory(p: &DaemonProxy<'_>, name: &ContextName) -> Option<(u64, u64, u64)> {
    let (ctx, used, _, soft, hard) = tokio::time::timeout(KOJED_TIMEOUT, p.memory(name.as_str()))
        .await
        .ok()?
        .ok()?;
    (!ctx.is_empty()).then_some((used, soft, hard))
}

async fn live(p: &DaemonProxy<'_>, name: &ContextName) -> Option<State> {
    let text = tokio::time::timeout(KOJED_TIMEOUT, p.state(name.as_str()))
        .await
        .ok()?
        .ok()?;
    State::from_toml(&text, std::path::Path::new("live state")).ok()
}

/// A stopped context's windows: those of the state `up` loads.
fn stored(paths: &Paths, name: &ContextName, state: &koje_core::name::StateName) -> Option<State> {
    State::load(&paths.state_file(name, state))
        .ok()??
        .state
        .into()
}
