//! The screen sharing picker (ADR 0050), without GTK: xdph's window list, what
//! the picker shows – windows grouped by context, the screens – and the reply
//! xdph reads from its stdout.

use crate::model::{Accent, RunningContext, WindowState};

/// One entry of `XDPH_WINDOW_SHARING_LIST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdphWindow {
    /// xdph's own id for the window: what the reply names.
    pub handle: u32,
    pub class: String,
    pub title: String,
    /// Hyprland's window address; 0 when xdph does not know it.
    pub address: u64,
}

/// xdph's list: `<handle>[HC>]<class>[HT>]<title>[HE>]<address>[HA>]`, one
/// entry after another (xdph 1.3.11). Malformed entries are skipped.
pub fn parse_list(list: &str) -> Vec<XdphWindow> {
    list.split("[HA>]")
        .filter_map(|entry| {
            let (handle, rest) = entry.split_once("[HC>]")?;
            let (class, rest) = rest.split_once("[HT>]")?;
            let (title, address) = rest.split_once("[HE>]")?;
            Some(XdphWindow {
                handle: handle.trim().parse().ok()?,
                class: class.to_owned(),
                title: title.to_owned(),
                address: address.trim().parse().unwrap_or(0),
            })
        })
        .collect()
}

/// A screen as Hyprland lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The connector: what the reply names (`DP-3`).
    pub name: String,
    /// Make and model.
    pub description: String,
    pub focused: bool,
}

/// What the picker shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareView {
    pub groups: Vec<Group>,
    /// The focused screen first.
    pub screens: Vec<Screen>,
    /// *Remember this selection* starts on (xdph's `--allow-token`).
    pub remember: bool,
}

/// The windows of one context, or the host's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The context's name; `None` for the host.
    pub context: Option<String>,
    pub accent: Accent,
    /// Most recently used first.
    pub windows: Vec<WindowTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTile {
    pub handle: u32,
    /// Hyprland's address (`0x…`) for its thumbnail; empty when unknown.
    pub address: String,
    /// The app id: icon and name come from its desktop file.
    pub app: String,
    pub title: String,
    /// In its context's stash: running, but not in the active layout.
    pub stashed: bool,
}

/// The picker's view: xdph's windows grouped by the context Hyprland's
/// windows carry (kojed's tag). The focused window's context comes first –
/// the best guess at who asks, as xdph does not tell –, the other contexts
/// by name, the host's windows last.
pub fn view(
    list: &[XdphWindow],
    windows: &[WindowState],
    contexts: &[RunningContext],
    screens: &[Screen],
    allow_token: bool,
) -> ShareView {
    let known = |w: &XdphWindow| {
        windows
            .iter()
            .find(|s| w.address != 0 && parse_address(&s.address) == Some(w.address))
    };
    let mut tiles: Vec<(Option<String>, i32, WindowTile)> = list
        .iter()
        .map(|w| {
            let state = known(w);
            let tile = WindowTile {
                handle: w.handle,
                address: state.map(|s| s.address.clone()).unwrap_or_default(),
                app: w.class.clone(),
                title: w.title.clone(),
                stashed: state.is_some_and(|s| s.workspace.starts_with("special:stash-")),
            };
            (
                state.and_then(|s| s.context.clone()),
                state.map_or(i32::MAX, |s| s.focus_history),
                tile,
            )
        })
        .collect();
    tiles.sort_by_key(|(_, recent, _)| *recent);
    let focused = windows
        .iter()
        .find(|w| w.focus_history == 0)
        .and_then(|w| w.context.clone());
    let mut names: Vec<Option<String>> = Vec::new();
    for (context, _, _) in &tiles {
        if !names.contains(context) {
            names.push(context.clone());
        }
    }
    names.sort_by_key(|c| match c {
        Some(_) if *c == focused => (0, String::new()),
        Some(name) => (1, name.clone()),
        None => (2, String::new()),
    });
    let groups = names
        .into_iter()
        .map(|context| {
            let accent = match &context {
                Some(name) if contexts.iter().any(|c| &c.name == name) => {
                    Accent::Context(name.clone())
                }
                _ => Accent::Host,
            };
            let windows = tiles
                .iter()
                .filter(|(c, _, _)| *c == context)
                .map(|(_, _, t)| t.clone())
                .collect();
            Group {
                context,
                accent,
                windows,
            }
        })
        .collect();
    let mut screens = screens.to_vec();
    screens.sort_by_key(|s| !s.focused);
    ShareView {
        groups,
        screens,
        remember: allow_token,
    }
}

fn parse_address(address: &str) -> Option<u64> {
    u64::from_str_radix(address.trim_start_matches("0x"), 16).ok()
}

/// What the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Window(u32),
    Screen(String),
}

