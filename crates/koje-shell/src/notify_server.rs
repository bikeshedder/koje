//! The shell is the session's notification server (M8.36):
//! `org.freedesktop.Notifications` on the session bus, as GNOME Shell's
//! `notificationDaemon.js` answers it. The decisions are in
//! [`crate::notifications`]; the banners and the date menu's list are drawn
//! from the updates sent here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use tokio::sync::Mutex;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use crate::model::Update;
use crate::notifications::{Closed, Incoming, Notification, Tray, Urgency};

pub const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";
/// Asking kojed which context a sender is in.
const KOJED_TIMEOUT: Duration = Duration::from_secs(1);

/// What the shell's widgets ask of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A click on the notification (`None`) or one of its buttons.
    Activate { id: u32, action: Option<String> },
    /// Its close button.
    Dismiss(u32),
    /// The banner went away by itself.
    BannerDone(u32),
    /// The date menu's "Clear".
    Clear,
    /// "Do Not Disturb".
    SetDnd(bool),
    /// The date menu was opened.
    Seen,
}

#[derive(Debug)]
struct State {
    tray: Tray,
    /// The banner on screen.
    showing: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Server {
    state: Arc<Mutex<State>>,
    tx: Sender<Update>,
}

struct Interface {
    server: Server,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn hint<T: TryFrom<OwnedValue>>(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    hints
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| T::try_from(v).ok())
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Interface {
    #[allow(clippy::too_many_arguments, reason = "the spec's signature")]
    async fn notify(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        // GNOME ignores the timeout too: banners stay 4 s, critical ones
        // until dismissed.
        let _ = expire_timeout;
        let context = match header.sender() {
            Some(sender) => context_of(conn, sender.as_str()).await,
            None => None,
        };
        let desktop_entry: Option<String> = hint::<String>(&hints, "desktop-entry")
            .map(|d| d.trim_end_matches(".desktop").to_owned())
            .filter(|d| !d.is_empty());
        let incoming = Incoming {
            app_name,
            replaces: replaces_id,
            app_icon,
            summary,
            body,
            actions,
            urgency: Urgency::from_hint(hint::<u8>(&hints, "urgency").unwrap_or(1)),
            transient: hint::<bool>(&hints, "transient").unwrap_or(false),
            resident: hint::<bool>(&hints, "resident").unwrap_or(false),
            desktop_entry,
            image_path: hint::<String>(&hints, "image-path")
                .or_else(|| hint::<String>(&hints, "image_path"))
                .map(|p| p.trim_start_matches("file://").to_owned())
                .filter(|p| !p.is_empty()),
            context,
        };
        self.server.add(incoming).await
    }

    async fn close_notification(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
    ) {
        if self.server.state.lock().await.tray.remove(id) {
            let _ = Self::notification_closed(&emitter, id, Closed::App as u32).await;
            self.server.publish().await;
        }
    }

    #[allow(clippy::unused_self, reason = "a D-Bus method")]
    fn get_capabilities(&self) -> Vec<String> {
        [
            "actions",
            "body",
            "body-markup",
            "icon-static",
            "persistence",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[allow(clippy::unused_self, reason = "a D-Bus method")]
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "koje-shell".to_owned(),
            "koje".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
            "1.2".to_owned(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

/// The running context of a sender: its PID from the bus, kojed tells whose
/// container it is in.
async fn context_of(conn: &zbus::Connection, sender: &str) -> Option<String> {
    let lookup = async {
        let dbus = zbus::fdo::DBusProxy::new(conn).await.ok()?;
        let pid = dbus
            .get_connection_unix_process_id(sender.try_into().ok()?)
            .await
            .ok()?;
        let reply = conn
            .call_method(
                Some(koje_core::api::BUS_NAME),
                koje_core::api::OBJECT_PATH,
                Some(koje_core::api::INTERFACE),
                "ContextOfPid",
                &(pid,),
            )
            .await
            .ok()?;
        reply.body().deserialize::<String>().ok()
    };
    tokio::time::timeout(KOJED_TIMEOUT, lookup)
        .await
        .ok()
        .flatten()
        .filter(|c| !c.is_empty())
}

/// The desktop file's name for the header (GNOME shows the app's name).
fn app_name(desktop_entry: Option<&str>) -> Option<String> {
    let id = format!("{}.desktop", desktop_entry?);
    let path = xdg_data_dirs()
        .into_iter()
        .map(|d| d.join("applications").join(&id))
        .find(|p| p.is_file())?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
            return Some(name.to_owned());
        }
    }
    None
}

fn xdg_data_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(std::path::PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join(".local/share"));
    }
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(std::path::PathBuf::from),
    );
    dirs
}

impl Server {
    async fn add(&self, incoming: Incoming) -> u32 {
        let name = app_name(incoming.desktop_entry.as_deref());
        let id = {
            let mut s = self.state.lock().await;
            let id = s.tray.notify(incoming, name, now());
            // The banner on screen was replaced: show it again.
            if s.showing == Some(id) {
                s.showing = None;
            }
            id
        };
        self.publish().await;
        self.next_banner().await;
        id
    }

