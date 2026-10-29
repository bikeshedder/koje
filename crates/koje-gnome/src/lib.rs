//! GNOME Shell adapter (ADR 0059): implements koje-core's [`Compositor`] port
//! over koje's GNOME Shell extension, which serves `io.koje.Extension1` on the
//! session bus from inside the Shell. The interface is
//! `gnome/extension/koje@koje.io/dbus/io.koje.Extension1.xml`; the bus form
//! of everything is in [`wire`], the event stream in [`events`].

pub mod events;
pub mod wire;

use std::time::Duration;

use koje_core::ports::{Compositor, CompositorCommand, WindowInfo};
use koje_core::snap::MonitorArea;

pub use events::Events;

pub const BUS_NAME: &str = "io.koje.Extension";
pub const OBJECT_PATH: &str = "/io/koje/Extension";

/// How long one call may take before koje gives up on the Shell.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("koje's GNOME Shell extension: {0}")]
    Dbus(#[from] zbus::Error),
    #[error("koje's GNOME Shell extension did not answer {0} within {REQUEST_TIMEOUT:?}")]
    Timeout(&'static str),
    #[error("not a GNOME window id: {0:?}")]
    InvalidAddress(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[zbus::proxy(
    interface = "io.koje.Extension1",
    default_service = "io.koje.Extension",
    default_path = "/io/koje/Extension",
    gen_blocking = false
)]
pub trait Extension {
    fn windows(&self) -> zbus::Result<Vec<wire::Window>>;
    fn active_workspace(&self) -> zbus::Result<String>;
    fn monitors(&self) -> zbus::Result<Vec<wire::Monitor>>;
    fn apply(&self, commands: &[wire::Command]) -> zbus::Result<()>;

    #[zbus(signal)]
    fn window_opened(
        &self,
        window: u64,
        workspace: String,
        app_id: String,
        title: String,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    fn window_closed(&self, window: u64) -> zbus::Result<()>;
    /// 0: no window has the focus.
    #[zbus(signal)]
    fn window_focused(&self, window: u64) -> zbus::Result<()>;
    #[zbus(signal)]
    fn window_title(&self, window: u64, title: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn workspace_focused(&self, name: String) -> zbus::Result<()>;
}

/// The extension in the Shell, reached over `conn` (the session bus).
#[derive(Debug, Clone)]
pub struct Gnome {
    conn: zbus::Connection,
}

impl Gnome {
    pub fn new(conn: zbus::Connection) -> Self {
        Self { conn }
    }

    /// The extension of the session this process runs in.
    pub async fn session() -> Result<Self> {
        Ok(Self::new(zbus::Connection::session().await?))
    }

    /// The event stream; it follows the extension when the Shell restarts.
    pub fn events(&self) -> Events {
        Events::new(self.conn.clone())
    }

    async fn proxy(&self) -> Result<ExtensionProxy<'static>> {
        Ok(ExtensionProxy::builder(&self.conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?)
    }

    async fn call<T>(
        &self,
        what: &'static str,
        call: impl Future<Output = zbus::Result<T>>,
    ) -> Result<T> {
        Ok(tokio::time::timeout(REQUEST_TIMEOUT, call)
            .await
            .map_err(|_| Error::Timeout(what))??)
    }
}

impl Compositor for Gnome {
    type Error = Error;

    async fn windows(&self) -> Result<Vec<WindowInfo>> {
        let proxy = self.proxy().await?;
        let windows = self.call("Windows", proxy.windows()).await?;
        Ok(windows.into_iter().map(WindowInfo::from).collect())
    }

    async fn active_workspace(&self) -> Result<String> {
        let proxy = self.proxy().await?;
        self.call("ActiveWorkspace", proxy.active_workspace()).await
    }

    async fn monitors(&self) -> Result<Vec<MonitorArea>> {
        let proxy = self.proxy().await?;
        let monitors = self.call("Monitors", proxy.monitors()).await?;
        Ok(monitors.into_iter().map(MonitorArea::from).collect())
    }

    async fn apply(&self, commands: &[CompositorCommand]) -> Result<()> {
        if commands.is_empty() {
            return Ok(());
        }
        let commands = wire::commands(commands)?;
        let proxy = self.proxy().await?;
        self.call("Apply", proxy.apply(&commands)).await
    }
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
