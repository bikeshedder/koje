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

    /// Stop serving the context's Wayland socket (`koje down`).
    fn release_wayland_socket(&self, context: &str) -> fdo::Result<()> {
        self.sockets.release(&context_arg(context)?);
        Ok(())
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
