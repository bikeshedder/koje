//! `io.koje.Daemon1` on the session bus (ADR 0027, 0060): a thin layer over
//! [`Daemon`], [`crate::manage`] and `koje_runtime`; names in
//! `koje_core::api`. The contract is `crates/kojed/dbus/io.koje.Daemon1.xml`
//! (a test compares it with what is served).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use koje_core::api::OBJECT_PATH;
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_gnome::Gnome;
use koje_podman::Podman;
use tokio::sync::Mutex;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use crate::daemon::{Change, Daemon};
use crate::wayland::Sockets;

/// How long restored windows may take to appear.
const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub type Shared = Arc<Mutex<Daemon<Gnome, Podman>>>;

#[derive(Debug)]
pub struct Api {
    pub daemon: Shared,
    pub sockets: Sockets,
    /// The host's `XDG_RUNTIME_DIR`.
    pub runtime: std::path::PathBuf,
    pub paths: Paths,
    /// Contexts with an operation (`Up`, `Down`, …) under way.
    pub busy: Arc<std::sync::Mutex<BTreeSet<ContextName>>>,
}

/// kojed's own errors, `io.koje.Error.<Name>` on the bus.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "io.koje.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// No context of that name, or no such state.
    NotFound(String),
    /// A name or value that cannot be used.
    InvalidArgs(String),
    /// `SetConfig` refused a change; nothing was written.
    InvalidConfig(String),
    /// Another operation on the context is under way.
    Busy(String),
    /// `Up` found a recovery file (ADR 0006): call it again with
    /// `restore` or `discard`.
    RecoveryPending(String),
    Failed(String),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        if let Some(invalid) = e.downcast_ref::<crate::manage::Invalid>() {
            return Self::InvalidConfig(invalid.0.clone());
        }
        if let Some(koje_core::error::Error::UnknownContext(_)) = e.downcast_ref() {
            return Self::NotFound(format!("{e:#}"));
        }
        Self::Failed(format!("{e:#}"))
    }
}

fn context_name(context: &str) -> Result<ContextName, Error> {
    ContextName::new(context).map_err(|e| Error::InvalidArgs(e.to_string()))
}

/// Run `f` on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::Failed(e.to_string()))?
        .map_err(Error::from)
}

/// `""` or `empty` → `None`.
fn state_name_arg(name: &str) -> fdo::Result<Option<StateName>> {
    if name.is_empty() {
        return Ok(None);
    }
    let n = StateName::new(name).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
    Ok((!n.is_empty_state()).then_some(n))
}

fn context_arg(context: &str) -> fdo::Result<ContextName> {
    ContextName::new(context).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))
}

fn failed(e: &anyhow::Error) -> fdo::Error {
    fdo::Error::Failed(format!("{e:#}"))
}

