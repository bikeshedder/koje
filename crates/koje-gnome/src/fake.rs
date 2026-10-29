//! A fake extension for tests, served over a peer-to-peer connection. It
//! answers what it was given, records `Apply` and fails like the extension
//! for verbs GNOME does not have.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use crate::{OBJECT_PATH, wire};

/// One recorded command: the verb and its arguments as text, sorted.
pub type Applied = (String, Vec<(String, String)>);

#[derive(Debug, Default)]
pub struct Fake {
    pub windows: Vec<wire::Window>,
    pub monitors: Vec<wire::Monitor>,
    pub active: String,
    pub applied: Arc<Mutex<Vec<Applied>>>,
}

#[zbus::interface(name = "io.koje.Extension1")]
impl Fake {
    fn windows(&self) -> Vec<wire::Window> {
        self.windows.clone()
    }

    fn active_workspace(&self) -> String {
        self.active.clone()
    }

    fn monitors(&self) -> Vec<wire::Monitor> {
        self.monitors.clone()
    }

    fn apply(&self, commands: Vec<(String, HashMap<String, OwnedValue>)>) -> zbus::fdo::Result<()> {
        let mut failed = Vec::new();
        for (verb, args) in commands {
            if verb == "toggle-special" {
                failed.push(format!("{verb}: not available on GNOME"));
                continue;
            }
            let mut args: Vec<_> = args
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect();
            args.sort();
            #[allow(clippy::unwrap_used)]
            self.applied.lock().unwrap().push((verb, args));
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(zbus::fdo::Error::Failed(failed.join("; ")))
        }
    }

    #[zbus(signal)]
    pub async fn window_opened(
        emitter: &SignalEmitter<'_>,
        window: u64,
        workspace: &str,
        app_id: &str,
        title: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn window_closed(emitter: &SignalEmitter<'_>, window: u64) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn window_focused(emitter: &SignalEmitter<'_>, window: u64) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn window_title(
        emitter: &SignalEmitter<'_>,
        window: u64,
        title: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn workspace_focused(emitter: &SignalEmitter<'_>, name: &str) -> zbus::Result<()>;
}

/// The fake served on one end of a socket pair, a client on the other.
#[allow(clippy::unwrap_used)]
pub async fn serve(fake: Fake) -> (zbus::Connection, zbus::Connection) {
    let (a, b) = tokio::net::UnixStream::pair().unwrap();
    let server = zbus::connection::Builder::unix_stream(a)
        .server(zbus::Guid::generate())
        .unwrap()
        .p2p()
        .serve_at(OBJECT_PATH, fake)
        .unwrap()
        .build();
    let client = zbus::connection::Builder::unix_stream(b).p2p().build();
    tokio::try_join!(server, client).unwrap()
}
