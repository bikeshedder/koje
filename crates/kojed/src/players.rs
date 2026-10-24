//! The media keys' player (ADR 0041): kojed follows the MPRIS players on the
//! host's session bus and on every running context's bus, remembers the one
//! that last started playing and sends it the keys (`koje media play-pause`).
//!
//! A context's bus is reached from the host below `/proc/<pid>/root` – kojed
//! runs as the same user. kojed is only a client there; names it reads from a
//! context's bus are used as destinations and in messages, never in commands.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use futures_util::StreamExt as _;
use koje_core::media::{self, Playback, PlayerAction};
use koje_core::name::ContextName;
use koje_podman::Podman;
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, MessageStream};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// A bus that does not answer in time (a context paused meanwhile) is skipped.
const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bus {
    Host,
    Context(ContextName),
}

/// A player: the unique name of its connection on a bus.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerRef {
    bus: Bus,
    name: String,
}

#[derive(Debug)]
struct Watch {
    conn: zbus::Connection,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Default)]
struct Inner {
    watches: BTreeMap<Bus, Watch>,
    last: Option<PlayerRef>,
}

#[derive(Debug, Clone)]
pub struct Players {
    inner: Arc<Mutex<Inner>>,
    podman: Podman,
    uid: u32,
}

impl Players {
    pub fn new(uid: u32) -> Self {
        Self {
            inner: Arc::default(),
            podman: Podman::new(),
            uid,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding it leaves plain data behind: keep going.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Follow the buses of the host and of the running contexts; forget those
    /// of contexts that no longer run. Paused contexts are connected once
    /// they run again.
    pub async fn sync(&self, running: &BTreeSet<ContextName>) {
        let watched: BTreeSet<Bus> = {
            let mut inner = self.lock();
            // A watch whose task ended lost its bus (the context stopped).
            inner.watches.retain(|bus, w| {
                !w.task.is_finished()
                    && match bus {
                        Bus::Host => true,
                        Bus::Context(c) => running.contains(c),
                    }
            });
            inner.watches.keys().cloned().collect()
        };
        let wanted = std::iter::once(Bus::Host).chain(running.iter().cloned().map(Bus::Context));
        for bus in wanted.filter(|b| !watched.contains(b)) {
            match self.connect(&bus).await {
                Ok(Some(conn)) => self.watch(bus, conn).await,
                Ok(None) => {}
                Err(e) => tracing::debug!(?bus, "media players: {e:#}"),
            }
        }
    }

    /// A connection to the bus; `None` for a context that does not run.
    async fn connect(&self, bus: &Bus) -> Result<Option<zbus::Connection>> {
        let address = match bus {
            Bus::Host => return Ok(Some(zbus::Connection::session().await?)),
            Bus::Context(ctx) => {
                let podman = self.podman.clone();
                let c = ctx.clone();
                let Some(pid) =
                    tokio::task::spawn_blocking(move || podman.running_pid(&c)).await??
                else {
                    return Ok(None);
                };
                let path = context_bus(pid, self.uid);
                format!("unix:path={}", path.display())
            }
        };
        let conn = tokio::time::timeout(TIMEOUT, async {
            zbus::connection::Builder::address(address.as_str())?
                .build()
                .await
        })
        .await
        .context("no answer")??;
        Ok(Some(conn))
    }

    async fn watch(&self, bus: Bus, conn: zbus::Connection) {
        // Something playing already counts until a player starts.
        let unknown = self.lock().last.is_none();
        if unknown
            && let Ok(players) = list(&conn).await
            && let Some((name, _)) = players.into_iter().find(|(_, p)| *p == Playback::Playing)
        {
            self.lock().last = Some(PlayerRef {
                bus: bus.clone(),
                name,
            });
        }
        let task = tokio::spawn(follow(self.clone(), bus.clone(), conn.clone()));
        tracing::debug!(?bus, "following media players");
        self.lock().watches.insert(bus, Watch { conn, task });
    }

    fn started_playing(&self, player: PlayerRef) {
        tracing::debug!(?player, "media player started");
        self.lock().last = Some(player);
    }

    /// Send a media key to the most recently active player; its name and
    /// where it runs, or `None` without players.
    pub async fn control(&self, action: PlayerAction) -> Option<String> {
        let (conns, last): (Vec<(Bus, zbus::Connection)>, _) = {
            let inner = self.lock();
            let conns = inner
                .watches
                .iter()
                .map(|(b, w)| (b.clone(), w.conn.clone()))
                .collect();
            (conns, inner.last.clone())
        };
        let mut found = Vec::new();
        let mut labels = HashMap::new();
        for (bus, conn) in &conns {
            match tokio::time::timeout(TIMEOUT, list_named(conn)).await {
                Ok(Ok(players)) => {
                    for (unique, label, playback) in players {
                        let player = PlayerRef {
                            bus: bus.clone(),
                            name: unique,
                        };
                        labels.insert(player.name.clone(), label);
                        found.push((player, playback));
                    }
                }
                Ok(Err(e)) => tracing::debug!(?bus, "listing media players: {e:#}"),
                Err(_) => tracing::debug!(?bus, "listing media players: no answer"),
            }
        }
        for player in media::player_order(last.as_ref(), &found) {
            let Some((_, conn)) = conns.iter().find(|(b, _)| *b == player.bus) else {
                continue;
            };
            let call = conn.call_method(
                Some(player.name.as_str()),
                PATH,
                Some(PLAYER),
                action.method(),
                &(),
            );
            match tokio::time::timeout(TIMEOUT, call).await {
                Ok(Ok(_)) => {
                    let label = labels.get(&player.name).cloned().unwrap_or_default();
                    let place = match &player.bus {
                        Bus::Host => "the host".to_owned(),
                        Bus::Context(c) => c.to_string(),
                    };
                    self.lock().last = Some(player);
                    return Some(format!("{label} in {place}"));
                }
                Ok(Err(e)) => tracing::debug!(?player, "media key: {e}"),
                Err(_) => tracing::debug!(?player, "media key: no answer"),
            }
        }
        None
    }
}

/// `/proc/<pid>/root/run/user/<uid>/bus`.
fn context_bus(pid: u32, uid: u32) -> PathBuf {
    let inside = koje_core::spec::session_bus(uid);
    Path::new("/proc")
        .join(pid.to_string())
        .join("root")
        .join(inside.strip_prefix("/").unwrap_or(&inside))
}

/// Remember players that start playing until the bus goes away.
async fn follow(players: Players, bus: Bus, conn: zbus::Connection) {
    if let Err(e) = follow_inner(&players, &bus, &conn).await {
        tracing::debug!(?bus, "media players: {e}");
    }
    tracing::debug!(?bus, "media players: bus gone");
}

async fn follow_inner(players: &Players, bus: &Bus, conn: &zbus::Connection) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .arg(0, PLAYER)?
        .build();
    let mut stream = MessageStream::for_match_rule(rule, conn, None).await?;
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let Some(sender) = msg.header().sender().map(ToString::to_string) else {
            continue;
        };
        let Ok((_, changed, _)) = msg
            .body()
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            continue;
        };
        let playing = changed
            .get("PlaybackStatus")
            .and_then(|v| text(v))
            .and_then(Playback::parse)
            == Some(Playback::Playing);
        if playing {
            players.started_playing(PlayerRef {
                bus: bus.clone(),
                name: sender,
            });
        }
    }
    Ok(())
}

