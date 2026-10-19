//! The adapter against fake Hyprland sockets in a temporary directory.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::time::Duration;

use koje_core::color::Color;
use koje_core::ports::{Compositor as _, CompositorCommand, CompositorEvent};
use koje_hyprland::{Error, Hyprland};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

const MONITORS: &str = r#"[{"id": 0, "name": "DP-1", "x": 0, "y": 0}]"#;
const CLIENTS: &str = r#"[{"address": "0xab", "at": [10, 20], "size": [300, 200],
    "workspace": {"id": 1, "name": "1"}, "monitor": 0, "class": "foot",
    "title": "~", "pid": 7}]"#;

/// A command socket that answers like Hyprland and reports each request.
fn fake_commands(dir: &Path) -> mpsc::UnboundedReceiver<String> {
    let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 4096];
            let n = s.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let reply = match request.as_str() {
                "j/monitors" => MONITORS.to_owned(),
                "j/clients" => CLIENTS.to_owned(),
                r if r.contains("bad") => "ok\n\ninvalid field\n\n".to_owned(),
                r => "ok\n\n".repeat(r.matches(';').count() + 1),
            };
            s.write_all(reply.as_bytes()).await.unwrap();
            tx.send(request).unwrap();
        }
    });
    rx
}

#[tokio::test(flavor = "current_thread")]
async fn windows_and_batches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut seen = fake_commands(tmp.path());
    let h = Hyprland::at(tmp.path());

    let w = h.windows().await.unwrap();
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].address.as_str(), w[0].pid), ("0xab", 7));
    assert_eq!(seen.recv().await.unwrap(), "j/monitors");
    assert_eq!(seen.recv().await.unwrap(), "j/clients");

    h.apply(&[
        CompositorCommand::ContextRule {
            tag: "koje-a".into(),
            active: Color::rgb(255, 0, 0),
            inactive: Color::rgb(128, 0, 0),
        },
        CompositorCommand::TagWindow {
            address: "0xab".into(),
            tag: "koje-a".into(),
        },
    ])
    .await
    .unwrap();
    assert_eq!(
        seen.recv().await.unwrap(),
        "[[BATCH]]keyword windowrule match:tag koje-a, border_color rgb(ff0000) rgb(800000);\
         dispatch tagwindow +koje-a address:0xab"
    );

    let err = h
        .apply(&[
            CompositorCommand::FocusWorkspace {
                workspace: "a".into(),
            },
            CompositorCommand::FocusWorkspace {
                workspace: "bad".into(),
            },
        ])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Rejected { reply, .. } if reply == "invalid field"),
        "{err}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn no_hyprland() {
    let tmp = tempfile::tempdir().unwrap();
    let err = Hyprland::at(tmp.path()).windows().await.unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn events_reconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".socket2.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut events = Hyprland::at(tmp.path()).events();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(b"workspace>>2\nopenwindow>>ab,2,foot,foot\nclosewindow>>ab\n")
            .await
            .unwrap();
        drop(s);
        // Hyprland restarts: a new socket at the same place.
        drop(listener);
        std::fs::remove_file(&path).unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        let listener = UnixListener::bind(&path).unwrap();
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(b"configreloaded>>\n").await.unwrap();
        s
    });
    let mut next = async || {
        tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .unwrap()
    };
    assert_eq!(next().await, CompositorEvent::Connected);
    assert!(matches!(
        next().await,
        CompositorEvent::WindowOpened { address, .. } if address == "0xab"
    ));
    assert_eq!(
        next().await,
        CompositorEvent::WindowClosed {
            address: "0xab".into()
        }
    );
    assert_eq!(next().await, CompositorEvent::Connected);
    assert_eq!(next().await, CompositorEvent::ConfigReloaded);
    drop(server.await.unwrap());
}
