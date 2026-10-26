//! What the bar reads from Hyprland: monitors, workspaces, windows and the
//! focused window, read again after every event that may change them – the
//! bar never keeps a copy that could drift from Hyprland's.

use anyhow::{Context as _, Result};
use koje_hyprland::Hyprland;
use serde::Deserialize;

use crate::model::{Desktop, MonitorState, Window, WorkspaceState};

#[derive(Debug, Deserialize)]
struct NameRef {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Monitor {
    name: String,
    #[serde(rename = "activeWorkspace")]
    active_workspace: NameRef,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    id: i64,
    name: String,
    monitor: String,
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    #[serde(default = "yes")]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    workspace: NameRef,
    class: String,
    title: String,
}

fn yes() -> bool {
    true
}

/// `j/activewindow` is `{}` when nothing has focus.
#[derive(Debug, Deserialize)]
struct Active {
    address: Option<String>,
}

async fn json<T: serde::de::DeserializeOwned>(hypr: &Hyprland, request: &str) -> Result<T> {
    let reply = hypr.request(request).await?;
    serde_json::from_str(&reply).with_context(|| format!("Hyprland's answer to {request}"))
}

/// Read the desktop from Hyprland.
pub async fn desktop(hypr: &Hyprland) -> Result<Desktop> {
    let monitors: Vec<Monitor> = json(hypr, "j/monitors").await?;
    let workspaces: Vec<Workspace> = json(hypr, "j/workspaces").await?;
    let clients: Vec<Client> = json(hypr, "j/clients").await?;
    let active: Active = json(hypr, "j/activewindow").await?;
    Ok(Desktop {
        monitors: monitors
            .into_iter()
            .map(|m| MonitorState {
                name: m.name,
                active_workspace: m.active_workspace.name,
            })
            .collect(),
        workspaces: workspaces
            .into_iter()
            .map(|w| WorkspaceState {
                id: w.id,
                name: w.name,
                monitor: w.monitor,
            })
            .collect(),
        windows: clients
            .into_iter()
            .filter(|c| c.mapped && !c.hidden)
            .map(|c| Window {
                address: koje_hyprland::clients::normalize_address(&c.address),
                workspace: c.workspace.name,
                class: c.class,
                title: c.title,
            })
            .collect(),
        focused: active
            .address
            .map(|a| koje_hyprland::clients::normalize_address(&a)),
    })
}

/// Hyprland events after which the bar reads the desktop again.
pub fn changes_desktop(line: &str) -> bool {
    let name = line.split_once(">>").map_or(line, |(n, _)| n);
    matches!(
        name,
        "workspace"
            | "workspacev2"
            | "focusedmon"
            | "focusedmonv2"
            | "createworkspace"
            | "createworkspacev2"
            | "destroyworkspace"
            | "destroyworkspacev2"
            | "renameworkspace"
            | "moveworkspace"
            | "moveworkspacev2"
            | "openwindow"
            | "closewindow"
            | "movewindow"
            | "movewindowv2"
            | "activewindowv2"
            | "windowtitlev2"
            | "monitoradded"
            | "monitoraddedv2"
            | "monitorremoved"
            | "monitorremovedv2"
            | "configreloaded"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_events_refresh_others_do_not() {
        for line in [
            "workspacev2>>4,kundenportal",
            "movewindowv2>>55d0e8c2a1b0,3,3",
            "windowtitlev2>>55d0e8c2a1b0,new title",
            "closewindow>>55d0e8c2a1b0",
            "configreloaded>>",
        ] {
            assert!(changes_desktop(line), "{line}");
        }
        for line in [
            "activelayout>>kb,English (US)",
            "submap>>resize",
            "screencast>>1,0",
        ] {
            assert!(!changes_desktop(line), "{line}");
        }
    }

    #[test]
    fn replies_parse() {
        let m: Vec<Monitor> = serde_json::from_str(
            r#"[{"id":0,"name":"eDP-1","activeWorkspace":{"id":-1337,"name":"kp"},"focused":true}]"#,
        )
        .unwrap();
        assert_eq!(
            (m[0].name.as_str(), m[0].active_workspace.name.as_str()),
            ("eDP-1", "kp")
        );
        let c: Vec<Client> = serde_json::from_str(
            r#"[{"address":"0x55d0e8c2a1b0","mapped":true,"hidden":false,"workspace":{"id":1,"name":"1"},"class":"foot","title":"~"}]"#,
        )
        .unwrap();
        assert_eq!(c[0].class, "foot");
        let none: Active = serde_json::from_str("{}").unwrap();
        assert_eq!(none.address, None);
    }
}