#[zbus::interface(name = "io.koje.Daemon1")]
impl Api {
    /// The context of the focused window; empty when it belongs to none.
    async fn active_context(&self) -> String {
        self.daemon
            .lock()
            .await
            .active_context()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    /// Show a context's workspace.
    async fn focus(&self, context: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .focus(&ctx)
            .await
            .map_err(|e| failed(&e))
    }

    /// The context's security-context Wayland socket (ADR 0022), created if
    /// needed; its directory is what `koje up` mounts.
    async fn wayland_socket(&self, context: &str) -> fdo::Result<String> {
        let ctx = context_arg(context)?;
        if !self.daemon.lock().await.knows(&ctx) {
            return Err(fdo::Error::InvalidArgs(format!("no context named {ctx}")));
        }
        let path = self.sockets.ensure(&ctx).map_err(|e| failed(&e))?;
        Ok(path.display().to_string())
    }

    /// The context's sound directory with links to the host's sound sockets
    /// (ADR 0029), kept current by kojed; `koje up` mounts it.
    async fn sound_sockets(&self, context: &str) -> fdo::Result<String> {
        let ctx = context_arg(context)?;
        if !self.daemon.lock().await.knows(&ctx) {
            return Err(fdo::Error::InvalidArgs(format!("no context named {ctx}")));
        }
        crate::sound::ensure(&self.runtime, &ctx)
            .map(|d| d.display().to_string())
            .map_err(|e| fdo::Error::Failed(format!("sound links for {ctx}: {e}")))
    }

    /// Stop serving the context's Wayland socket (`koje down`).
    fn release_wayland_socket(&self, context: &str) -> fdo::Result<()> {
        self.sockets.release(&context_arg(context)?);
        Ok(())
    }

    /// The live state of a running context as TOML (ADR 0030), the format of
    /// a state file.
    async fn state(&self, context: &str) -> fdo::Result<String> {
        let ctx = context_arg(context)?;
        let state = self
            .daemon
            .lock()
            .await
            .state(&ctx)
            .await
            .map_err(|e| failed(&e))?;
        state
            .to_toml(std::path::Path::new("live state"))
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Start the windows of a state (TOML, as a state file) in a context that
    /// was just started (`koje up`, M5.3); the number of programs started.
    /// Windows that do not appear within a minute leave the live state.
    async fn restore(&self, context: &str, state: &str, name: &str) -> fdo::Result<u32> {
        let ctx = context_arg(context)?;
        let name = state_name_arg(name)?;
        let state = koje_core::state::State::parse(state, std::path::Path::new("state from koje"))
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?
            .state;
        let plan = self
            .daemon
            .lock()
            .await
            .restore(&ctx, state, name)
            .await
            .map_err(|e| failed(&e))?;
        let daemon = Arc::clone(&self.daemon);
        tokio::spawn(async move {
            tokio::time::sleep(RESTORE_TIMEOUT).await;
            daemon.lock().await.expire_pending(&ctx);
        });
        Ok(u32::try_from(plan.launches.len()).unwrap_or(u32::MAX))
    }

    /// End the context's programs in order (`koje down`, M5.4) and wait up
    /// to `timeout` seconds for their windows to go; the windows still open
    /// (`id (app id)`), e.g. a program asking to save.
    async fn close_windows(&self, context: &str, timeout: u32) -> fdo::Result<Vec<String>> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .close_windows(&ctx)
            .await
            .map_err(|e| failed(&e))?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout.into());
        loop {
            // Unlocked meanwhile: the event loop sees the windows close.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let open = self.daemon.lock().await.open_windows(&ctx);
            if open.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(open);
            }
        }
    }

    /// `koje up` started the context.
    async fn started(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .started(&ctx)
            .await
            .map_err(|e| failed(&e))?;
        let _ = Self::contexts_changed(&emitter).await;
        Ok(())
    }

    /// `koje down` stopped the context.
    async fn stopped(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon.lock().await.stopped(&ctx).await;
        let _ = Self::contexts_changed(&emitter).await;
        Ok(())
    }

    /// The running context a process belongs to (same PID namespace as the
    /// context's container), `""` for the host – the shell marks a
    /// notification with its sender's context (M8.36).
    async fn context_of_pid(&self, pid: u32) -> String {
        let running = self.daemon.lock().await.running().await;
        let podman = Podman::new();
        for ctx in running {
            let p = podman.clone();
            let c = ctx.clone();
            let Ok(Ok(Some(main))) = tokio::task::spawn_blocking(move || p.running_pid(&c)).await
            else {
                continue;
            };
            if crate::procfs::same_pid_namespace(std::path::Path::new("/proc"), pid, main) {
                return ctx.to_string();
            }
        }
        String::new()
    }

    /// A running context's programs for GNOME's app grid and search (ADR
    /// 0059): `(desktop file id, name, icon, keywords)`, sorted by name; the
    /// icon a file below `/proc/<pid>/root`, else a theme name, `""` for none.
    async fn apps(&self, context: &str) -> fdo::Result<Vec<(String, String, String, Vec<String>)>> {
        let ctx = context_arg(context)?;
        let (apps, pid) = tokio::task::spawn_blocking(move || {
            let podman = Podman::new();
            let apps = context_apps(&podman, &ctx)?;
            anyhow::Ok((apps, podman.running_pid(&ctx)?))
        })
        .await
        .map_err(|e| fdo::Error::Failed(e.to_string()))?
        .map_err(|e| failed(&e))?;
        let root = pid.map(|pid| format!("/proc/{pid}/root"));
        Ok(apps
            .into_iter()
            .map(|a| {
                let icon = match (&a.icon, &root) {
                    (Some(icon), Some(root)) => koje_core::apps::host_icon(icon, root, |p| {
                        std::path::Path::new(p).is_file()
                    }),
                    (icon, _) => icon.clone().unwrap_or_default(),
                };
                (a.id, a.name, icon, a.keywords)
            })
            .collect())
    }

    /// Start a running context's program by its desktop file id.
    async fn launch_app(&self, context: &str, id: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let podman = Podman::new();
            let apps = context_apps(&podman, &ctx)?;
            let Some(app) = apps.iter().find(|a| a.id == id) else {
                anyhow::bail!("{ctx} has no program {id}");
            };
            koje_core::ports::ContainerRuntime::launch(
                &podman,
                &ctx,
                &koje_core::apps::launch_line(app),
                None,
            )?;
            anyhow::Ok(())
        })
        .await
        .map_err(|e| fdo::Error::Failed(e.to_string()))?
        .map_err(|e| failed(&e))
    }

    /// Every context in name order: `(name, label, icon, color, status,
    /// windows)`. `status` is `running`, `paused`, `stopped`, or `invalid`
    /// when its `context.toml` cannot be read.
    async fn contexts(&self) -> Result<Vec<crate::manage::ContextRow>, Error> {
        let live = {
            let mut d = self.daemon.lock().await;
            let running = d.running().await;
            let paused = running.iter().filter(|c| d.is_paused(c)).cloned().collect();
            let windows = d
                .contexts()
                .into_iter()
                .map(|c| (c.name, c.windows))
                .collect();
            crate::manage::Live {
                running,
                paused,
                windows,
            }
        };
        let paths = self.paths.clone();
        blocking(move || crate::manage::contexts(&paths, &live)).await
    }

    /// Something about the contexts changed: which run, their windows, their
    /// config, the active one. Clients read `Contexts` again.
    #[zbus(signal)]
    async fn contexts_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// A context's states: `(name, role, current, stored, windows, layouts,
    /// modified, error)` – role `default`, `fresh` or empty; `stored` false
    /// for a role state never saved and for `empty`; `modified` in seconds
    /// since the epoch, 0 when unknown; `error` why a stored state cannot be
    /// read.
    async fn states(&self, context: &str) -> Result<Vec<crate::manage::StateRow>, Error> {
        let ctx = context_name(context)?;
        let current = self.daemon.lock().await.current_state(&ctx).cloned();
        let paths = self.paths.clone();
        blocking(move || crate::manage::states(&paths, &ctx, current.as_ref())).await
    }

    /// The states of a context changed (saved, copied, renamed, removed).
    #[zbus(signal)]
    async fn states_changed(emitter: &SignalEmitter<'_>, context: &str) -> zbus::Result<()>;

    /// Copy a state (`koje state cp`).
    async fn copy_state(
        &self,
        context: &str,
        from: &str,
        to: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        self.state_op(context, from, Some(to), koje_core::states::copy, &emitter)
            .await
    }

    /// Rename a state (`koje state mv`); a role keeps the old name.
    async fn rename_state(
        &self,
        context: &str,
        from: &str,
        to: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        self.state_op(context, from, Some(to), koje_core::states::rename, &emitter)
            .await
    }

    /// Delete a state (`koje state rm`, the only way states are deleted).
    async fn remove_state(
        &self,
        context: &str,
        state: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        self.state_op(
            context,
            state,
            None,
            |paths, ctx, name, _| koje_core::states::remove(paths, ctx, name),
            &emitter,
        )
        .await
    }

    /// A context's settings (see `crate::manage::config` for the keys).
    async fn config(&self, context: &str) -> Result<HashMap<String, OwnedValue>, Error> {
        let ctx = context_name(context)?;
        let paths = self.paths.clone();
        blocking(move || crate::manage::config(&paths, &ctx)).await
    }

    /// Change a context's settings: the keys given, checked as `koje edit`
    /// checks the file (`io.koje.Error.InvalidConfig`, nothing written
    /// otherwise), followed live where the context can. Returns notes for
    /// the user, e.g. that a change applies only to a new container.
    async fn set_config(
        &self,
        context: &str,
        settings: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<Vec<String>, Error> {
        let ctx = context_name(context)?;
        let paths = self.paths.clone();
        let notes = blocking(move || crate::manage::set_config(&paths, &ctx, &settings)).await?;
        let reloaded = self.daemon.lock().await.reload().await;
        let _ = Self::contexts_changed(&emitter).await;
        reloaded?;
        Ok(notes)
    }

    /// Create a context as `koje new` does (a color of its own); returns its
    /// name.
    async fn new_context(
        &self,
        name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<String, Error> {
        let paths = self.paths.clone();
        let name = name.to_owned();
        let created = blocking(move || crate::manage::new_context(&paths, &name)).await?;
        let reloaded = self.daemon.lock().await.reload().await;
        let _ = Self::contexts_changed(&emitter).await;
        reloaded?;
        Ok(created)
    }

    /// The windows of a running context's live state: `(id, app id,
    /// command, integration)`, empty strings when unknown.
    async fn windows(&self, context: &str) -> Result<Vec<(String, String, String, String)>, Error> {
        let ctx = context_name(context)?;
        let state = self.daemon.lock().await.state(&ctx).await?;
        Ok(state
            .windows
            .into_iter()
            .map(|w| {
                (
                    w.id.to_string(),
                    w.app_id.unwrap_or_default(),
                    w.command.unwrap_or_default(),
                    w.integration.unwrap_or_default(),
                )
            })
            .collect())
    }

    /// Start a context and restore a state (`""`: its `default`), or switch
    /// a running one to the state, as `koje up --no-focus` (ADR 0060).
    /// `recovery` after a crash: `restore` or `discard` the recovery file;
    /// `""` fails with `io.koje.Error.RecoveryPending` when there is one.
    /// Returns at once; `OperationFinished` or `OperationFailed` follow.
    async fn up(
        &self,
        context: &str,
        state: &str,
        recovery: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let (recover, no_recover) = match recovery {
            "" => (false, false),
            "restore" => (true, false),
            "discard" => (false, true),
            other => return Err(Error::InvalidArgs(format!("recovery: {other:?}"))),
        };
        let state = (!state.is_empty()).then(|| state.to_owned());
        if recovery.is_empty() {
            let (paths, c) = (self.paths.clone(), ctx.clone());
            let pending = blocking(move || {
                use koje_core::ports::ContainerRuntime as _;
                let stopped =
                    Podman::new().status(&c)? != koje_core::ports::ContainerStatus::Running;
                Ok(stopped && koje_core::recovery::read(&paths, &c)?.is_some())
            })
            .await?;
            if pending {
                return Err(Error::RecoveryPending(format!(
                    "{ctx} did not shut down cleanly: restore or discard its recovery file"
                )));
            }
        }
        self.operation(conn, ctx, "up", move |paths, ctx| {
            koje_runtime::runtime::up(
                paths,
                ctx.as_str(),
                state.as_deref(),
                koje_runtime::runtime::UpOptions {
                    no_save: false,
                    recover,
                    no_recover,
                    focus: false,
                },
            )
        })
    }

    /// Save the windows into `state` (`""`: the state the context is in) and
    /// stop the context, as `koje down`; `save` false stops without saving.
    /// Fails when windows stay open (a program asking to save). Returns at
    /// once; `OperationFinished` or `OperationFailed` follow.
    fn down(
        &self,
        context: &str,
        state: &str,
        save: bool,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let state = (!state.is_empty()).then(|| state.to_owned());
        self.operation(conn, ctx, "down", move |paths, ctx| {
            koje_runtime::runtime::down(paths, ctx.as_str(), state.as_deref(), !save)
        })
    }

    /// Save the windows of a running context into `state` (`""`: the state
    /// it is in), as `koje save`.
    async fn save(
        &self,
        context: &str,
        state: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let (paths, state) = (self.paths.clone(), state.to_owned());
        let c = ctx.clone();
        blocking(move || {
            koje_runtime::runtime::save(
                &paths,
                c.as_str(),
                (!state.is_empty()).then_some(state.as_str()),
            )
        })
        .await?;
        let _ = Self::states_changed(&emitter, ctx.as_str()).await;
        Ok(())
    }

    /// Pause a context, as `koje sleep`. Returns at once.
    fn sleep(
        &self,
        context: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        self.operation(conn, ctx, "sleep", |paths, ctx| {
            koje_runtime::runtime::sleep(paths, ctx.as_str())
        })
    }

    /// A new container from the current config, as `koje recreate --yes`:
    /// everything installed in the old one outside the home is lost. Returns
    /// at once.
    fn recreate(
        &self,
        context: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        self.operation(conn, ctx, "recreate", |paths, ctx| {
            koje_runtime::runtime::recreate(paths, ctx.as_str(), true)
        })
    }

    /// An operation (`up`, `down`, `sleep`, `recreate`) on a context began.
    #[zbus(signal)]
    async fn operation_started(
        emitter: &SignalEmitter<'_>,
        context: &str,
        operation: &str,
    ) -> zbus::Result<()>;

    /// It ended well.
    #[zbus(signal)]
    async fn operation_finished(
        emitter: &SignalEmitter<'_>,
        context: &str,
        operation: &str,
    ) -> zbus::Result<()>;

    /// It failed; `error` says why.
    #[zbus(signal)]
    async fn operation_failed(
        emitter: &SignalEmitter<'_>,
        context: &str,
        operation: &str,
        error: &str,
    ) -> zbus::Result<()>;

    /// The last memory reading of a context (`""` = the active one) for the
    /// bar (M8.21): `(context, used, charged, soft, hard)` in bytes; a limit
    /// of 0 is none. `("", 0, 0, 0, 0)` without an active context or before
    /// the first reading (every 5 s).
    async fn memory(&self, context: &str) -> fdo::Result<(String, u64, u64, u64, u64)> {
        let d = self.daemon.lock().await;
        let ctx = if context.is_empty() {
            match d.active_context() {
                Some(c) => c.clone(),
                None => return Ok((String::new(), 0, 0, 0, 0)),
            }
        } else {
            context_arg(context)?
        };
        Ok(match d.memory(&ctx) {
            Some((usage, limits)) => (
                ctx.to_string(),
                usage.working_set().0,
                usage.current.0,
                limits.memory_soft.map_or(0, |b| b.0),
                limits.memory_hard.map_or(0, |b| b.0),
            ),
            None => (String::new(), 0, 0, 0, 0),
        })
    }

    /// The working set of a running context (`""` = the active one) over
    /// the last minutes, oldest first, one reading every `interval` seconds:
    /// `(context, interval, readings)` for the shell's graph (M8.55);
    /// `("", 0, [])` without an active context.
    async fn memory_history(&self, context: &str) -> fdo::Result<(String, u32, Vec<u64>)> {
        let d = self.daemon.lock().await;
        let ctx = if context.is_empty() {
            match d.active_context() {
                Some(c) => c.clone(),
                None => return Ok((String::new(), 0, Vec::new())),
            }
        } else {
            context_arg(context)?
        };
        let interval = u32::try_from(crate::TICK.as_secs()).unwrap_or(u32::MAX);
        Ok((ctx.to_string(), interval, d.memory_history(&ctx)))
    }

    /// New memory readings were taken (every tick).
    #[zbus(signal)]
    async fn memory_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Before `koje sleep` (M7.4): every window of the context into its stash.
    async fn hide(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        let hidden = self.daemon.lock().await.hide(&ctx).await;
        let _ = Self::contexts_changed(&emitter).await;
        hidden.map_err(|e| failed(&e))
    }

    /// After `koje wake` (M7.4): the active layout back.
    async fn show(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        let shown = self.daemon.lock().await.show(&ctx).await;
        let _ = Self::contexts_changed(&emitter).await;
        shown.map_err(|e| failed(&e))
    }

    /// Wake a sleeping context (as `koje wake`): continue its container,
    /// then show its windows – the sleep overlay's button on GNOME (ADR 0059).
    async fn wake(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        use koje_core::ports::{ContainerRuntime as _, ContainerStatus};
        let ctx = context_arg(context)?;
        let c = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let podman = Podman::new();
            if podman.status(&c)? == ContainerStatus::Paused {
                podman.unpause(&c)?;
            }
            anyhow::Ok(())
        })
        .await
        .map_err(|e| fdo::Error::Failed(e.to_string()))?
        .map_err(|e| failed(&e))?;
        self.show(context, emitter).await
    }

    /// The state a running context is in (ADR 0032); `""` for `empty` or
    /// unknown. `koje save`/`down` without `-s` write it.
    async fn current_state(&self, context: &str) -> fdo::Result<String> {
        let ctx = context_arg(context)?;
        Ok(self
            .daemon
            .lock()
            .await
            .current_state(&ctx)
            .map(ToString::to_string)
            .unwrap_or_default())
    }

    /// Whether a running context lost its windows (Hyprland or the session
    /// crashed, the container survived): `koje up` then offers its recovery
    /// file (ADR 0006).
    async fn windows_lost(&self, context: &str) -> fdo::Result<bool> {
        let ctx = context_arg(context)?;
        Ok(self.daemon.lock().await.is_lost(&ctx))
    }

    /// Switch a running context to another state (TOML, as a state file;
    /// M5.9). Returns `(windows kept, windows closed, programs started)`.
    /// New windows that do not appear within a minute leave the live state.
    async fn switch_state(
        &self,
        context: &str,
        state: &str,
        name: &str,
    ) -> fdo::Result<(u32, u32, u32)> {
        let ctx = context_arg(context)?;
        let name = state_name_arg(name)?;
        let state = koje_core::state::State::parse(state, std::path::Path::new("state from koje"))
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?
            .state;
        let (kept, closed, started) = self
            .daemon
            .lock()
            .await
            .switch_state(&ctx, state, name)
            .await
            .map_err(|e| failed(&e))?;
        let daemon = Arc::clone(&self.daemon);
        tokio::spawn(async move {
            tokio::time::sleep(RESTORE_TIMEOUT).await;
            daemon.lock().await.expire_pending(&ctx);
        });
        let n = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
        Ok((n(kept), n(closed), n(started)))
    }

    /// Read the contexts' config files again (koje calls it after `new` and `edit`).
    async fn reload(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        let reloaded = self.daemon.lock().await.reload().await;
        // A new or removed context, a new color.
        let _ = Self::contexts_changed(&emitter).await;
        reloaded.map_err(|e| failed(&e))
    }

    #[zbus(signal)]
    async fn window_added(
        emitter: &SignalEmitter<'_>,
        address: &str,
        context: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn window_removed(
        emitter: &SignalEmitter<'_>,
        address: &str,
        context: &str,
    ) -> zbus::Result<()>;
}

impl Api {
    /// Run `f` for `ctx` on the blocking pool, reported by the operation
    /// signals; one operation per context at a time.
    fn operation(
        &self,
        conn: &zbus::Connection,
        ctx: ContextName,
        operation: &'static str,
        f: impl FnOnce(&Paths, &ContextName) -> anyhow::Result<()> + Send + 'static,
    ) -> Result<(), Error> {
        {
            let mut busy = self.busy.lock().map_err(|e| Error::Failed(e.to_string()))?;
            if !busy.insert(ctx.clone()) {
                return Err(Error::Busy(format!("{ctx} is busy with another operation")));
            }
        }
        let (conn, paths, busy) = (conn.clone(), self.paths.clone(), Arc::clone(&self.busy));
        tokio::spawn(async move {
            let emitter = match SignalEmitter::new(&conn, OBJECT_PATH) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("signals for {operation} {ctx}: {e}");
                    return;
                }
            };
            let _ = Self::operation_started(&emitter, ctx.as_str(), operation).await;
            let c = ctx.clone();
            let result = blocking(move || f(&paths, &c)).await;
            if let Ok(mut b) = busy.lock() {
                b.remove(&ctx);
            }
            match result {
                Ok(()) => {
                    let _ = Self::operation_finished(&emitter, ctx.as_str(), operation).await;
                }
                Err(e) => {
                    let error = match e {
                        Error::Failed(m) | Error::NotFound(m) | Error::InvalidArgs(m) => m,
                        other => other.to_string(),
                    };
                    tracing::warn!(context = %ctx, "{operation}: {error}");
                    let _ = Self::operation_failed(&emitter, ctx.as_str(), operation, &error).await;
                }
            }
            let _ = Self::contexts_changed(&emitter).await;
            let _ = Self::states_changed(&emitter, ctx.as_str()).await;
        });
        Ok(())
    }

    /// A state operation `f(paths, ctx, from, to)`; `to` unused for removal.
    async fn state_op(
        &self,
        context: &str,
        from: &str,
        to: Option<&str>,
        f: fn(&Paths, &ContextName, &StateName, &StateName) -> koje_core::error::Result<()>,
        emitter: &SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let from =
            crate::manage::state_name(from).map_err(|e| Error::InvalidArgs(format!("{e:#}")))?;
        let to = match to {
            Some(t) => {
                crate::manage::state_name(t).map_err(|e| Error::InvalidArgs(format!("{e:#}")))?
            }
            None => from.clone(),
        };
        let (paths, c) = (self.paths.clone(), ctx.clone());
        blocking(move || {
            koje_core::context::load(&paths, &c)?;
            Ok(f(&paths, &c, &from, &to)?)
        })
        .await?;
        let _ = Self::states_changed(emitter, ctx.as_str()).await;
        Ok(())
    }
}

