//! What goes over the bus, as pure conversions between koje-core's port and
//! `io.koje.Extension1` (`gnome/extension/koje@koje.io/dbus/`).
//!
//! A window's address is its Mutter id (`Meta.Window.get_id()`) in decimal.

use std::collections::HashMap;

use koje_core::layout::{Placement, Rect};
use koje_core::ports::{CompositorCommand, WindowInfo};
use koje_core::snap::MonitorArea;
use serde::{Deserialize, Serialize};
use zbus::zvariant::{Type, Value};

use crate::{Error, Result};

/// One entry of `Windows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Window {
    pub id: u64,
    pub pid: u32,
    pub app_id: String,
    pub title: String,
    pub workspace: String,
    pub monitor: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl From<Window> for WindowInfo {
    fn from(w: Window) -> Self {
        Self {
            address: w.id.to_string(),
            pid: w.pid,
            app_id: w.app_id,
            title: w.title,
            workspace: w.workspace,
            placement: Placement {
                monitor: w.monitor,
                rect: Rect {
                    x: w.x,
                    y: w.y,
                    w: w.width,
                    h: w.height,
                },
            },
        }
    }
}

/// One entry of `Monitors`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Monitor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl From<Monitor> for MonitorArea {
    fn from(m: Monitor) -> Self {
        Self {
            name: m.name,
            width: m.width,
            height: m.height,
            reserved: [m.left, m.top, m.right, m.bottom],
        }
    }
}

/// One command of `Apply`: a verb and its arguments.
pub type Command = (String, HashMap<String, Value<'static>>);

/// A window address as the extension's id.
pub fn window_id(address: &str) -> Result<u64> {
    address
        .parse()
        .map_err(|_| Error::InvalidAddress(address.to_owned()))
}

/// The address of a window id; `None` for 0 (no window).
pub fn address(id: u64) -> Option<String> {
    (id != 0).then(|| id.to_string())
}

/// The bus form of one command.
pub fn command(c: &CompositorCommand) -> Result<Command> {
    use CompositorCommand as C;
    let window = |a: &str| Ok::<_, Error>(("window", Value::from(window_id(a)?)));
    let text = |k: &'static str, v: &str| (k, Value::from(v.to_owned()));
    let color = |k: &'static str, c: koje_core::color::Color| (k, Value::from(c.to_string()));
    let (verb, args) = match c {
        C::ContextRule {
            tag,
            active,
            inactive,
        } => (
            "context-rule",
            vec![
                text("tag", tag),
                color("active", *active),
                color("inactive", *inactive),
            ],
        ),
        C::BorderColors {
            address,
            active,
            inactive,
        } => (
            "border-colors",
            vec![
                window(address)?,
                color("active", *active),
                color("inactive", *inactive),
            ],
        ),
        C::TagWindow { address, tag } => ("tag-window", vec![window(address)?, text("tag", tag)]),
        C::Place { address, placement } => {
            let r = placement.rect;
            (
                "place",
                vec![
                    window(address)?,
                    text("monitor", &placement.monitor),
                    ("x", Value::from(r.x)),
                    ("y", Value::from(r.y)),
                    ("width", Value::from(r.w)),
                    ("height", Value::from(r.h)),
                ],
            )
        }
        C::MoveToWorkspace { address, workspace } => (
            "move-to-workspace",
            vec![window(address)?, text("workspace", workspace)],
        ),
        C::FocusWindow { address } => ("focus-window", vec![window(address)?]),
        C::ToggleSpecial { name } => ("toggle-special", vec![text("name", name)]),
        C::CloseWindow { address } => ("close-window", vec![window(address)?]),
        C::FocusWorkspace { workspace } => ("focus-workspace", vec![text("workspace", workspace)]),
        C::CarryToWorkspace { address, workspace } => (
            "carry-to-workspace",
            vec![window(address)?, text("workspace", workspace)],
        ),
    };
    Ok((
        verb.to_owned(),
        args.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    ))
}

pub fn commands(cs: &[CompositorCommand]) -> Result<Vec<Command>> {
    cs.iter().map(command).collect()
}

#[cfg(test)]
mod tests {
    use koje_core::color::Color;

    use super::*;

    fn args(c: &Command) -> Vec<(String, String)> {
        let mut out: Vec<_> =
            c.1.iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect();
        out.sort();
        out
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn place_is_relative_to_its_monitor() {
        let c = command(&CompositorCommand::Place {
            address: "42".into(),
            placement: Placement {
                monitor: "DP-1".into(),
                rect: Rect {
                    x: -5,
                    y: 32,
                    w: 800,
                    h: 600,
                },
            },
        })
        .unwrap();
        assert_eq!(c.0, "place");
        assert_eq!(
            args(&c),
            pairs(&[
                ("height", "uint32 600"),
                ("monitor", "\"DP-1\""),
                ("width", "uint32 800"),
                ("window", "uint64 42"),
                ("x", "-5"),
                ("y", "32"),
            ])
        );
    }

    #[test]
    fn colors_go_as_hex() {
        let c = command(&CompositorCommand::ContextRule {
            tag: "koje-work".into(),
            active: "#3584e4".parse::<Color>().unwrap(),
            inactive: "#112233".parse::<Color>().unwrap(),
        })
        .unwrap();
        assert_eq!(c.0, "context-rule");
        assert_eq!(
            args(&c),
            pairs(&[
                ("active", "\"#3584E4\""),
                ("inactive", "\"#112233\""),
                ("tag", "\"koje-work\""),
            ])
        );
    }

    #[test]
    fn every_verb() {
        use CompositorCommand as C;
        let a = || "7".to_owned();
        let red = "#ff0000".parse::<Color>().unwrap();
        let verbs: Vec<String> = commands(&[
            C::BorderColors {
                address: a(),
                active: red,
                inactive: red,
            },
            C::TagWindow {
                address: a(),
                tag: "t".into(),
            },
            C::MoveToWorkspace {
                address: a(),
                workspace: "work".into(),
            },
            C::FocusWindow { address: a() },
            C::ToggleSpecial { name: "s".into() },
            C::CloseWindow { address: a() },
            C::FocusWorkspace {
                workspace: "1".into(),
            },
            C::CarryToWorkspace {
                address: a(),
                workspace: "work".into(),
            },
        ])
        .unwrap()
        .into_iter()
        .map(|c| c.0)
        .collect();
        assert_eq!(
            verbs,
            [
                "border-colors",
                "tag-window",
                "move-to-workspace",
                "focus-window",
                "toggle-special",
                "close-window",
                "focus-workspace",
                "carry-to-workspace"
            ]
        );
    }

    #[test]
    fn addresses_are_decimal_ids() {
        assert_eq!(window_id("12345").unwrap(), 12345);
        for bad in ["0x55d1", "", "-1", "1 "] {
            assert!(
                matches!(window_id(bad), Err(Error::InvalidAddress(_))),
                "{bad}"
            );
        }
        assert_eq!(address(0), None);
        assert_eq!(address(9).as_deref(), Some("9"));
    }
}
