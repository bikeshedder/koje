//! Hyprland's event socket (`.socket2.sock`): a pure line parser and a reader
//! that reconnects when Hyprland goes away (restart, crash) and reports that
//! as [`CompositorEvent::Reconnected`].

use std::path::PathBuf;
use std::time::Duration;

use koje_core::ports::CompositorEvent;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::net::UnixStream;

use crate::clients::normalize_address;
use crate::instance;

const FIRST_RETRY: Duration = Duration::from_millis(50);
const MAX_RETRY: Duration = Duration::from_secs(2);

/// One socket2 line (`name>>data`) as an event; `None` for events koje ignores.
pub fn parse_line(line: &str) -> Option<CompositorEvent> {
    let (name, data) = line.trim_end_matches(['\n', '\r']).split_once(">>")?;
    let address = |a: &str| (!a.is_empty()).then(|| normalize_address(a));
    Some(match name {
        "openwindow" => {
            let mut f = data.splitn(4, ',');
            CompositorEvent::WindowOpened {
                address: address(f.next()?)?,
                workspace: f.next()?.to_owned(),
                class: f.next()?.to_owned(),
                title: f.next().unwrap_or_default().to_owned(),
            }
        }
        "closewindow" => CompositorEvent::WindowClosed {
            address: address(data)?,
        },
        "activewindowv2" => CompositorEvent::WindowFocused {
            address: address(data.trim_matches(',')),
        },
        "windowtitlev2" => {
            let (a, title) = data.split_once(',')?;
            CompositorEvent::WindowTitle {
                address: address(a)?,
                title: title.to_owned(),
            }
        }
        "configreloaded" => CompositorEvent::ConfigReloaded,
        _ => return None,
    })
}

/// The event stream; [`Events::next`] never ends, it waits for Hyprland.
#[derive(Debug)]
pub struct Events {
    fixed: Option<PathBuf>,
    reader: Option<BufReader<UnixStream>>,
    /// Connected before: the next connection is a reconnect.
    was_connected: bool,
    retry: Duration,
}

impl Events {
    pub fn from_env() -> Self {
        Self::new(None)
    }

    pub fn at(dir: PathBuf) -> Self {
        Self::new(Some(dir))
    }

    fn new(fixed: Option<PathBuf>) -> Self {
        Self {
            fixed,
            reader: None,
            was_connected: false,
            retry: FIRST_RETRY,
        }
    }

    async fn connect(&mut self) -> BufReader<UnixStream> {
        loop {
            let dir = match &self.fixed {
                Some(d) => Ok(d.clone()),
                None => instance::from_env(),
            };
            match dir {
                Ok(dir) => match UnixStream::connect(dir.join(instance::EVENT_SOCKET)).await {
                    Ok(s) => {
                        self.retry = FIRST_RETRY;
                        tracing::debug!(instance = %dir.display(), "following Hyprland events");
                        return BufReader::new(s);
                    }
                    Err(e) => tracing::debug!("Hyprland event socket: {e}"),
                },
                Err(e) => tracing::debug!("{e}"),
            }
            tokio::time::sleep(self.retry).await;
            self.retry = (self.retry * 2).min(MAX_RETRY);
        }
    }

    /// The next event koje cares about.
    pub async fn next(&mut self) -> CompositorEvent {
        let mut line = Vec::new();
        loop {
            if self.reader.is_none() {
                let r = self.connect().await;
                self.reader = Some(r);
                if self.was_connected {
                    return CompositorEvent::Reconnected;
                }
                self.was_connected = true;
            }
            let Some(reader) = &mut self.reader else {
                continue;
            };
            line.clear();
            match reader.read_until(b'\n', &mut line).await {
                Ok(0) | Err(_) => {
                    tracing::info!("lost the Hyprland event socket, reconnecting");
                    self.reader = None;
                }
                Ok(_) => {
                    if let Some(event) = parse_line(&String::from_utf8_lossy(&line)) {
                        return event;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_lines() {
        // Lines from the M2.2 report (Hyprland 0.53.3).
        assert_eq!(
            parse_line("openwindow>>63b2bc83e780,2,foot,foot"),
            Some(CompositorEvent::WindowOpened {
                address: "0x63b2bc83e780".into(),
                workspace: "2".into(),
                class: "foot".into(),
                title: "foot".into(),
            })
        );
        assert_eq!(
            parse_line("closewindow>>63b2bc83e780\n"),
            Some(CompositorEvent::WindowClosed {
                address: "0x63b2bc83e780".into()
            })
        );
        assert_eq!(
            parse_line("activewindowv2>>63b2bc7d7f70"),
            Some(CompositorEvent::WindowFocused {
                address: Some("0x63b2bc7d7f70".into())
            })
        );
        assert_eq!(
            parse_line("windowtitlev2>>63b2bc83e780,foot"),
            Some(CompositorEvent::WindowTitle {
                address: "0x63b2bc83e780".into(),
                title: "foot".into()
            })
        );
        assert_eq!(
            parse_line("configreloaded>>"),
            Some(CompositorEvent::ConfigReloaded)
        );
    }

    #[test]
    fn edge_cases() {
        // Titles may contain commas and `>>`.
        assert_eq!(
            parse_line("openwindow>>a1,name:work,firefox,a, b >> c"),
            Some(CompositorEvent::WindowOpened {
                address: "0xa1".into(),
                workspace: "name:work".into(),
                class: "firefox".into(),
                title: "a, b >> c".into(),
            })
        );
        assert_eq!(
            parse_line("windowtitlev2>>a1,x, y"),
            Some(CompositorEvent::WindowTitle {
                address: "0xa1".into(),
                title: "x, y".into()
            })
        );
        // Nothing focused.
        for line in ["activewindowv2>>", "activewindowv2>>,"] {
            assert_eq!(
                parse_line(line),
                Some(CompositorEvent::WindowFocused { address: None })
            );
        }
        for ignored in [
            "activewindow>>foot,~",
            "workspace>>2",
            "openwindow>>a1,2",
            "closewindow>>",
            "garbage",
        ] {
            assert_eq!(parse_line(ignored), None, "{ignored}");
        }
    }
}
