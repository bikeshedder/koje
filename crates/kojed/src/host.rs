//! kojed's side of `koje_runtime::host::Host` (G.34): what `up`, `down`,
//! `save`, `sleep` and `recreate` need from kojed, on its own state. The
//! runtime runs on the blocking pool; each call waits for kojed's async side
//! there, never on the event loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use koje_core::name::{ContextName, StateName};
use koje_core::state::State;
use tokio::runtime::Handle;

use crate::api::Shared;
use crate::wayland::Sockets;

/// How long restored windows may take to appear.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Host {
    pub daemon: Shared,
    pub sockets: Sockets,
    /// The host's `XDG_RUNTIME_DIR`.
    pub runtime: PathBuf,
    pub conn: zbus::Connection,
    pub handle: Handle,
}

impl Host {
    /// Windows of `ctx` that do not appear within a minute leave its live
    /// state.
    fn expire_later(&self, ctx: &ContextName) {
        let (daemon, ctx) = (Arc::clone(&self.daemon), ctx.clone());
        self.handle.spawn(async move {
            tokio::time::sleep(RESTORE_TIMEOUT).await;
            daemon.lock().await.expire_pending(&ctx);
        });
    }

    fn contexts_changed(&self) {
        let conn = self.conn.clone();
        self.handle.spawn(async move {
            let _ = crate::api::contexts_changed(&conn).await;
        });
    }

    /// Read the contexts again: after one was removed.
    pub fn reload(&self) -> Result<()> {
        self.handle
            .block_on(async { self.daemon.lock().await.reload().await })
    }

    fn known(&self, ctx: &ContextName) -> Result<()> {
        if !self.handle.block_on(self.daemon.lock()).knows(ctx) {
            anyhow::bail!("no context named {ctx}");
        }
        Ok(())
    }
}

impl koje_runtime::host::Host for Host {
    fn started(&self, ctx: &ContextName) -> Result<()> {
        self.handle
            .block_on(async { self.daemon.lock().await.started(ctx).await })?;
        self.contexts_changed();
        Ok(())
    }

    fn stopped(&self, ctx: &ContextName) {
        self.handle
            .block_on(async { self.daemon.lock().await.stopped(ctx).await });
        self.contexts_changed();
    }

    fn focus(&self, ctx: &ContextName) -> Result<()> {
        self.handle
            .block_on(async { self.daemon.lock().await.focus(ctx).await })
    }

    fn wayland_socket(&self, ctx: &ContextName) -> Result<PathBuf> {
        self.known(ctx)?;
        self.sockets.ensure(ctx)
    }

    fn sound_sockets(&self, ctx: &ContextName) -> Result<PathBuf> {
        self.known(ctx)?;
        crate::sound::ensure(&self.runtime, ctx)
            .map_err(|e| anyhow::anyhow!("sound links for {ctx}: {e}"))
    }

    fn release_wayland_socket(&self, ctx: &ContextName) {
        self.sockets.release(ctx);
    }

    fn live_state(&self, ctx: &ContextName) -> Result<State> {
        self.handle
            .block_on(async { self.daemon.lock().await.state(ctx).await })
    }

    fn restore(&self, ctx: &ContextName, state: State, name: Option<StateName>) -> Result<usize> {
        let plan = self
            .handle
            .block_on(async { self.daemon.lock().await.restore(ctx, state, name).await })?;
        self.expire_later(ctx);
        Ok(plan.launches.len())
    }

    fn switch_state(
        &self,
        ctx: &ContextName,
        state: State,
        name: Option<StateName>,
    ) -> Result<(usize, usize, usize)> {
        let counts = self.handle.block_on(async {
            self.daemon
                .lock()
                .await
                .switch_state(ctx, state, name)
                .await
        })?;
        self.expire_later(ctx);
        Ok(counts)
    }

    fn current_state(&self, ctx: &ContextName) -> Option<StateName> {
        self.handle
            .block_on(self.daemon.lock())
            .current_state(ctx)
            .cloned()
    }

    fn windows_lost(&self, ctx: &ContextName) -> bool {
        self.handle.block_on(self.daemon.lock()).is_lost(ctx)
    }

    fn close_windows(&self, ctx: &ContextName, timeout: Duration) -> Result<Vec<String>> {
        self.handle.block_on(async {
            self.daemon.lock().await.close_windows(ctx).await?;
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                // Unlocked meanwhile: the event loop sees the windows close.
                tokio::time::sleep(Duration::from_millis(200)).await;
                let open = self.daemon.lock().await.open_windows(ctx);
                if open.is_empty() || tokio::time::Instant::now() >= deadline {
                    return Ok(open);
                }
            }
        })
    }

    fn hide(&self, ctx: &ContextName) -> Result<()> {
        let hidden = self
            .handle
            .block_on(async { self.daemon.lock().await.hide(ctx).await });
        self.contexts_changed();
        hidden
    }
}
