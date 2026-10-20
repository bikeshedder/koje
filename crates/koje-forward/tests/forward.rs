//! `koje-forward` against two real `dbus-daemon`s: a "host" bus with fake
//! portal and notification services, and a "context" bus with a client that
//! behaves like libwebrtc (listens on the predicted request path first).
//! Skipped (with a message) when `dbus-daemon` is not installed.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::Read as _;
use std::os::fd::{AsFd as _, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::StreamExt as _;
use zbus::message::Type;
use zbus::zvariant::{Fd, ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

const P: &str = "/org/freedesktop/portal/desktop";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const NOTIFY: &str = "org.freedesktop.Notifications";

/// A private `dbus-daemon`, killed on drop.
struct Bus {
    address: String,
    pid: u32,
}

impl Bus {
    fn start() -> Option<Self> {
        let out = Command::new("dbus-daemon")
            .args(["--session", "--fork", "--print-address=1", "--print-pid=2"])
            .output()
            .ok()?;
        let address = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .to_owned();
        let pid = String::from_utf8_lossy(&out.stderr).trim().parse().ok()?;
        Some(Self { address, pid })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = Command::new("kill").arg(self.pid.to_string()).status();
    }
}

struct Forwarder(Child);

impl Drop for Forwarder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn seg(unique: &str) -> String {
    unique.trim_start_matches(':').replace('.', "_")
}

/// Fake host services: a portal with `ScreenCast` + `Session`, and notifications.
/// Records the paths the portal saw.
async fn fake_host(address: &str) -> (Connection, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let conn = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    conn.request_name(PORTAL).await.unwrap();
    conn.request_name(NOTIFY).await.unwrap();
    let (seen_tx, seen_rx) = tokio::sync::mpsc::unbounded_channel();
    let c = conn.clone();
    let mut stream = MessageStream::from(&c);
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            if msg.message_type() != Type::MethodCall {
                continue;
            }
            let h = msg.header();
            let sender = h.sender().unwrap().to_string();
            let member = h.member().unwrap().to_string();
            let path = h.path().unwrap().to_string();
            let dest = h.destination().map(ToString::to_string).unwrap_or_default();
            let iface = h.interface().map(ToString::to_string).unwrap_or_default();
            // Like separate services: portal calls must name the portal.
            let wrong = (iface.starts_with("org.freedesktop.portal.") && dest != PORTAL)
                || (iface == NOTIFY && dest != NOTIFY);
            match if wrong { "" } else { member.as_str() } {
                "CreateSession" => {
                    let (opts,): (HashMap<String, OwnedValue>,) = msg.body().deserialize().unwrap();
                    let token: String = opts["handle_token"].clone().try_into().unwrap();
                    let req = format!("{P}/request/{}/{token}", seg(&sender));
                    let ses = format!("{P}/session/{}/s1", seg(&sender));
                    let reply = Message::method_return(&h)
                        .unwrap()
                        .build(&(ObjectPath::try_from(req.as_str()).unwrap(),))
                        .unwrap();
                    c.send(&reply).await.unwrap();
                    let results = HashMap::from([("session_handle", Value::from(ses.as_str()))]);
                    c.emit_signal(
                        Some(sender.as_str()),
                        req.as_str(),
                        "org.freedesktop.portal.Request",
                        "Response",
                        &(0u32, results),
                    )
                    .await
                    .unwrap();
                }
                "OpenPipeWireRemote" => {
                    let (session, _opts): (OwnedObjectPath, HashMap<String, OwnedValue>) =
                        msg.body().deserialize().unwrap();
                    seen_tx
                        .send(format!("OpenPipeWireRemote {session}"))
                        .unwrap();
                    let (r, mut w) = std::io::pipe().unwrap();
                    std::io::Write::write_all(&mut w, b"pipewire remote").unwrap();
                    drop(w);
                    let fd: OwnedFd = r.into();
                    let reply = Message::method_return(&h)
                        .unwrap()
                        .build(&(Fd::from(fd),))
                        .unwrap();
                    c.send(&reply).await.unwrap();
                }
                "Close" => {
                    seen_tx.send(format!("Close {path}")).unwrap();
                    c.send(&Message::method_return(&h).unwrap().build(&()).unwrap())
                        .await
                        .unwrap();
                }
                "Notify" => {
                    c.send(
                        &Message::method_return(&h)
                            .unwrap()
                            .build(&(42u32,))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                    c.emit_signal(
                        None::<&str>,
                        "/org/freedesktop/Notifications",
                        NOTIFY,
                        "NotificationClosed",
                        &(42u32, 2u32),
                    )
                    .await
                    .unwrap();
                }
                _ => {
                    let err = Message::error(&h, "org.freedesktop.DBus.Error.UnknownMethod")
                        .unwrap()
                        .build(&("unknown",))
                        .unwrap();
                    c.send(&err).await.unwrap();
                }
            }
        }
    });
    (conn, seen_rx)
}

