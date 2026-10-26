//! The dash at the bottom of the overview, as GNOME's (ADR 0049): the
//! favorites, then the apps that run and are not among them. An app in
//! several contexts is one entry per (app, context); favorites stand for
//! the active context – or the host on a host workspace – and launch there.

use crate::model::{Accent, WindowState};

/// One icon in the dash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashEntry {
    /// Its desktop file id (`firefox.desktop`) when known, else its app id:
    /// what its icon and name are looked up by.
    pub app: String,
    /// The desktop file id to launch it by, when known.
    pub id: Option<String>,
    /// The context it runs or launches in; `None` for the host.
    pub context: Option<String>,
    pub accent: Accent,
    pub favorite: bool,
    /// Its windows, the ones in view before stashed ones, most recently used
    /// first. Empty: not running.
    pub windows: Vec<DashWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashWindow {
    pub address: String,
    pub stashed: bool,
}

/// What [`dash`] works from.
pub struct DashInput<'a> {
    /// `org.gnome.shell favorite-apps`: desktop file ids.
    pub favorites: &'a [String],
    pub windows: &'a [WindowState],
    /// The running contexts.
    pub contexts: &'a [String],
    /// The context whose workspace is shown; `None` on a host workspace.
    pub active: Option<&'a str>,
    /// The desktop file id of an app id, if the host has one.
    pub desktop_id: &'a dyn Fn(&str) -> Option<String>,
}

/// A window's context, when it is one that runs.
fn context_of<'a>(w: &'a WindowState, contexts: &[String]) -> Option<&'a str> {
    w.context
        .as_deref()
        .filter(|c| contexts.iter().any(|r| r == c))
}

fn accent(context: Option<&str>) -> Accent {
    context.map_or(Accent::Host, |c| Accent::Context(c.to_owned()))
}

/// Whether a window of app id `class` is the app with desktop file `id`.
fn same_app(id: &str, class: &str, resolved: Option<&str>) -> bool {
    resolved.is_some_and(|r| r == id)
        || id
            .strip_suffix(".desktop")
            .is_some_and(|stem| stem.eq_ignore_ascii_case(class))
}

pub fn dash(input: &DashInput<'_>) -> Vec<DashEntry> {
    let active = input.active;
    // Each window with its context and its app's desktop file id.
    let windows: Vec<(&WindowState, Option<&str>, Option<String>)> = input
        .windows
        .iter()
        .map(|w| {
            (
                w,
                context_of(w, input.contexts),
                (input.desktop_id)(&w.class),
            )
        })
        .collect();
    let mut taken = vec![false; windows.len()];
    let mut out = Vec::new();

    for id in input.favorites {
        let mut mine = Vec::new();
        for (i, (w, ctx, resolved)) in windows.iter().enumerate() {
            if !taken[i] && *ctx == active && same_app(id, &w.class, resolved.as_deref()) {
                taken[i] = true;
                mine.push(*w);
            }
        }
        out.push(DashEntry {
            app: id.clone(),
            id: Some(id.clone()),
            context: active.map(str::to_owned),
            accent: accent(active),
            favorite: true,
            windows: ordered(&mine),
        });
    }

    // The others, grouped by (app, context): the active context first, then
    // by context and app – a stable order, not one that follows the focus.
    let mut groups: Vec<Group<'_>> = Vec::new();
    for (i, (w, ctx, resolved)) in windows.iter().enumerate() {
        if taken[i] {
            continue;
        }
        let app = resolved.clone().unwrap_or_else(|| w.class.clone());
        match groups
            .iter_mut()
            .find(|g| g.context == *ctx && g.app == app)
        {
            Some(g) => g.windows.push(w),
            None => groups.push(Group {
                context: *ctx,
                app,
                id: resolved.clone(),
                windows: vec![w],
            }),
        }
    }
    groups.sort_by_key(|g| {
        (
            g.context != active,
            g.context.map(str::to_owned),
            g.app.clone(),
        )
    });
    out.extend(groups.into_iter().map(|g| DashEntry {
        // A context's desktop files are inside it: `<app id>.desktop` is
        // the best guess `koje apps --start` can look for there.
        id:
            g.id.or_else(|| g.context.map(|_| format!("{}.desktop", g.windows[0].class))),
        context: g.context.map(str::to_owned),
        accent: accent(g.context),
        favorite: false,
        windows: ordered(&g.windows),
        app: g.app,
    }));
    out
}

