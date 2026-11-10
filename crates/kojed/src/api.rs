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
use crate::host::Host;
use crate::wayland::Sockets;

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
    /// Images being built (`BuildImage`).
    pub building: Arc<std::sync::Mutex<BTreeSet<String>>>,
}

/// A row of `Apps`.
type AppRow = (String, String, String, Vec<String>, String, String);

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
    /// `Down` closed the windows, but some stayed open (a program asking to
    /// save); the context still runs. Call it again with `force`.
    WindowsOpen(String),
    Failed(String),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        if let Some(invalid) = e.downcast_ref::<crate::manage::Invalid>() {
            return Self::InvalidConfig(invalid.0.clone());
        }
        if e.downcast_ref::<koje_runtime::runtime::StillOpen>()
            .is_some()
        {
            return Self::WindowsOpen(format!("{e:#}"));
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
    /// 0059) and dash (ADR 0062): `(desktop file id, name, icon, keywords,
    /// StartupWMClass, program)`, sorted by name; the icon a file below
    /// `/proc/<pid>/root`, else a theme name; `""` for none.
    async fn apps(&self, context: &str) -> fdo::Result<Vec<AppRow>> {
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
                let program = koje_core::apps::program(&a).unwrap_or_default();
                (
                    a.id,
                    a.name,
                    icon,
                    a.keywords,
                    a.wm_class.unwrap_or_default(),
                    program,
                )
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

    /// Every image (ADR 0064): koje's own and the image folders, see
    /// `crate::manage::ImageRow`.
    async fn images(&self) -> Result<Vec<crate::manage::ImageRow>, Error> {
        let paths = self.paths.clone();
        blocking(move || crate::manage::images(&paths)).await
    }

    /// Create an image folder `FROM` `from` (as `image = …` names an image)
    /// with a label; returns its name.
    async fn new_image(
        &self,
        name: &str,
        from: &str,
        label: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<String, Error> {
        let (paths, name, from, label) = (
            self.paths.clone(),
            name.to_owned(),
            from.to_owned(),
            label.to_owned(),
        );
        let created =
            blocking(move || crate::manage::new_image(&paths, &name, &from, &label)).await?;
        let _ = Self::images_changed(&emitter).await;
        Ok(created)
    }

    /// Change an image's `label` or `description` (`""` removes one).
    async fn set_image(
        &self,
        name: &str,
        settings: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, name) = (self.paths.clone(), name.to_owned());
        blocking(move || crate::manage::set_image(&paths, &name, &settings)).await?;
        let _ = Self::images_changed(&emitter).await;
        Ok(())
    }

    /// Customize koje's own image `name` (ADR 0065): its files copied into
    /// its folder, which takes its place.
    async fn customize_image(
        &self,
        name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, name) = (self.paths.clone(), name.to_owned());
        blocking(move || crate::manage::customize_image(&paths, &name)).await?;
        let _ = Self::images_changed(&emitter).await;
        Ok(())
    }

    /// Back to koje's own image: the customized folder is removed.
    async fn reset_image(
        &self,
        name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, name) = (self.paths.clone(), name.to_owned());
        blocking(move || crate::manage::reset_image(&paths, &name)).await?;
        let _ = Self::images_changed(&emitter).await;
        let _ = Self::contexts_changed(&emitter).await;
        Ok(())
    }

    /// Keep a customized image over koje's newer version: the mark goes.
    async fn keep_image_changes(
        &self,
        name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, name) = (self.paths.clone(), name.to_owned());
        blocking(move || crate::manage::keep_image_changes(&paths, &name)).await?;
        let _ = Self::images_changed(&emitter).await;
        Ok(())
    }

    /// Remove an image folder; refused while a context uses it.
    async fn remove_image(
        &self,
        name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, name) = (self.paths.clone(), name.to_owned());
        blocking(move || crate::manage::remove_image(&paths, &name)).await?;
        let _ = Self::images_changed(&emitter).await;
        Ok(())
    }

    /// Build an image with what it builds on and your user on top, as
    /// `koje image build <name>`. Returns at once; `ImageBuildProgress`,
    /// then `ImageBuildFinished` or `ImageBuildFailed` follow. Contexts get
    /// the build when recreated.
    fn build_image(
        &self,
        name: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let image = koje_core::name::ImageName::new(name)
            .map_err(|e| Error::InvalidArgs(e.to_string()))?
            .to_string();
        {
            let mut busy = self
                .building
                .lock()
                .map_err(|e| Error::Failed(e.to_string()))?;
            if !busy.insert(image.clone()) {
                return Err(Error::Busy(format!("{image} is being built already")));
            }
        }
        let (conn, paths, busy) = (conn.clone(), self.paths.clone(), Arc::clone(&self.building));
        tokio::spawn(async move {
            let Ok(emitter) = SignalEmitter::new(&conn, OBJECT_PATH) else {
                return;
            };
            let _ = Self::image_build_started(&emitter, &image).await;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let name = image.clone();
            let run = blocking(move || {
                koje_runtime::say::to(
                    move |line| {
                        let _ = tx.send(line.to_owned());
                    },
                    || {
                        koje_runtime::images::ensure(
                            &paths,
                            &name,
                            koje_runtime::images::Options {
                                force: true,
                                ..Default::default()
                            },
                        )
                        .map(drop)
                    },
                )
            });
            let progress = async {
                while let Some(line) = rx.recv().await {
                    let _ = Self::image_build_progress(&emitter, &image, &line).await;
                }
            };
            let (result, ()) = tokio::join!(run, progress);
            if let Ok(mut b) = busy.lock() {
                b.remove(&image);
            }
            match result {
                Ok(()) => {
                    let _ = Self::image_build_finished(&emitter, &image).await;
                }
                Err(e) => {
                    let message = zbus::DBusError::description(&e)
                        .map_or_else(|| e.to_string(), ToOwned::to_owned);
                    tracing::warn!(image, "build: {message}");
                    let _ = Self::image_build_failed(&emitter, &image, &message).await;
                }
            }
            let _ = Self::images_changed(&emitter).await;
            let _ = Self::contexts_changed(&emitter).await;
        });
        Ok(())
    }

    /// Images came, went or changed.
    #[zbus(signal)]
    async fn images_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn image_build_started(emitter: &SignalEmitter<'_>, image: &str) -> zbus::Result<()>;

    /// A line of what the build does (podman's output).
    #[zbus(signal)]
    async fn image_build_progress(
        emitter: &SignalEmitter<'_>,
        image: &str,
        line: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn image_build_finished(emitter: &SignalEmitter<'_>, image: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn image_build_failed(
        emitter: &SignalEmitter<'_>,
        image: &str,
        message: &str,
    ) -> zbus::Result<()>;

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

    /// The context's dash favorites (ADR 0053, 0062): `(own, ids)`; `own`
    /// false when it has none of its own (GNOME's apply, as far as it has
    /// the programs).
    async fn favorites(&self, context: &str) -> Result<(bool, Vec<String>), Error> {
        let (paths, ctx) = (self.paths.clone(), context_name(context)?);
        let own = blocking(move || crate::manage::favorites(&paths, &ctx)).await?;
        Ok((own.is_some(), own.unwrap_or_default()))
    }

    /// Set them, as pinned from the dash; `own` false removes the context's
    /// own (`ids` ignored).
    async fn set_favorites(
        &self,
        context: &str,
        own: bool,
        ids: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let (paths, ctx) = (self.paths.clone(), context_name(context)?);
        let c = ctx.clone();
        blocking(move || crate::manage::set_favorites(&paths, &c, own.then_some(&ids[..]))).await?;
        let _ = Self::favorites_changed(&emitter, ctx.as_str()).await;
        Ok(())
    }

    /// A context's dash favorites changed.
    #[zbus(signal)]
    async fn favorites_changed(emitter: &SignalEmitter<'_>, context: &str) -> zbus::Result<()>;

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
    /// a running one to the state, as `koje up` (ADR 0060). `recovery` after
    /// a crash: `restore`, `discard` or `keep` the recovery file; `""` fails
    /// with `io.koje.Error.RecoveryPending` when there is one. `focus` shows
    /// its workspace first; `save` saves a running one's windows before it
    /// switches. Returns at once; `OperationFinished` or `OperationFailed`
    /// follow.
    #[allow(clippy::fn_params_excessive_bools)] // the D-Bus signature
    async fn up(
        &self,
        context: &str,
        state: &str,
        recovery: &str,
        focus: bool,
        save: bool,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let (recover, no_recover) = match recovery {
            "" | "keep" => (false, false),
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
        self.operation(conn, ctx, "up", move |host, paths, ctx| {
            koje_runtime::runtime::up(
                host,
                paths,
                ctx.as_str(),
                state.as_deref(),
                koje_runtime::runtime::UpOptions {
                    no_save: !save,
                    recover,
                    no_recover,
                    focus,
                },
            )
        })
    }

    /// Save the windows into `state` (`""`: the state the context is in) and
    /// stop the context, as `koje down`; `save` false stops without saving.
    /// Windows that stay open (a program asking to save) fail it with
    /// `io.koje.Error.WindowsOpen`, unless `force`. Returns at once;
    /// `OperationFinished` or `OperationFailed` follow.
    fn down(
        &self,
        context: &str,
        state: &str,
        save: bool,
        force: bool,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        let state = (!state.is_empty()).then(|| state.to_owned());
        self.operation(conn, ctx, "down", move |host, paths, ctx| {
            koje_runtime::runtime::down(host, paths, ctx.as_str(), state.as_deref(), !save, force)
        })
    }

    /// Save the windows of a running context into `state` (`""`: the state
    /// it is in), as `koje save`: the state written and its windows.
    async fn save(
        &self,
        context: &str,
        state: &str,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(String, u32), Error> {
        let ctx = context_name(context)?;
        let (paths, state) = (self.paths.clone(), state.to_owned());
        let c = ctx.clone();
        let host = self.host(conn);
        let (saved, windows) = blocking(move || {
            koje_runtime::runtime::save(
                &host,
                &paths,
                c.as_str(),
                (!state.is_empty()).then_some(state.as_str()),
            )
        })
        .await?;
        let _ = Self::states_changed(&emitter, ctx.as_str()).await;
        Ok((
            saved.to_string(),
            u32::try_from(windows).unwrap_or(u32::MAX),
        ))
    }

    /// Pause a context, as `koje sleep`. Returns at once.
    fn sleep(
        &self,
        context: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        self.operation(conn, ctx, "sleep", |host, paths, ctx| {
            koje_runtime::runtime::sleep(host, paths, ctx.as_str())
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
        self.operation(conn, ctx, "recreate", |host, paths, ctx| {
            koje_runtime::runtime::recreate(host, paths, ctx.as_str())
        })
    }

    /// Remove a context for good, as `koje rm --yes`: a running one is
    /// stopped without saving, then its container, home volume, states and
    /// config are deleted. Returns at once; `OperationFinished` or
    /// `OperationFailed` follow, then `ContextsChanged`.
    fn remove(
        &self,
        context: &str,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(), Error> {
        let ctx = context_name(context)?;
        self.operation(conn, ctx, "remove", |host, paths, ctx| {
            koje_runtime::runtime::remove(host, paths, ctx.as_str())?;
            host.reload()
        })
    }

    /// An operation (`up`, `down`, `sleep`, `recreate`, `remove`) on a context began.
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

    /// It failed: `error` is an error name (`io.koje.Error.Failed`,
    /// `io.koje.Error.WindowsOpen`, …), `message` says why.
    #[zbus(signal)]
    async fn operation_failed(
        emitter: &SignalEmitter<'_>,
        context: &str,
        operation: &str,
        error: &str,
        message: &str,
    ) -> zbus::Result<()>;

    /// A line of what it does or noticed, as `koje` prints it.
    #[zbus(signal)]
    async fn operation_progress(
        emitter: &SignalEmitter<'_>,
        context: &str,
        operation: &str,
        line: &str,
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

    /// Wake a sleeping context (as `koje wake`): continue its container,
    /// then show its windows – the sleep overlay's button on GNOME (ADR 0059).
    async fn wake(
        &self,
        context: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        use koje_core::lifecycle::{PausePlan, plan_wake};
        use koje_core::ports::ContainerRuntime as _;
        let ctx = context_arg(context)?;
        let c = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let podman = Podman::new();
            match plan_wake(podman.status(&c)?) {
                PausePlan::Do => podman.unpause(&c)?,
                PausePlan::Already => {}
                PausePlan::NotRunning => {
                    anyhow::bail!("{c} is not running – start it with: koje up {c} -s empty")
                }
            }
            anyhow::Ok(())
        })
        .await
        .map_err(|e| fdo::Error::Failed(e.to_string()))?
        .map_err(|e| failed(&e))?;
        // Awake now; without the extension there is no overlay to take away.
        let shown = self.daemon.lock().await.show(&ctx).await;
        let _ = Self::contexts_changed(&emitter).await;
        if let Err(e) = shown {
            tracing::warn!(context = %ctx, "windows not shown after waking: {e:#}");
        }
        Ok(())
    }

    /// Read the contexts' config files again (koje calls it after `new` and `edit`).
    async fn reload(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        let reloaded = self.daemon.lock().await.reload().await;
        // A new or removed context, a new color; `koje image new`/`rm`.
        let _ = Self::contexts_changed(&emitter).await;
        let _ = Self::images_changed(&emitter).await;
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
    /// What the runtime needs from kojed (G.34).
    fn host(&self, conn: &zbus::Connection) -> Host {
        Host {
            daemon: Arc::clone(&self.daemon),
            sockets: self.sockets.clone(),
            runtime: self.runtime.clone(),
            conn: conn.clone(),
            handle: tokio::runtime::Handle::current(),
        }
    }

    /// Run `f` for `ctx` on the blocking pool, reported by the operation
    /// signals; one operation per context at a time.
    fn operation(
        &self,
        conn: &zbus::Connection,
        ctx: ContextName,
        operation: &'static str,
        f: impl FnOnce(&Host, &Paths, &ContextName) -> anyhow::Result<()> + Send + 'static,
    ) -> Result<(), Error> {
        {
            let mut busy = self.busy.lock().map_err(|e| Error::Failed(e.to_string()))?;
            if !busy.insert(ctx.clone()) {
                return Err(Error::Busy(format!("{ctx} is busy with another operation")));
            }
        }
        let host = self.host(conn);
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
            // The runtime's lines, in order and before the end.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let c = ctx.clone();
            let run = blocking(move || {
                koje_runtime::say::to(
                    move |line| {
                        let _ = tx.send(line.to_owned());
                    },
                    || f(&host, &paths, &c),
                )
            });
            let progress = async {
                while let Some(line) = rx.recv().await {
                    tracing::info!(context = %ctx, "{operation}: {line}");
                    let _ =
                        Self::operation_progress(&emitter, ctx.as_str(), operation, &line).await;
                }
            };
            let (result, ()) = tokio::join!(run, progress);
            if let Ok(mut b) = busy.lock() {
                b.remove(&ctx);
            }
            match result {
                Ok(()) => {
                    let _ = Self::operation_finished(&emitter, ctx.as_str(), operation).await;
                }
                Err(e) => {
                    let name = zbus::DBusError::name(&e).to_string();
                    let message = zbus::DBusError::description(&e)
                        .map_or_else(|| e.to_string(), ToOwned::to_owned);
                    tracing::warn!(context = %ctx, "{operation}: {message}");
                    let _ =
                        Self::operation_failed(&emitter, ctx.as_str(), operation, &name, &message)
                            .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_get_their_names() {
        let open = anyhow::Error::new(koje_runtime::runtime::StillOpen {
            context: ContextName::new("kp").unwrap(),
            windows: vec!["foot".to_owned()],
        })
        .context("stopping kp");
        let e = Error::from(open);
        assert_eq!(
            zbus::DBusError::name(&e).as_str(),
            "io.koje.Error.WindowsOpen"
        );
        assert!(
            zbus::DBusError::description(&e)
                .unwrap()
                .contains("koje down kp --no-save")
        );
        let other = Error::from(anyhow::anyhow!("podman: boom"));
        assert_eq!(
            zbus::DBusError::name(&other).as_str(),
            "io.koje.Error.Failed"
        );
    }
}