/// Tell the bar that what it shows may have changed.
pub async fn contexts_changed(conn: &zbus::Connection) -> zbus::Result<()> {
    Api::contexts_changed(&SignalEmitter::new(conn, OBJECT_PATH)?).await
}

pub async fn memory_changed(conn: &zbus::Connection) -> zbus::Result<()> {
    Api::memory_changed(&SignalEmitter::new(conn, OBJECT_PATH)?).await
}

/// Send a change as a signal.
pub async fn emit(conn: &zbus::Connection, change: &Change) -> zbus::Result<()> {
    let emitter = SignalEmitter::new(conn, OBJECT_PATH)?;
    match change {
        Change::WindowAdded { address, context } => {
            Api::window_added(&emitter, address, context.as_str()).await
        }
        Change::WindowRemoved { address, context } => {
            Api::window_removed(&emitter, address, context.as_str()).await
        }
    }
}

/// The programs of a running context (`koje apps`).
fn context_apps(podman: &Podman, ctx: &ContextName) -> anyhow::Result<Vec<koje_core::apps::App>> {
    use koje_core::ports::ContainerRuntime as _;
    koje_core::lifecycle::require_running(podman.status(ctx)?, ctx.as_str())
        .map_err(anyhow::Error::msg)?;
    let listing = podman.query(ctx, &["sh", "-c", koje_core::apps::LIST_SCRIPT])?;
    Ok(koje_core::apps::parse_listing(&listing))
}