    /// The list to the date menu.
    async fn publish(&self) {
        let (list, unseen, dnd) = {
            let s = self.state.lock().await;
            (s.tray.list.clone(), s.tray.unseen(), s.tray.dnd)
        };
        let _ = self
            .tx
            .send(Update::Notifications { list, unseen, dnd })
            .await;
    }

    async fn next_banner(&self) {
        let banner: Option<Notification> = {
            let mut s = self.state.lock().await;
            if s.showing.is_some() {
                return;
            }
            let next = s.tray.next_banner();
            s.showing = next.as_ref().map(|n| n.id);
            next
        };
        if let Some(n) = banner {
            let _ = self.tx.send(Update::Banner(n)).await;
        }
    }

    /// Handle what the widgets ask; signals go out on `conn`.
    pub async fn handle(&self, conn: &zbus::Connection, request: Request) {
        let emitter = SignalEmitter::new(conn, PATH).ok();
        let closed = |id, reason: Closed| {
            let emitter = emitter.clone();
            async move {
                if let Some(e) = emitter {
                    let _ = Interface::notification_closed(&e, id, reason as u32).await;
                }
            }
        };
        match request {
            Request::Activate { id, action } => {
                let key = action.unwrap_or_else(|| "default".to_owned());
                let invoke = self
                    .state
                    .lock()
                    .await
                    .tray
                    .get(id)
                    .is_some_and(|n| key != "default" || n.default_action);
                if invoke && let Some(e) = &emitter {
                    let _ = Interface::action_invoked(e, id, &key).await;
                }
                let gone = self.state.lock().await.tray.activated(id);
                if let Some(reason) = gone {
                    closed(id, reason).await;
                }
                self.banner_gone(id).await;
            }
            Request::Dismiss(id) => {
                if self.state.lock().await.tray.remove(id) {
                    closed(id, Closed::Dismissed).await;
                }
                self.banner_gone(id).await;
            }
            Request::BannerDone(id) => {
                let gone = self.state.lock().await.tray.banner_done(id);
                if let Some(reason) = gone {
                    closed(id, reason).await;
                }
                self.banner_gone(id).await;
            }
            Request::Clear => {
                let gone = self.state.lock().await.tray.clear();
                for id in gone {
                    closed(id, Closed::Dismissed).await;
                }
            }
            Request::SetDnd(on) => self.state.lock().await.tray.dnd = on,
            Request::Seen => self.state.lock().await.tray.see_all(),
        }
        self.publish().await;
    }

    async fn banner_gone(&self, id: u32) {
        {
            let mut s = self.state.lock().await;
            if s.showing == Some(id) {
                s.showing = None;
            }
        }
        self.next_banner().await;
    }
}

/// Serve on the session bus; `None` when another server has the name (mako
/// outside the koje session) – then the shell shows none.
pub async fn start(tx: Sender<Update>) -> Option<(Server, zbus::Connection)> {
    let server = Server {
        state: Arc::new(Mutex::new(State {
            tray: Tray::default(),
            showing: None,
        })),
        tx,
    };
    let built = async {
        let conn = zbus::connection::Builder::session()?
            .serve_at(
                PATH,
                Interface {
                    server: server.clone(),
                },
            )?
            .build()
            .await?;
        conn.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
            .await?;
        zbus::Result::Ok(conn)
    };
    match built.await {
        Ok(conn) => {
            tracing::info!("serving {NAME}");
            Some((server, conn))
        }
        Err(e) => {
            tracing::warn!("not the notification server (is another one running?): {e}");
            None
        }
    }
}
