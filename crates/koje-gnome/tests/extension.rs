//! The adapter against koje's real GNOME Shell extension. In the GNOME lab
//! (docs/spikes/setup/gnome-lab):
//!
//! ```sh
//! DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/koje-gnome-lab/run/bus \
//!   just test -p koje-gnome -- --ignored
//! ```
//!
//! It switches to a workspace `koje-test` and back; it moves no windows.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use koje_core::ports::{Compositor as _, CompositorCommand, CompositorEvent};
use koje_gnome::Gnome;

async fn next_focus(events: &mut koje_gnome::Events) -> String {
    loop {
        let next = tokio::time::timeout(Duration::from_secs(5), events.next());
        if let CompositorEvent::WorkspaceFocused { name } = next.await.unwrap() {
            return name;
        }
    }
}

#[tokio::test]
#[ignore = "needs koje's GNOME Shell extension on the session bus (GNOME lab)"]
async fn against_the_extension() {
    let gnome = Gnome::session().await.unwrap();
    let monitors = gnome.monitors().await.unwrap();
    assert_ne!(monitors.len(), 0);
    for w in gnome.windows().await.unwrap() {
        w.address.parse::<u64>().unwrap();
        assert!(
            monitors.iter().any(|m| m.name == w.placement.monitor),
            "{w:?}"
        );
    }
    let before = gnome.active_workspace().await.unwrap();

    let mut events = gnome.events();
    assert_eq!(events.next().await, CompositorEvent::Connected);
    let focus = |workspace: &str| {
        [CompositorCommand::FocusWorkspace {
            workspace: workspace.to_owned(),
        }]
    };
    gnome.apply(&focus("koje-test")).await.unwrap();
    assert_eq!(next_focus(&mut events).await, "koje-test");
    assert_eq!(gnome.active_workspace().await.unwrap(), "koje-test");
    gnome.apply(&focus(&before)).await.unwrap();
    assert_eq!(next_focus(&mut events).await, before);

    let e = gnome
        .apply(&[CompositorCommand::FocusWindow {
            address: "1".into(),
        }])
        .await
        .unwrap_err();
    assert!(e.to_string().contains("no window 1"), "{e}");
}
