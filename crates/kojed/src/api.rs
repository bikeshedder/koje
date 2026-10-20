//! `io.koje.Daemon1` on the session bus (ADR 0027): a thin layer over
//! [`Daemon`]; names in `koje_core::api`.

use std::sync::Arc;

use koje_core::api::OBJECT_PATH;
use koje_core::name::ContextName;
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
    async fn restore(&self, context: &str, state: &str) -> fdo::Result<u32> {
        let ctx = context_arg(context)?;
        let state = koje_core::state::State::parse(state, std::path::Path::new("state from koje"))
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?
            .state;
        let plan = self
            .daemon
            .lock()
            .await
            .restore(&ctx, state)
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

    /// `koje up` started the context: its workspace stays while empty.
    async fn started(&self, context: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .started(&ctx)
            .await
            .map_err(|e| failed(&e))
    }

    /// `koje down` stopped the context.
    async fn stopped(&self, context: &str) -> fdo::Result<()> {
        let ctx = context_arg(context)?;
        self.daemon
            .lock()
            .await
            .stopped(&ctx)
            .await
            .map_err(|e| failed(&e))
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