/// Running windows of one app in one context.
struct Group<'a> {
    context: Option<&'a str>,
    app: String,
    id: Option<String>,
    windows: Vec<&'a WindowState>,
}

/// Windows in view first, each part most recently used first.
fn ordered(windows: &[&WindowState]) -> Vec<DashWindow> {
    let mut ws: Vec<&WindowState> = windows.to_vec();
    let stashed = |w: &WindowState| w.workspace.starts_with("special:stash-");
    ws.sort_by_key(|w| (stashed(w), w.focus_history));
    ws.into_iter()
        .map(|w| DashWindow {
            address: w.address.clone(),
            stashed: stashed(w),
        })
        .collect()
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
            address: address.into(),
            class: class.into(),
            title: String::new(),
            workspace: workspace.into(),
            at: (0, 0),
            size: (100, 100),
            focus_history: focus,
            context: context.map(str::to_owned),
        }
    }

    fn resolve(class: &str) -> Option<String> {
        (class == "org.gnome.Nautilus").then(|| "org.gnome.Nautilus.desktop".to_owned())
    }

    fn summary(entries: &[DashEntry]) -> Vec<(String, Option<String>, bool, Vec<String>)> {
        entries
            .iter()
            .map(|e| {
                (
                    e.app.clone(),
                    e.context.clone(),
                    e.favorite,
                    e.windows.iter().map(|w| w.address.clone()).collect(),
                )
            })
            .collect()
    }

    fn s(v: &str) -> String {
        v.to_owned()
    }

    #[test]
    fn favorites_stand_for_the_active_context_other_contexts_get_their_own_entries() {
        let favorites = [
            s("firefox.desktop"),
            s("org.gnome.Nautilus.desktop"),
            s("org.gnome.Calculator.desktop"),
        ];
        let windows = [
            win("0x1", "firefox", "kp", Some("kp"), 3),
            win("0x2", "firefox", "privat", Some("privat"), 1),
            win(
                "0x3",
                "org.gnome.Nautilus",
                "special:stash-kp",
                Some("kp"),
                0,
            ),
            win("0x4", "org.gnome.Nautilus", "kp", Some("kp"), 5),
            win("0x5", "foot", "kp", Some("kp"), 2),
            win("0x6", "foot", "1", None, 4),
            // A context that does not run (any more): a host window.
            win("0x7", "foot", "2", Some("gone"), 6),
        ];
        let contexts = [s("kp"), s("privat")];
        let entries = dash(&DashInput {
            favorites: &favorites,
            windows: &windows,
            contexts: &contexts,
            active: Some("kp"),
            desktop_id: &resolve,
        });
        assert_eq!(
            summary(&entries),
            [
                (s("firefox.desktop"), Some(s("kp")), true, vec![s("0x1")]),
                // In view before stashed.
                (
                    s("org.gnome.Nautilus.desktop"),
                    Some(s("kp")),
                    true,
                    vec![s("0x4"), s("0x3")]
                ),
                (
                    s("org.gnome.Calculator.desktop"),
                    Some(s("kp")),
                    true,
                    vec![]
                ),
                (s("foot"), Some(s("kp")), false, vec![s("0x5")]),
                (s("foot"), None, false, vec![s("0x6"), s("0x7")]),
                (s("firefox"), Some(s("privat")), false, vec![s("0x2")]),
            ]
        );
        assert!(entries[1].windows[1].stashed);
        assert_eq!(entries[1].id.as_deref(), Some("org.gnome.Nautilus.desktop"));
        assert_eq!(entries[3].id.as_deref(), Some("foot.desktop"));
        // A host app without a desktop file cannot be started again.
        assert_eq!(entries[4].id, None);
        assert_eq!(entries[5].accent, Accent::Context(s("privat")));
    }

    #[test]
    fn on_a_host_workspace_favorites_are_the_hosts() {
        let favorites = [s("firefox.desktop")];
        let windows = [
            win("0x1", "firefox", "kp", Some("kp"), 0),
            win("0x2", "firefox", "1", None, 1),
        ];
        let contexts = [s("kp")];
        let entries = dash(&DashInput {
            favorites: &favorites,
            windows: &windows,
            contexts: &contexts,
            active: None,
            desktop_id: &|_| None,
        });
        assert_eq!(
            summary(&entries),
            [
                (s("firefox.desktop"), None, true, vec![s("0x2")]),
                (s("firefox"), Some(s("kp")), false, vec![s("0x1")]),
            ]
        );
        assert_eq!(entries[0].accent, Accent::Host);
    }
}
