//! The adapter against the fake extension, and the fake against the
//! interface file the extension loads.

use std::collections::BTreeSet;
use std::time::Duration;

use koje_core::layout::{Placement, Rect};
use koje_core::ports::{Compositor as _, CompositorCommand, CompositorEvent, WindowInfo};
use koje_core::snap::MonitorArea;

use crate::fake::{self, Fake};
use crate::{Error, Gnome, OBJECT_PATH, wire};

fn window(id: u64, workspace: &str) -> wire::Window {
    wire::Window {
        id,
        pid: 4242,
        app_id: "org.gnome.Ptyxis".into(),
        title: "Terminal".into(),
        workspace: workspace.into(),
        monitor: "DP-1".into(),
        x: 10,
        y: 42,
        width: 800,
        height: 600,
    }
}

#[tokio::test]
async fn queries() {
    let fake = Fake {
        windows: vec![window(17, "work"), window(18, "1")],
        monitors: vec![wire::Monitor {
            name: "DP-1".into(),
            width: 2560,
            height: 1440,
            left: 0,
            top: 32,
            right: 0,
            bottom: 0,
        }],
        active: "work".into(),
        ..Fake::default()
    };
    let (_server, client) = fake::serve(fake).await;
    let gnome = Gnome::new(client);

    let windows = gnome.windows().await.unwrap();
    assert_eq!(
        windows[0],
        WindowInfo {
            address: "17".into(),
            pid: 4242,
            app_id: "org.gnome.Ptyxis".into(),
            title: "Terminal".into(),
            workspace: "work".into(),
            placement: Placement {
                monitor: "DP-1".into(),
                rect: Rect {
                    x: 10,
                    y: 42,
                    w: 800,
                    h: 600
                },
            },
        }
    );
    assert_eq!(windows[1].workspace, "1");
    assert_eq!(gnome.active_workspace().await.unwrap(), "work");
    assert_eq!(
        gnome.monitors().await.unwrap(),
        [MonitorArea {
            name: "DP-1".into(),
            width: 2560,
            height: 1440,
            reserved: [0, 32, 0, 0],
        }]
    );
}

#[tokio::test]
async fn apply_sends_one_batch() {
    let fake = Fake::default();
    let applied = fake.applied.clone();
    let (_server, client) = fake::serve(fake).await;
    let gnome = Gnome::new(client);

    gnome
        .apply(&[
            CompositorCommand::MoveToWorkspace {
                address: "17".into(),
                workspace: "work".into(),
            },
            CompositorCommand::FocusWorkspace {
                workspace: "work".into(),
            },
        ])
        .await
        .unwrap();
    gnome.apply(&[]).await.unwrap();
    assert_eq!(
        *applied.lock().unwrap(),
        [
            (
                "move-to-workspace".to_owned(),
                vec![
                    ("window".to_owned(), "uint64 17".to_owned()),
                    ("workspace".to_owned(), "\"work\"".to_owned()),
                ]
            ),
            (
                "focus-workspace".to_owned(),
                vec![("workspace".to_owned(), "\"work\"".to_owned())]
            ),
        ]
    );
}

#[tokio::test]
async fn rejections_and_bad_addresses_are_errors() {
    let (_server, client) = fake::serve(Fake::default()).await;
    let gnome = Gnome::new(client);
    let e = gnome
        .apply(&[CompositorCommand::ToggleSpecial {
            name: "stash-work".into(),
        }])
        .await
        .unwrap_err();
    assert!(e.to_string().contains("not available on GNOME"), "{e}");
    // A Hyprland address never reaches the bus.
    let e = gnome
        .apply(&[CompositorCommand::FocusWindow {
            address: "0x55d1".into(),
        }])
        .await
        .unwrap_err();
    assert!(matches!(e, Error::InvalidAddress(_)), "{e}");
}

