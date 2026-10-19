//! Hyprland's `j/clients` and `j/monitors` replies, turned into
//! [`WindowInfo`]s. Placements are relative to the window's monitor.

use koje_core::layout::{Placement, Rect};
use koje_core::ports::WindowInfo;
use serde::Deserialize;

/// The fields koje reads from `j/monitors`. Positions are logical pixels.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub name: String,
}

/// The fields koje reads from `j/clients`.
#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    pub address: String,
    #[serde(default = "yes")]
    pub mapped: bool,
    pub at: [i32; 2],
    pub size: [i32; 2],
    pub workspace: Workspace,
    pub monitor: i64,
    pub class: String,
    pub title: String,
    pub pid: i64,
}

fn yes() -> bool {
    true
}

/// `63b2…` → `0x63b2…`: events omit the prefix that `j/clients` has.
pub fn normalize_address(a: &str) -> String {
    if a.starts_with("0x") {
        a.to_owned()
    } else {
        format!("0x{a}")
    }
}

/// Mapped windows with their placement on their monitor.
pub fn windows(clients: &[Client], monitors: &[Monitor]) -> Vec<WindowInfo> {
    clients
        .iter()
        .filter(|c| c.mapped)
        .map(|c| {
            let monitor = monitors.iter().find(|m| m.id == c.monitor);
            let (mx, my) = monitor.map_or((0, 0), |m| (m.x, m.y));
            WindowInfo {
                address: normalize_address(&c.address),
                pid: u32::try_from(c.pid).unwrap_or(0),
                app_id: c.class.clone(),
                title: c.title.clone(),
                workspace: c.workspace.name.clone(),
                placement: Placement {
                    monitor: monitor.map(|m| m.name.clone()).unwrap_or_default(),
                    rect: Rect {
                        x: c.at[0].saturating_sub(mx),
                        y: c.at[1].saturating_sub(my),
                        w: u32::try_from(c.size[0]).unwrap_or(0),
                        h: u32::try_from(c.size[1]).unwrap_or(0),
                    },
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a Hyprland 0.53.3 reply (M2.2).
    const CLIENTS: &str = r#"[{
        "address": "0x63b2bc83e780", "mapped": true, "hidden": false,
        "at": [2600, 70], "size": [800, 600],
        "workspace": {"id": -98, "name": "special:koje-stash"},
        "floating": true, "monitor": 1, "class": "foot", "title": "~",
        "initialClass": "foot", "pid": 41234, "xwayland": false, "tags": []
    }, {
        "address": "0x63b2bc7d7f70", "mapped": false, "hidden": false,
        "at": [0, 0], "size": [0, 0], "workspace": {"id": 1, "name": "1"},
        "floating": true, "monitor": 0, "class": "", "title": "", "pid": -1
    }]"#;
    const MONITORS: &str = r#"[
        {"id": 0, "name": "DP-1", "x": 0, "y": 0, "width": 2560, "scale": 1.0},
        {"id": 1, "name": "HDMI-A-1", "x": 2560, "y": 0, "width": 1920, "scale": 1.0}
    ]"#;

    #[test]
    fn parses_and_converts() {
        let clients: Vec<Client> = serde_json::from_str(CLIENTS).unwrap();
        let monitors: Vec<Monitor> = serde_json::from_str(MONITORS).unwrap();
        let w = windows(&clients, &monitors);
        assert_eq!(
            w,
            [WindowInfo {
                address: "0x63b2bc83e780".into(),
                pid: 41234,
                app_id: "foot".into(),
                title: "~".into(),
                workspace: "special:koje-stash".into(),
                placement: Placement {
                    monitor: "HDMI-A-1".into(),
                    rect: Rect {
                        x: 40,
                        y: 70,
                        w: 800,
                        h: 600
                    },
                },
            }]
        );
    }

    #[test]
    fn addresses() {
        assert_eq!(normalize_address("63b2"), "0x63b2");
        assert_eq!(normalize_address("0x63b2"), "0x63b2");
    }
}