/// The line xdph reads: `r` grants a restore token. xdph drops the last
/// character of a screen's name, so the newline is required.
pub fn reply(choice: &Choice, remember: bool) -> String {
    let flags = if remember { "r" } else { "" };
    match choice {
        Choice::Window(handle) => format!("[SELECTION]{flags}/window:{handle}\n"),
        Choice::Screen(name) => format!("[SELECTION]{flags}/screen:{name}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From the host (M8.65).
    const LIST: &str = "852087840[HC>]firefox_firefox[HT>]gUM Test Page — Mozilla Firefox[HE>]95409315344256[HA>]852332432[HC>]com.microsoft.VSCode[HT>]Dev Container: koje - workspace[HE>]95409315570416[HA>]852333056[HC>]org.gnome.Ptyxis[HT>]bikeshedder@brain: ~/projects/koje[HE>]95409315487120[HA>]";

    fn win(address: &str, workspace: &str, context: Option<&str>, recent: i32) -> WindowState {
        WindowState {
            address: address.to_owned(),
            class: String::new(),
            title: String::new(),
            workspace: workspace.to_owned(),
            at: (0, 0),
            size: (800, 600),
            focus_history: recent,
            context: context.map(str::to_owned),
        }
    }

    fn ctx(name: &str) -> RunningContext {
        RunningContext {
            name: name.to_owned(),
            color: "#3584e4".to_owned(),
            active: false,
        }
    }

    #[test]
    fn xdphs_list_parses() {
        let list = parse_list(LIST);
        assert_eq!(list.len(), 3);
        assert_eq!(
            list[0],
            XdphWindow {
                handle: 852_087_840,
                class: "firefox_firefox".to_owned(),
                title: "gUM Test Page — Mozilla Firefox".to_owned(),
                address: 0x56c6_3659_6f80,
            }
        );
        assert_eq!(list[2].title, "bikeshedder@brain: ~/projects/koje");
        assert_eq!(parse_list(""), []);
        assert_eq!(parse_list("x[HC>]a[HT>]b[HE>]0[HA>]7[HC>]a").len(), 0);
        // No mapping protocol: address 0.
        assert_eq!(parse_list("7[HC>]a[HT>]b[HE>]0[HA>]")[0].address, 0);
    }

    #[test]
    fn windows_are_grouped_by_context_the_focused_one_first() {
        let list = parse_list(LIST);
        let windows = [
            // Firefox in "web", VS Code in "kp" (focused), the terminal on the host.
            win("0x56c636596f80", "web", Some("web"), 1),
            win("0x56c6365ce2f0", "kp", Some("kp"), 0),
            win("0x56c6365b9d90", "1", None, 2),
        ];
        let v = view(&list, &windows, &[ctx("kp"), ctx("web")], &[], false);
        let order: Vec<Option<&str>> = v.groups.iter().map(|g| g.context.as_deref()).collect();
        assert_eq!(order, [Some("kp"), Some("web"), None]);
        assert_eq!(v.groups[0].accent, Accent::Context("kp".to_owned()));
        assert_eq!(v.groups[2].accent, Accent::Host);
        assert_eq!(v.groups[1].windows[0].handle, 852_087_840);
        assert_eq!(v.groups[1].windows[0].address, "0x56c636596f80");
    }

    #[test]
    fn stashed_and_unknown_windows() {
        let list = parse_list(LIST);
        let windows = [
            win("0x56c636596f80", "special:stash-web", Some("web"), 3),
            win("0x56c6365ce2f0", "web", Some("web"), 0),
        ];
        let v = view(&list, &windows, &[], &[], true);
        assert!(v.remember);
        // A context kojed did not name still groups, without its color.
        assert_eq!(v.groups[0].context.as_deref(), Some("web"));
        assert_eq!(v.groups[0].accent, Accent::Host);
        // Most recently used first; the stashed one marked.
        let web: Vec<(u32, bool)> = v.groups[0]
            .windows
            .iter()
            .map(|t| (t.handle, t.stashed))
            .collect();
        assert_eq!(web, [(852_332_432, false), (852_087_840, true)]);
        // Hyprland did not list the terminal: the host's, no thumbnail.
        assert_eq!(v.groups[1].context, None);
        assert_eq!(v.groups[1].windows[0].address, "");
    }

    #[test]
    fn the_focused_screen_comes_first() {
        let screen = |name: &str, focused| Screen {
            name: name.to_owned(),
            description: String::new(),
            focused,
        };
        let v = view(
            &[],
            &[],
            &[],
            &[screen("DP-1", false), screen("DP-3", true)],
            false,
        );
        assert_eq!(v.screens[0].name, "DP-3");
        assert_eq!(v.groups, []);
    }

    #[test]
    fn replies_as_xdph_reads_them() {
        assert_eq!(
            reply(&Choice::Window(852_087_840), false),
            "[SELECTION]/window:852087840\n"
        );
        assert_eq!(
            reply(&Choice::Screen("DP-3".to_owned()), true),
            "[SELECTION]r/screen:DP-3\n"
        );
    }
}
