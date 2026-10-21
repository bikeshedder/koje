//! M4.8: the adapter against the running Hyprland. Ignored by default:
//!
//!   cargo test -p koje-hyprland --test live -- --ignored
//!
//! Needs Hyprland (`HYPRLAND_INSTANCE_SIGNATURE`, or the newest instance) and a
//! terminal on the host (foot, kitty or alacritty). The tests only touch the
//! window they start themselves and close it again; they never focus anything.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use koje_core::color::Color;
use koje_core::ports::{Compositor as _, CompositorCommand, CompositorEvent, WindowInfo};
use koje_hyprland::{Events, Hyprland};

/// A test window, killed on drop.
struct TestWindow(Child);

impl Drop for TestWindow {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start a terminal with a unique app id.
fn spawn(app_id: &str) -> TestWindow {
    let candidates: [(&str, Vec<String>); 3] = [
        ("foot", vec![format!("--app-id={app_id}")]),
        ("kitty", vec!["--class".into(), app_id.into()]),
        ("alacritty", vec!["--class".into(), app_id.into()]),
    ];
    for (program, args) in candidates {
        if let Ok(child) = Command::new(program)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            return TestWindow(child);
        }
    }
    panic!("needs foot, kitty or alacritty on the host");
}

async fn opened(events: &mut Events, app_id: &str) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let CompositorEvent::WindowOpened { address, class, .. } = events.next().await
                && class == app_id
            {
                return address;
            }
        }
    })
    .await
    .expect("the test window did not open within 10 s")
}

async fn window(h: &Hyprland, address: &str) -> WindowInfo {
    h.windows()
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.address == address)
        .expect("the test window is in j/clients")
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a running Hyprland and a terminal on the host"]
async fn requests_and_events_against_hyprland() {
    let h = Hyprland::from_env();
    let mut events = h.events();
    assert_eq!(events.next().await, CompositorEvent::Connected);

    let app_id = format!("koje-test-{}", std::process::id());
    let tag = format!("{app_id}-tag");
    let workspace = format!("{app_id}-ws");
    let child = spawn(&app_id);
    let address = opened(&mut events, &app_id).await;
    let w = window(&h, &address).await;
    assert_eq!(w.app_id, app_id);
    assert_eq!(
        w.pid,
        child.0.id(),
        "the window's pid is the process koje started"
    );
    assert!(
        w.placement.rect.w > 0 && !w.placement.monitor.is_empty(),
        "{w:?}"
    );

    // Every request kojed sends, in one batch: Hyprland answers ok to each.
    let red = Color::rgb(0xe0, 0x6c, 0x75);
    h.apply(&[
        CompositorCommand::ContextRule {
            tag: tag.clone(),
            active: red,
            inactive: red.dimmed(),
        },
        CompositorCommand::TagWindow {
            address: address.clone(),
            tag: tag.clone(),
        },
        CompositorCommand::BorderColors {
            address: address.clone(),
            active: red,
            inactive: red.dimmed(),
        },
        CompositorCommand::MoveToWorkspace {
            address: address.clone(),
            workspace: workspace.clone(),
        },
    ])
    .await
    .unwrap();
    let clients = h.request("j/clients").await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&clients).unwrap();
    let me = json
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["address"] == address.as_str())
        .unwrap();
    let tags: Vec<&str> = me["tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert!(
        tags.iter().any(|t| t.trim_end_matches('*') == tag),
        "tagged: {tags:?}"
    );
    assert_eq!(window(&h, &address).await.workspace, workspace);

    // A request Hyprland rejects comes back as an error, not as success.
    assert!(
        h.request("dispatch koje-no-such-dispatcher")
            .await
            .is_ok_and(|r| r.trim() != "ok")
    );

    drop(child);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if events.next().await
                == (CompositorEvent::WindowClosed {
                    address: address.clone(),
                })
            {
                return;
            }
        }
    })
    .await
    .expect("closewindow for the test window");
}
