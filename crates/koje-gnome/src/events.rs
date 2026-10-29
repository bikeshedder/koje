//! The extension's signals as [`CompositorEvent`]s. Like Hyprland's event
//! socket, the stream never ends and reports [`CompositorEvent::Connected`]
//! first and whenever the extension appears on the bus again (the Shell
//! restarted, the extension was enabled): windows may have changed unseen.

use std::time::Duration;

use futures_util::stream::{BoxStream, SelectAll, StreamExt as _};
use koje_core::ports::CompositorEvent;

use crate::{ExtensionProxy, wire};

const FIRST_RETRY: Duration = Duration::from_millis(50);
const MAX_RETRY: Duration = Duration::from_secs(2);

/// One item of the merged signal streams.
enum Item {
    Event(CompositorEvent),
    /// The extension's bus name got an owner (`true`) or lost it.
    Owner(bool),
}

/// The event stream; [`Events::next`] never ends.
#[derive(Debug)]
pub struct Events {
    conn: zbus::Connection,
    streams: Option<Streams>,
    retry: Duration,
}

struct Streams(SelectAll<BoxStream<'static, Item>>);

impl std::fmt::Debug for Streams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Streams")
    }
}

impl Events {
    pub fn new(conn: zbus::Connection) -> Self {
        Self {
            conn,
            streams: None,
            retry: FIRST_RETRY,
        }
    }

    async fn subscribe(&self) -> zbus::Result<Streams> {
        let proxy = ExtensionProxy::builder(&self.conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        let mut all = SelectAll::new();
        all.push(
            proxy
                .receive_window_opened()
                .await?
                .filter_map(|s| async move {
                    let a = s.args().ok()?;
                    Some(Item::Event(CompositorEvent::WindowOpened {
                        address: wire::address(a.window)?,
                        workspace: a.workspace,
                        class: a.app_id,
                        title: a.title,
                    }))
                })
                .boxed(),
        );
        all.push(
            proxy
                .receive_window_closed()
                .await?
                .filter_map(|s| async move {
                    Some(Item::Event(CompositorEvent::WindowClosed {
                        address: wire::address(s.args().ok()?.window)?,
                    }))
                })
                .boxed(),
        );
        all.push(
            proxy
                .receive_window_focused()
                .await?
                .filter_map(|s| async move {
                    Some(Item::Event(CompositorEvent::WindowFocused {
                        address: wire::address(s.args().ok()?.window),
                    }))
                })
                .boxed(),
        );
        all.push(
            proxy
                .receive_window_title()
                .await?
                .filter_map(|s| async move {
                    let a = s.args().ok()?;
                    Some(Item::Event(CompositorEvent::WindowTitle {
                        address: wire::address(a.window)?,
                        title: a.title,
                    }))
                })
                .boxed(),
        );
        all.push(
            proxy
                .receive_workspace_focused()
                .await?
                .filter_map(|s| async move {
                    Some(Item::Event(CompositorEvent::WorkspaceFocused {
                        name: s.args().ok()?.name,
                    }))
                })
                .boxed(),
        );
        // A peer-to-peer connection (tests) has no bus and no owners.
        if self.conn.unique_name().is_some() {
            all.push(
                proxy
                    .inner()
                    .receive_owner_changed()
                    .await?
                    .map(|owner| Item::Owner(owner.is_some()))
                    .boxed(),
            );
        }
        Ok(Streams(all))
    }

    /// The next event koje cares about.
    pub async fn next(&mut self) -> CompositorEvent {
        loop {
            let Some(Streams(streams)) = &mut self.streams else {
                match self.subscribe().await {
                    Ok(s) => {
                        self.streams = Some(s);
                        self.retry = FIRST_RETRY;
                        return CompositorEvent::Connected;
                    }
                    Err(e) => {
                        tracing::debug!("following koje's GNOME Shell extension: {e}");
                        tokio::time::sleep(self.retry).await;
                        self.retry = (self.retry * 2).min(MAX_RETRY);
                        continue;
                    }
                }
            };
            match streams.next().await {
                Some(Item::Event(event)) => return event,
                Some(Item::Owner(true)) => return CompositorEvent::Connected,
                Some(Item::Owner(false)) => {
                    tracing::info!("koje's GNOME Shell extension left the bus");
                }
                None => {
                    tracing::info!("lost the session bus, following the extension again");
                    self.streams = None;
                }
            }
        }
    }
}