/// A string, also inside a variant (`Get` answers with one).
fn text<'a>(value: &'a zbus::zvariant::Value<'_>) -> Option<&'a str> {
    use zbus::zvariant::Value;
    match value {
        Value::Str(s) => Some(s.as_str()),
        Value::Value(inner) => text(inner),
        _ => None,
    }
}

/// The players on a bus: unique name and status.
async fn list(conn: &zbus::Connection) -> Result<Vec<(String, Playback)>> {
    Ok(list_named(conn)
        .await?
        .into_iter()
        .map(|(unique, _, p)| (unique, p))
        .collect())
}

/// The players on a bus: unique name, short name (`firefox` from
/// `org.mpris.MediaPlayer2.firefox.instance_1_23`) and status. A player that
/// owns several names is listed once.
async fn list_named(conn: &zbus::Connection) -> Result<Vec<(String, String, Playback)>> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let mut players: Vec<(String, String, Playback)> = Vec::new();
    for name in dbus.list_names().await? {
        let Some(short) = name.as_str().strip_prefix(PREFIX) else {
            continue;
        };
        let label = short.split('.').next().unwrap_or(short).to_owned();
        let Ok(owner) = dbus.get_name_owner(name.as_ref()).await else {
            continue;
        };
        let unique = owner.to_string();
        if players.iter().any(|(u, _, _)| *u == unique) {
            continue;
        }
        let reply = conn
            .call_method(
                Some(unique.as_str()),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(PLAYER, "PlaybackStatus"),
            )
            .await;
        let playback = reply
            .ok()
            .and_then(|r| r.body().deserialize::<OwnedValue>().ok())
            .and_then(|v| text(&v).and_then(Playback::parse))
            .unwrap_or(Playback::Stopped);
        players.push((unique, label, playback));
    }
    Ok(players)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_below_the_containers_root() {
        assert_eq!(
            context_bus(598_546, 1000),
            PathBuf::from("/proc/598546/root/run/user/1000/bus")
        );
    }
}
