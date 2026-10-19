//! Hyprland adapter: implements koje-core's [`Compositor`] port over
//! Hyprland's command socket and follows its event socket (M2.2, ADR 0018).
//!
//! All request strings live in [`requests`], the event parser in [`events`];
//! both are pure. Syntax is Hyprland 0.53 (window rules `match:…`, `snake_case`
//! properties). Socket I/O is tokio (ADR 0026).

pub mod clients;
pub mod events;
pub mod instance;
pub mod requests;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use koje_core::ports::{Compositor, CompositorCommand, WindowInfo};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;

pub use events::Events;

/// How long one request may take before koje gives up on Hyprland.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("XDG_RUNTIME_DIR is not set")]
    NoRuntimeDir,
    #[error("no running Hyprland found in {0}")]
    NoInstance(PathBuf),
    #[error("no Wayland socket for the Hyprland instance in {0}")]
    NoWaylandSocket(PathBuf),
    #[error("Hyprland socket {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Hyprland did not answer {request:?} within {REQUEST_TIMEOUT:?}")]
    Timeout { request: String },
    #[error("unexpected reply from Hyprland to {request:?}: {source}")]
    Json {
        request: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Hyprland rejected {request:?}: {reply}")]
    Rejected { request: String, reply: String },
    #[error("cannot send {0:?} to Hyprland: contains ';', ',' or a line break")]
    InvalidArgument(String),
    #[error("unknown monitor {0:?}")]
    UnknownMonitor(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A Hyprland instance, found by [`instance::discover`]. The instance is looked
/// up again when its socket goes away (Hyprland restarted).
#[derive(Debug)]
pub struct Hyprland {
    /// `None`: discover from the environment; `Some` fixed: tests.
    fixed: Option<PathBuf>,
    dir: Mutex<Option<PathBuf>>,
}

impl Hyprland {
    /// The instance of `$HYPRLAND_INSTANCE_SIGNATURE`, else the newest one.
    pub fn from_env() -> Self {
        Self {
            fixed: None,
            dir: Mutex::new(None),
        }
    }

    /// A fixed instance directory (containing `.socket.sock`, `.socket2.sock`).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Self {
            fixed: Some(dir.clone()),
            dir: Mutex::new(Some(dir)),
        }
    }

    fn instance_dir(&self, rediscover: bool) -> Result<PathBuf> {
        if let Some(dir) = &self.fixed {
            return Ok(dir.clone());
        }
        #[allow(clippy::unwrap_used)] // poisoned only after a panic
        let mut cached = self.dir.lock().unwrap();
        if !rediscover && let Some(dir) = cached.as_ref() {
            return Ok(dir.clone());
        }
        let dir = instance::from_env()?;
        *cached = Some(dir.clone());
        Ok(dir)
    }

    /// The event stream of this instance; reconnects by itself.
    pub fn events(&self) -> Events {
        match &self.fixed {
            Some(dir) => Events::at(dir.clone()),
            None => Events::from_env(),
        }
    }

    /// One request on the command socket; Hyprland closes it after replying.
    pub async fn request(&self, request: &str) -> Result<String> {
        let dir = self.instance_dir(false)?;
        match send(&dir, request).await {
            Err(Error::Io { .. }) if self.fixed.is_none() => {
                let fresh = self.instance_dir(true)?;
                send(&fresh, request).await
            }
            other => other,
        }
    }

    /// A request answered with JSON (`j/…`).
    async fn json<T: serde::de::DeserializeOwned>(&self, request: &str) -> Result<T> {
        let reply = self.request(request).await?;
        serde_json::from_str(&reply).map_err(|source| Error::Json {
            request: request.to_owned(),
            source,
        })
    }

    pub async fn monitors(&self) -> Result<Vec<clients::Monitor>> {
        self.json("j/monitors").await
    }
}

async fn send(dir: &Path, request: &str) -> Result<String> {
    let path = dir.join(instance::COMMAND_SOCKET);
    let io = |source| Error::Io {
        path: path.clone(),
        source,
    };
    let exchange = async {
        let mut stream = UnixStream::connect(&path).await.map_err(io)?;
        stream.write_all(request.as_bytes()).await.map_err(io)?;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).await.map_err(io)?;
        Ok(String::from_utf8_lossy(&reply).into_owned())
    };
    tokio::time::timeout(REQUEST_TIMEOUT, exchange)
        .await
        .map_err(|_| Error::Timeout {
            request: request.to_owned(),
        })?
}

impl Compositor for Hyprland {
    type Error = Error;

    async fn windows(&self) -> Result<Vec<WindowInfo>> {
        let monitors = self.monitors().await?;
        let clients: Vec<clients::Client> = self.json("j/clients").await?;
        Ok(clients::windows(&clients, &monitors))
    }

    async fn apply(&self, commands: &[CompositorCommand]) -> Result<()> {
        if commands.is_empty() {
            return Ok(());
        }
        let needs_monitors = commands
            .iter()
            .any(|c| matches!(c, CompositorCommand::Place { .. }));
        let monitors = if needs_monitors {
            self.monitors().await?
        } else {
            Vec::new()
        };
        let lines = requests::commands(commands, &monitors)?;
        let request = requests::batch(&lines);
        let reply = self.request(&request).await?;
        requests::check_reply(&request, &reply)
    }
}