async fn wait_for_name(conn: &Connection, name: &str) {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.unwrap();
    for _ in 0..100 {
        if dbus.name_has_owner(name.try_into().unwrap()).await.unwrap() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{name} never appeared on the context bus");
}

async fn unique_names(conn: &Connection) -> usize {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.unwrap();
    dbus.list_names()
        .await
        .unwrap()
        .iter()
        .filter(|n| n.starts_with(':'))
        .count()
}

/// Like libwebrtc: subscribe to the predicted request path, then call
/// `CreateSession`, `OpenPipeWireRemote` (fd) and `Session.Close`.
async fn screencast_round_trip(
    client: &Connection,
    seen: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) {
    let me = seg(client.unique_name().unwrap().as_str());
    let predicted = format!("{P}/request/{me}/webrtc1");
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(predicted.as_str())
        .unwrap()
        .build();
    let mut responses = MessageStream::for_match_rule(rule, client, None)
        .await
        .unwrap();
    let opts = HashMap::from([
        ("handle_token", Value::from("webrtc1")),
        ("session_handle_token", Value::from("s1")),
    ]);
    let reply = client
        .call_method(
            Some(PORTAL),
            P,
            Some("org.freedesktop.portal.ScreenCast"),
            "CreateSession",
            &(opts,),
        )
        .await
        .unwrap();
    let (handle,): (OwnedObjectPath,) = reply.body().deserialize().unwrap();
    assert_eq!(
        handle.as_str(),
        predicted,
        "returned handle in the client's form"
    );

    let response = tokio::time::timeout(Duration::from_secs(5), responses.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        response.body().deserialize().unwrap();
    assert_eq!(code, 0);
    let session: String = results["session_handle"].clone().try_into().unwrap();
    assert_eq!(
        session,
        format!("{P}/session/{me}/s1"),
        "session handle in the client's form"
    );

    // A call with the session handle as argument, answered with an fd.
    let reply = client
        .call_method(
            Some(PORTAL),
            P,
            Some("org.freedesktop.portal.ScreenCast"),
            "OpenPipeWireRemote",
            &(
                ObjectPath::try_from(session.as_str()).unwrap(),
                HashMap::<&str, Value<'_>>::new(),
            ),
        )
        .await
        .unwrap();
    let body = reply.body();
    let (fd,): (Fd<'_>,) = body.deserialize().unwrap();
    let mut text = String::new();
    std::fs::File::from(fd.as_fd().try_clone_to_owned().unwrap())
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "pipewire remote");
    let seen_open = seen.recv().await.unwrap();
    assert!(
        seen_open.starts_with("OpenPipeWireRemote /org/freedesktop/portal/desktop/session/")
            && !seen_open.contains(&format!("/{me}/")),
        "host saw the host form: {seen_open}"
    );

    client
        .call_method(
            Some(PORTAL),
            session.as_str(),
            Some("org.freedesktop.portal.Session"),
            "Close",
            &(),
        )
        .await
        .unwrap();
    let seen_close = seen.recv().await.unwrap();
    assert!(
        !seen_close.contains(&format!("/{me}/")),
        "host saw the host form: {seen_close}"
    );
}

/// A call and the broadcast `NotificationClosed`.
async fn notifications(client: &Connection) {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(NOTIFY)
        .unwrap()
        .build();
    let mut closed = MessageStream::for_match_rule(rule, client, None)
        .await
        .unwrap();
    let reply = client
        .call_method(
            Some(NOTIFY),
            "/org/freedesktop/Notifications",
            Some(NOTIFY),
            "Notify",
            &(
                "koje",
                0u32,
                "",
                "hi",
                "",
                Vec::<&str>::new(),
                HashMap::<&str, Value<'_>>::new(),
                -1i32,
            ),
        )
        .await
        .unwrap();
    assert_eq!(reply.body().deserialize::<(u32,)>().unwrap(), (42,));
    let sig = tokio::time::timeout(Duration::from_secs(5), closed.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        sig.header().member().unwrap().as_str(),
        "NotificationClosed"
    );
    assert_eq!(sig.body().deserialize::<(u32, u32)>().unwrap(), (42, 2));
}

/// Like `GDBus`: resolve the portal's owner, then call that unique name. With
/// notifications forwarded too, this once went to the notification service.
async fn by_unique_name(client: &Connection) {
    let dbus = zbus::fdo::DBusProxy::new(client).await.unwrap();
    let owner = dbus
        .get_name_owner(PORTAL.try_into().unwrap())
        .await
        .unwrap()
        .to_string();
    let reply = client
        .call_method(
            Some(owner.as_str()),
            P,
            Some("org.freedesktop.portal.ScreenCast"),
            "CreateSession",
            &(HashMap::from([("handle_token", Value::from("gdbus1"))]),),
        )
        .await
        .unwrap();
    let (handle,): (OwnedObjectPath,) = reply.body().deserialize().unwrap();
    assert!(handle.as_str().ends_with("/gdbus1"), "{handle}");
}

/// An error from the host comes back as an error.
async fn host_error(client: &Connection) {
    let err = client
        .call_method(
            Some(PORTAL),
            P,
            Some("org.freedesktop.portal.Nope"),
            "Nope",
            &(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("UnknownMethod"), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn forwards_portal_and_notifications() {
    let (Some(host), Some(ctx)) = (Bus::start(), Bus::start()) else {
        eprintln!("dbus-daemon not installed: skipping");
        return;
    };
    let (host_conn, mut seen) = fake_host(&host.address).await;
    let _forwarder = Forwarder(
        Command::new(env!("CARGO_BIN_EXE_koje-forward"))
            .args(["--host", &host.address, NOTIFY, PORTAL])
            .env("DBUS_SESSION_BUS_ADDRESS", &ctx.address)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = zbus::connection::Builder::address(ctx.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    wait_for_name(&client, PORTAL).await;
    let host_names_before = unique_names(&host_conn).await;

    screencast_round_trip(&client, &mut seen).await;
    by_unique_name(&client).await;
    notifications(&client).await;
    host_error(&client).await;

    // The client leaves: its host connection closes.
    let with_client = unique_names(&host_conn).await;
    assert_eq!(
        with_client,
        host_names_before + 1,
        "one host connection for the client"
    );
    client.close().await.unwrap();
    let mut closed_on_host = false;
    for _ in 0..50 {
        if unique_names(&host_conn).await < with_client {
            closed_on_host = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(closed_on_host, "the client's host connection should close");
}
