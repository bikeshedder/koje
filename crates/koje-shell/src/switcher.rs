//! Alt+Tab as GNOME's (M8.49, ADR 0049): the apps with windows on the
//! current workspace (M8.100), the most recently used first, one entry per
//! (app, context); Alt+grave (the key above Tab) goes through one app's
//! windows. Pure: the UI keeps the selection and renders.

use crate::model::{Accent, WindowState};

/// One app in the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchApp {
    pub app: String,
    pub context: Option<String>,
    pub accent: Accent,
    /// Most recently used first.
    pub windows: Vec<SwitchWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchWindow {
    pub address: String,
    pub title: String,
    pub stashed: bool,
}

/// The apps of the windows on `workspace` (a name as in
/// [`WindowState::workspace`]; every workspace when `None`) – its context's
/// stashed ones too, as GNOME lists minimized ones –, ordered by their most
/// recent window.
pub fn apps(
    windows: &[WindowState],
    running: &[String],
    workspace: Option<&str>,
) -> Vec<SwitchApp> {
    let stash = workspace.map(|w| format!("special:stash-{w}"));
    let mut ws: Vec<&WindowState> = windows
        .iter()
        .filter(|w| {
            workspace.is_none_or(|current| {
                w.workspace == current || stash.as_deref() == Some(w.workspace.as_str())
            })
        })
        .collect();
    ws.sort_by_key(|w| w.focus_history);
    let mut out: Vec<SwitchApp> = Vec::new();
    for w in ws {
        let context = w
            .context
            .as_deref()
            .filter(|c| running.iter().any(|r| r == c))
            .map(str::to_owned);
        let window = SwitchWindow {
            address: w.address.clone(),
            title: w.title.clone(),
            stashed: w.workspace.starts_with("special:stash-"),
        };
        match out
            .iter_mut()
            .find(|a| a.app == w.class && a.context == context)
        {
            Some(a) => a.windows.push(window),
            None => out.push(SwitchApp {
                app: w.class.clone(),
                accent: context
                    .as_ref()
                    .map_or(Accent::Host, |c| Accent::Context(c.clone())),
                context,
                windows: vec![window],
            }),
        }
    }
    out
}

/// The entry after (or before) `current` among `len`, wrapping around.
pub fn step(current: usize, len: usize, backwards: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if backwards {
        (current + len - 1) % len
    } else {
        (current + 1) % len
    }
}

/// Where a switcher opens: on the previous entry – the one before the
/// current app or window –, or the first when there is no other.
pub fn first(len: usize, backwards: bool) -> usize {
    match len {
        0 | 1 => 0,
        _ if backwards => len - 1,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(
        address: &str,
        class: &str,
        workspace: &str,
        context: Option<&str>,
        focus: i32,
    ) -> WindowState {
        WindowState {
            pinnable: true,
            address: address.into(),
            class: class.into(),
            title: format!("t{address}"),
            workspace: workspace.into(),
            at: (0, 0),
            size: (1, 1),
            focus_history: focus,
            context: context.map(str::to_owned),
        }
    }

    #[test]
    fn apps_by_their_most_recent_window_one_per_context() {
        let windows = [
            win("0x1", "firefox", "kp", Some("kp"), 2),
            win("0x2", "foot", "kp", Some("kp"), 0),
            win("0x3", "firefox", "privat", Some("privat"), 1),
            win("0x4", "firefox", "special:stash-kp", Some("kp"), 3),
            win("0x5", "foot", "1", None, 4),
        ];
        let running = ["kp".to_owned(), "privat".to_owned()];
        let a = apps(&windows, &running, None);
        let summary: Vec<(&str, Option<&str>, Vec<&str>)> = a
            .iter()
            .map(|a| {
                (
                    a.app.as_str(),
                    a.context.as_deref(),
                    a.windows.iter().map(|w| w.address.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("foot", Some("kp"), vec!["0x2"]),
                ("firefox", Some("privat"), vec!["0x3"]),
                ("firefox", Some("kp"), vec!["0x1", "0x4"]),
                ("foot", None, vec!["0x5"]),
            ]
        );
        assert!(a[2].windows[1].stashed);
        assert_eq!(a[3].accent, Accent::Host);
    }

    #[test]
    fn only_the_current_workspace_and_its_stash() {
        let windows = [
            win("0x1", "firefox", "kp", Some("kp"), 2),
            win("0x2", "foot", "privat", Some("privat"), 0),
            win("0x3", "mail", "special:mail", None, 1),
            win("0x4", "firefox", "special:stash-kp", Some("kp"), 3),
            win("0x5", "foot", "1", None, 4),
            win("0x6", "foot", "special:stash-privat", Some("privat"), 5),
        ];
        let running = ["kp".to_owned(), "privat".to_owned()];
        let addresses = |workspace| -> Vec<String> {
            apps(&windows, &running, Some(workspace))
                .iter()
                .flat_map(|a| a.windows.iter().map(|w| w.address.clone()))
                .collect()
        };
        assert_eq!(addresses("kp"), ["0x1", "0x4"]);
        assert_eq!(addresses("1"), ["0x5"]);
        assert_eq!(addresses("special:mail"), ["0x3"]);
    }

    #[test]
    fn stepping_wraps_and_starts_on_the_previous_entry() {
        assert_eq!(step(0, 3, false), 1);
        assert_eq!(step(2, 3, false), 0);
        assert_eq!(step(0, 3, true), 2);
        assert_eq!(step(0, 0, false), 0);
        assert_eq!(first(3, false), 1);
        assert_eq!(first(3, true), 2);
        assert_eq!(first(1, false), 0);
        assert_eq!(first(0, true), 0);
    }
}
