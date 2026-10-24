//! `io.koje.Daemon1` on the session bus (ADR 0027): a thin layer over
//! [`Daemon`]; names in `koje_core::api`.

use std::sync::Arc;

use koje_core::api::OBJECT_PATH;
use koje_core::name::{ContextName, LayoutName, StateName, WindowId};
use koje_hyprland::Hyprland;
use koje_podman::Podman;
use tokio::sync::Mutex;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

use crate::daemon::{Change, Daemon};
use crate::wayland::Sockets;

/// How long restored windows may take to appear.
const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub type Shared = Arc<Mutex<Daemon<Hyprland, Podman>>>;

#[derive(Debug)]
pub struct Api {
    pub daemon: Shared,
    pub sockets: Sockets,
    /// The host's `XDG_RUNTIME_DIR`.
    pub runtime: std::path::PathBuf,
}

/// `""` or `empty` → `None`.
fn state_name_arg(name: &str) -> fdo::Result<Option<StateName>> {
    if name.is_empty() {
        return Ok(None);
    }
    let n = StateName::new(name).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
    Ok((!n.is_empty_state()).then_some(n))
}

fn layout_arg(name: &str) -> fdo::Result<LayoutName> {
    LayoutName::new(name).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))
}

fn context_arg(context: &str) -> fdo::Result<ContextName> {
    ContextName::new(context).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))
}

fn failed(e: &anyhow::Error) -> fdo::Error {
    fdo::Error::Failed(format!("{e:#}"))
}

#[zbus::interface(name = "io.koje.Daemon1")]
impl Api {
    /// Contexts with color (`#RRGGBB`) and number of windows.
    async fn list_contexts(&self) -> Vec<(String, String, u32)> {
        self.daemon
            .lock()
            .await
            .contexts()
            .into_iter()
            .map(|c| {
                (
                    c.name.to_string(),
                    c.color.to_string(),
                    u32::try_from(c.windows).unwrap_or(u32::MAX),
                )
            })
            .collect()
    }

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
        let path = self.sockets.ensure(&ctx).await.map_err(|e| failed(&e))?;
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

    /// Empty when the compositor offers security contexts (ADR 0022), else
    /// why not (`koje doctor`).
    async fn wayland_status(&self) -> String {
        match self.sockets.probe().await {
            Ok(()) => String::new(),
            Err(e) => format!("{e:#}"),
        }
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

    /// Running contexts: `(name, color, windows, active)` – the bar (M5.15).
    async fn running_contexts(&self) -> Vec<(String, String, u32, bool)> {
        let mut d = self.daemon.lock().await;
        let running = d.running().await;
        let active = d.active_context().cloned();
        d.contexts()
            .into_iter()
            .filter(|c| running.contains(&c.name))
            .map(|c| {
                let is_active = active.as_ref() == Some(&c.name);
                (
                    c.name.to_string(),
                    c.color.to_string(),
                    u32::try_from(c.windows).unwrap_or(u32::MAX),
                    is_active,
                )
            })
            .collect()
    }

    /// Something the bar shows changed: running contexts, the active one,
    /// window counts.
    #[zbus(signal)]
    async fn contexts_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

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

    /// New memory readings were taken (every tick).
    #[zbus(signal)]
    async fn memory_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Switch the layout of a running context (`""` = the active one); `name`
    /// may be `+next` or `+prev` (layout names start with a letter). Returns
    /// the layout now active (M5.5).
    async fn switch_layout(&self, context: &str, name: &str) -> fdo::Result<String> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let result = match name {
            "+next" => d.cycle_layout(&ctx, 1).await,
            "+prev" => d.cycle_layout(&ctx, -1).await,
            other => {
                let layout = layout_arg(other)?;
                d.switch_layout(&ctx, &layout).await.map(|()| layout)
            }
        };
        result.map(|l| l.to_string()).map_err(|e| failed(&e))
    }

    /// `layout new <name> [--copy]`.
    async fn new_layout(&self, context: &str, name: &str, copy: bool) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        d.new_layout(&ctx, layout_arg(name)?, copy)
            .await
            .map_err(|e| failed(&e))
    }

    /// `layout save`: the current arrangement into the active layout.
    async fn save_layout(&self, context: &str) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        d.save_layout(&ctx).await.map_err(|e| failed(&e))
    }

    /// `layout autosave <name> on|off`.
    async fn set_layout_autosave(&self, context: &str, name: &str, on: bool) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let state = d.live_state_mut(&ctx).map_err(|e| failed(&e))?;
        koje_core::layouts::set_autosave(state, &layout_arg(name)?, on)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// `layout mv <from> <to>`.
    async fn rename_layout(&self, context: &str, from: &str, to: &str) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let state = d.live_state_mut(&ctx).map_err(|e| failed(&e))?;
        koje_core::layouts::rename(state, &layout_arg(from)?, layout_arg(to)?)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// `layout rm <name>` (not the active one).
    async fn remove_layout(&self, context: &str, name: &str) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let state = d.live_state_mut(&ctx).map_err(|e| failed(&e))?;
        koje_core::layouts::remove(state, &layout_arg(name)?)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// `layout list`: `(name, active, autosave, windows)` in cycle order, and
    /// the context it is about.
    async fn layouts(
        &self,
        context: &str,
    ) -> fdo::Result<(String, Vec<(String, bool, bool, u32)>)> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let list = d
            .layouts(&ctx)
            .map_err(|e| failed(&e))?
            .into_iter()
            .map(|l| {
                (
                    l.name.to_string(),
                    l.active,
                    l.autosave,
                    u32::try_from(l.windows).unwrap_or(u32::MAX),
                )
            })
            .collect();
        Ok((ctx.to_string(), list))
    }

    /// `stash`: the focused window to its context's stash (M5.6); returns
    /// `(context, window id)`.
    async fn stash(&self) -> fdo::Result<(String, String)> {
        let (ctx, id) = self
            .daemon
            .lock()
            .await
            .stash_focused()
            .await
            .map_err(|e| failed(&e))?;
        Ok((ctx.to_string(), id))
    }

    /// `unstash [<window>] [--to <layout>]`; `""` for the focused window and
    /// the active layout. Returns the window id.
    async fn unstash(&self, context: &str, window: &str, to: &str) -> fdo::Result<String> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        let window = (!window.is_empty())
            .then(|| WindowId::new(window))
            .transpose()
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let to = (!to.is_empty()).then(|| layout_arg(to)).transpose()?;
        d.unstash(&ctx, window, to)
            .await
            .map(|id| id.to_string())
            .map_err(|e| failed(&e))
    }

    /// Before `koje sleep` (M7.4): every window of the context into its stash.
    async fn hide(&self, context: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .hide(&ctx)
            .await
            .map_err(|e| failed(&e))
    }

    /// After `koje wake` (M7.4): the active layout back.
    async fn show(&self, context: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .show(&ctx)
            .await
            .map_err(|e| failed(&e))
    }

    /// `stash --show`: show or hide the context's stash.
    async fn toggle_stash(&self, context: &str) -> fdo::Result<()> {
        let mut d = self.daemon.lock().await;
        let ctx = d.context_or_active(context).map_err(|e| failed(&e))?;
        d.toggle_stash(&ctx).await.map_err(|e| failed(&e))
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
    async fn reload(&self) -> fdo::Result<()> {
        self.daemon
            .lock()
            .await
            .reload()
            .await
            .map_err(|e| failed(&e))
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
