//! What the shell reads from Hyprland: monitors, workspaces and windows, read again
//! after every event that may change them – the bar never keeps a copy that
//! could drift from Hyprland's.

use anyhow::{Context as _, Result};
use koje_hyprland::Hyprland;
use serde::Deserialize;

use crate::model::{Desktop, MonitorState, WindowState, WorkspaceState};

#[derive(Debug, Deserialize)]
struct NameRef {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Monitor {
    name: String,
    #[serde(rename = "activeWorkspace")]
    active_workspace: NameRef,
    #[serde(default)]
    focused: bool,
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
    #[serde(default)]
    at: (i32, i32),
    #[serde(default)]
    size: (i32, i32),
    workspace: NameRef,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    /// 0 for the focused window, 1 for the one before, …
    #[serde(rename = "focusHistoryID", default)]
    focus_history: i32,
    /// kojed tags a context's windows `koje-<context>`; Hyprland marks tags
    /// from rules with a trailing `*`.
    #[serde(default)]
    tags: Vec<String>,
}

/// The context a window's tags name.
fn tagged_context(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.trim_end_matches('*').strip_prefix("koje-"))
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
}

fn yes() -> bool {
    true
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
    let plugins: Vec<Plugin> = json(hypr, "j/plugin list").await.unwrap_or_default();
    Ok(Desktop {
        monitors: monitors
            .into_iter()
            .map(|m| MonitorState {
                name: m.name,
                active_workspace: m.active_workspace.name,
                focused: m.focused,
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
            .map(|c| WindowState {
                address: c.address,
                class: c.class,
                title: c.title,
                workspace: c.workspace.name,
                at: c.at,
                size: c.size,
                focus_history: c.focus_history,
                context: tagged_context(&c.tags),
            })
            .collect(),
        plugin: plugins.iter().any(|p| p.name == PLUGIN),
    })
}

/// koje's Hyprland plugin as `hyprctl plugin list` names it (ADR 0051).
const PLUGIN: &str = "koje";

#[derive(Debug, Deserialize)]
struct Plugin {
    name: String,
}

/// Hyprland events after which the shell reads the desktop again. A context's
/// workspace appears with its first window (`createworkspacev2`); windows
/// open, close, move, change their title and take the focus (the dash's
/// most recent window). Sizes change without an event: the overview has
/// the desktop read again when it opens.
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
            | "monitoradded"
            | "monitoraddedv2"
            | "monitorremoved"
            | "monitorremovedv2"
            | "configreloaded"
            | "openwindow"
            | "closewindow"
            | "movewindowv2"
            | "windowtitlev2"
            | "activewindowv2"
            | "changefloatingmode"
    )
}

/// koje's plugin flew the windows into the overview on this monitor
/// (`kojeoverview>>landed,DP-1`): the cards show up.
pub fn overview_landed(line: &str) -> Option<&str> {
    line.strip_prefix("kojeoverview>>landed,")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_events_refresh_others_do_not() {
        for line in [
            "workspacev2>>4,kundenportal",
            "createworkspacev2>>5,privat",
            "focusedmonv2>>DP-1,3",
            "configreloaded>>",
            "windowtitlev2>>55d0e8c2a1b0,new title",
            "openwindow>>55d0e8c2a1b0,kp,foot,~",
        ] {
            assert!(changes_desktop(line), "{line}");
        }
        for line in [
            "activelayout>>kb,English (US)",
            "windowtitle>>55d0e8c2a1b0",
            "screencast>>1,0",
        ] {
            assert!(!changes_desktop(line), "{line}");
        }
    }

    #[test]
    fn the_plugin_tells_when_the_windows_landed() {
        assert_eq!(overview_landed("kojeoverview>>landed,DP-1"), Some("DP-1"));
        assert_eq!(overview_landed("workspacev2>>4,kp"), None);
    }

    #[test]
    fn replies_parse() {
        let m: Vec<Monitor> = serde_json::from_str(
            r#"[{"id":0,"name":"eDP-1","activeWorkspace":{"id":-1337,"name":"kp"},"focused":true}]"#,
        )
        .unwrap();
        assert_eq!(
            (
                m[0].name.as_str(),
                m[0].active_workspace.name.as_str(),
                m[0].focused
            ),
            ("eDP-1", "kp", true)
        );
        let w: Vec<Workspace> =
            serde_json::from_str(r#"[{"id":1,"name":"1","monitor":"eDP-1","windows":2}]"#).unwrap();
        assert_eq!((w[0].id, w[0].monitor.as_str()), (1, "eDP-1"));
        let c: Vec<Client> = serde_json::from_str(
            r#"[{"address":"0x5","mapped":true,"hidden":false,"at":[10,40],"size":[800,600],
                "workspace":{"id":-1337,"name":"kp"},"floating":false,"class":"foot","title":"~",
                "focusHistoryID":2,"pid":7,"tags":["koje-kp"]}]"#,
        )
        .unwrap();
        assert_eq!(
            (
                c[0].at,
                c[0].size,
                c[0].workspace.name.as_str(),
                c[0].focus_history
            ),
            ((10, 40), (800, 600), "kp", 2)
        );
        assert_eq!(tagged_context(&c[0].tags).as_deref(), Some("kp"));
        assert_eq!(
            tagged_context(&["x".into(), "koje-mail*".into()]).as_deref(),
            Some("mail")
        );
        assert_eq!(tagged_context(&[]), None);
    }
}