#[tokio::test]
async fn signals_become_events() {
    let (server, client) = fake::serve(Fake::default()).await;
    let mut events = Gnome::new(client).events();
    assert_eq!(events.next().await, CompositorEvent::Connected);

    let iface = server
        .object_server()
        .interface::<_, Fake>(OBJECT_PATH)
        .await
        .unwrap();
    let e = iface.signal_emitter();
    Fake::window_opened(e, 17, "1", "firefox", "Mozilla Firefox")
        .await
        .unwrap();
    Fake::window_title(e, 17, "koje – GitHub").await.unwrap();
    Fake::window_focused(e, 17).await.unwrap();
    Fake::window_focused(e, 0).await.unwrap();
    Fake::workspace_focused(e, "work").await.unwrap();
    Fake::window_closed(e, 17).await.unwrap();

    let mut got = Vec::new();
    for _ in 0..6 {
        let next = tokio::time::timeout(Duration::from_secs(5), events.next());
        got.push(next.await.unwrap());
    }
    // One stream per signal: their order between signals is not kept.
    let expected = [
        CompositorEvent::WindowOpened {
            address: "17".into(),
            workspace: "1".into(),
            class: "firefox".into(),
            title: "Mozilla Firefox".into(),
        },
        CompositorEvent::WindowTitle {
            address: "17".into(),
            title: "koje – GitHub".into(),
        },
        CompositorEvent::WindowFocused {
            address: Some("17".into()),
        },
        CompositorEvent::WindowFocused { address: None },
        CompositorEvent::WorkspaceFocused {
            name: "work".into(),
        },
        CompositorEvent::WindowClosed {
            address: "17".into(),
        },
    ];
    for e in &expected {
        assert!(got.contains(e), "{e:?} missing in {got:?}");
    }
}

/// `method Name(in sig)(out sig)` / `signal Name(sig)()` for every member of
/// `io.koje.Extension1` in introspection XML.
fn members(xml: &str) -> BTreeSet<String> {
    use quick_xml::events::{BytesStart, Event};
    fn attr(e: &BytesStart<'_>, name: &str) -> String {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .unwrap()
                    .into_owned()
            })
            .unwrap_or_default()
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = BTreeSet::new();
    let mut ours = false;
    // Name, in signature, out signature of the open member.
    let mut member: Option<(String, String, String)> = None;
    loop {
        let (e, empty) = match reader.read_event().unwrap() {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if matches!(e.name().as_ref(), "method" | "signal")
                    && let Some((name, ins, outs)) = member.take()
                {
                    out.insert(format!("{name}({ins})({outs})"));
                }
                continue;
            }
            Event::Eof => return out,
            _ => continue,
        };
        match e.name().as_ref() {
            "interface" => ours = attr(&e, "name") == "io.koje.Extension1",
            kind @ ("method" | "signal") if ours => {
                let name = format!("{kind} {}", attr(&e, "name"));
                if empty {
                    out.insert(format!("{name}()()"));
                } else {
                    member = Some((name, String::new(), String::new()));
                }
            }
            "arg" => {
                if let Some((_, ins, outs)) = &mut member {
                    let into = if attr(&e, "direction") == "out" {
                        outs
                    } else {
                        ins
                    };
                    into.push_str(&attr(&e, "type"));
                }
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn the_fake_matches_the_extensions_interface() {
    let file = include_str!("../../../gnome/src/extension/dbus/io.koje.Extension1.xml");
    let (_server, client) = fake::serve(Fake::default()).await;
    let introspect = zbus::fdo::IntrospectableProxy::builder(&client)
        .destination(crate::BUS_NAME)
        .unwrap()
        .path(OBJECT_PATH)
        .unwrap()
        .build()
        .await
        .unwrap();
    let served = introspect.introspect().await.unwrap();
    let expected = members(file);
    assert_eq!(expected.len(), 9, "{expected:#?}");
    assert_eq!(members(&served), expected);
}
