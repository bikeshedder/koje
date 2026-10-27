//! The dash at the bottom of the overview, as GNOME's (ADR 0049, 0053): the
//! favorites, then the apps that run and are not among them – of the active
//! context only, or of the host on a host workspace. A context is like a
//! machine of its own: its favorites are its programs (`favorites` in its
//! `context.toml`, else GNOME's as far as it has them) and launch there.

use crate::model::{Accent, WindowState};
use crate::search::AppItem;

/// One icon in the dash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashEntry {
    /// Its desktop file id (`firefox.desktop`) when known, else its app id:
    /// what a host app's icon and name are looked up by.
    pub app: String,
    /// The desktop file id to launch it by, when known.
    pub id: Option<String>,
    /// The context's program, with its name and icon; `None` on the host
    /// or for a window no program of the context matches.
    pub item: Option<AppItem>,
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
    /// `org.gnome.shell favorite-apps`: the host's desktop file ids.
    pub favorites: &'a [String],
    pub windows: &'a [WindowState],
    /// The running contexts.
    pub contexts: &'a [String],
    /// The context whose workspace is shown; `None` on a host workspace.
    pub active: Option<&'a str>,
    /// The active context's programs (`koje apps --json`).
    pub apps: &'a [AppItem],
    /// The active context's own favorites, when it has set them.
    pub context_favorites: Option<&'a [String]>,
    /// The desktop file id of an app id, if the host has one.
    pub desktop_id: &'a dyn Fn(&str) -> Option<String>,
    /// The name of a host desktop file: GNOME's favorites are found in a
    /// context by it too.
    pub host_name: &'a dyn Fn(&str) -> Option<String>,
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

/// `org.mozilla.firefox.desktop` → `org.mozilla.firefox`.
fn stem(id: &str) -> &str {
    id.strip_suffix(".desktop").unwrap_or(id)
}

/// `org.mozilla.firefox` → `firefox`.
fn last(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Whether a window of app id `class` is the host app with desktop file `id`.
fn same_app(id: &str, class: &str, resolved: Option<&str>) -> bool {
    resolved.is_some_and(|r| r == id) || stem(id).eq_ignore_ascii_case(class)
}

/// Whether a window of app id `class` belongs to a context's program: its
/// `StartupWMClass`, its id (also the last part: `firefox` for
/// `org.mozilla.firefox`) or its program's name.
fn window_of(app: &AppItem, class: &str) -> bool {
    let id = stem(&app.id);
    app.wm_class
        .as_deref()
        .is_some_and(|w| w.eq_ignore_ascii_case(class))
        || id.eq_ignore_ascii_case(class)
        || last(id).eq_ignore_ascii_case(class)
        || app
            .binary
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case(class))
}

/// A host favorite among a context's programs: the same id, the same last
/// part (`firefox.desktop` – `org.mozilla.firefox.desktop`), the same name
/// or a program of that name.
fn favorite_in<'a>(id: &str, name: Option<&str>, apps: &'a [AppItem]) -> Option<&'a AppItem> {
    let short = last(stem(id));
    apps.iter()
        .find(|a| a.id == id)
        .or_else(|| {
            apps.iter()
                .find(|a| last(stem(&a.id)).eq_ignore_ascii_case(short))
        })
        .or_else(|| name.and_then(|n| apps.iter().find(|a| a.name.eq_ignore_ascii_case(n))))
        .or_else(|| {
            apps.iter().find(|a| {
                a.binary
                    .as_deref()
                    .is_some_and(|b| b.eq_ignore_ascii_case(short))
            })
        })
}

pub fn dash(input: &DashInput<'_>) -> Vec<DashEntry> {
    let active = input.active;
    // The windows of the active context (the host's on a host workspace),
    // each with its program: the context's, else the host's desktop file.
    let windows: Vec<(&WindowState, Option<String>)> = input
        .windows
        .iter()
        .filter(|w| context_of(w, input.contexts) == active)
        .map(|w| {
            let resolved = match active {
                Some(_) => input
                    .apps
                    .iter()
                    .find(|a| window_of(a, &w.class))
                    .map(|a| a.id.clone()),
                None => (input.desktop_id)(&w.class),
            };
            (w, resolved)
        })
        .collect();
    let item = |id: &str| input.apps.iter().find(|a| a.id == id).cloned();
    let mut taken = vec![false; windows.len()];
    let mut out = Vec::new();

    for id in favorites(input) {
        let mut mine = Vec::new();
        for (i, (w, resolved)) in windows.iter().enumerate() {
            let is = match active {
                Some(_) => resolved.as_deref() == Some(id.as_str()),
                None => same_app(&id, &w.class, resolved.as_deref()),
            };
            if !taken[i] && is {
                taken[i] = true;
                mine.push(*w);
            }
        }
        out.push(DashEntry {
            item: active.and_then(|_| item(&id)),
            app: id.clone(),
            id: Some(id),
            context: active.map(str::to_owned),
            accent: accent(active),
            favorite: true,
            windows: ordered(&mine),
        });
    }

    // The others, one entry per app, by app – a stable order, not one that
    // follows the focus.
    let mut groups: Vec<(String, Option<String>, Vec<&WindowState>)> = Vec::new();
    for (i, (w, resolved)) in windows.iter().enumerate() {
        if taken[i] {
            continue;
        }
        let app = resolved.clone().unwrap_or_else(|| w.class.clone());
        match groups.iter_mut().find(|g| g.0 == app) {
            Some(g) => g.2.push(w),
            None => groups.push((app, resolved.clone(), vec![w])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(groups.into_iter().map(|(app, id, ws)| DashEntry {
        item: id.as_deref().and_then(item),
        // A context's window no program matches: `<app id>.desktop` is
        // the best guess `koje apps --start` can look for there.
        id: id.or_else(|| active.map(|_| format!("{}.desktop", ws[0].class))),
        context: active.map(str::to_owned),
        accent: accent(active),
        favorite: false,
        windows: ordered(&ws),
        app,
    }));
    out
}

/// The favorites' desktop file ids: GNOME's on the host; in a context its
/// own, else GNOME's it has (by its ids, once each).
fn favorites(input: &DashInput<'_>) -> Vec<String> {
    if input.active.is_none() {
        return input.favorites.to_vec();
    }
    if let Some(own) = input.context_favorites {
        return own.to_vec();
    }
    let mut ids: Vec<String> = Vec::new();
    for id in input.favorites {
        let name = (input.host_name)(id);
        if let Some(app) = favorite_in(id, name.as_deref(), input.apps)
            && !ids.contains(&app.id)
        {
            ids.push(app.id.clone());
        }
    }
    ids
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

/// The favorites after pinning or unpinning `id` – what goes into the
/// context's `favorites` (or GNOME's on the host).
pub fn pinned(entries: &[DashEntry], id: &str, pin: bool) -> Vec<String> {
    let mut ids: Vec<String> = entries
        .iter()
        .filter(|e| e.favorite)
        .filter_map(|e| e.id.clone())
        .filter(|i| i != id)
        .collect();
    if pin {
        ids.push(id.to_owned());
    }
    ids
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
            title: String::new(),
            workspace: workspace.into(),
            at: (0, 0),
            size: (100, 100),
            focus_history: focus,
            context: context.map(str::to_owned),
        }
    }

    fn app(name: &str, id: &str, binary: &str) -> AppItem {
        AppItem {
            name: name.into(),
            id: id.into(),
            icon: None,
            keywords: vec![],
            terminal: false,
            wm_class: None,
            binary: Some(binary.into()),
        }
    }

    fn resolve(class: &str) -> Option<String> {
        (class == "org.gnome.Nautilus").then(|| "org.gnome.Nautilus.desktop".to_owned())
    }

    fn host_name(id: &str) -> Option<String> {
        match id {
            "org.gnome.Ptyxis.desktop" => Some("Terminal".into()),
            "org.gnome.Nautilus.desktop" => Some("Files".into()),
            _ => None,
        }
    }

    fn summary(entries: &[DashEntry]) -> Vec<(String, bool, Vec<String>)> {
        entries
            .iter()
            .map(|e| {
                (
                    e.app.clone(),
                    e.favorite,
                    e.windows.iter().map(|w| w.address.clone()).collect(),
                )
            })
            .collect()
    }

    fn s(v: &str) -> String {
        v.to_owned()
    }

    fn gnome() -> Vec<String> {
        vec![
            s("firefox.desktop"),
            s("org.gnome.Nautilus.desktop"),
            s("org.gnome.Ptyxis.desktop"),
            s("org.gnome.Calculator.desktop"),
        ]
    }

    /// A Fedora context: its Firefox under another id, its own terminal.
    fn fedora() -> Vec<AppItem> {
        vec![
            app("Firefox", "org.mozilla.firefox.desktop", "firefox"),
            app("Files", "org.gnome.Nautilus.desktop", "nautilus"),
            app("Terminal", "foot.desktop", "foot"),
            app(
                "Text Editor",
                "org.gnome.TextEditor.desktop",
                "gnome-text-editor",
            ),
        ]
    }

    fn windows() -> Vec<WindowState> {
        vec![
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
            win("0x5", "org.gnome.TextEditor", "kp", Some("kp"), 2),
            win("0x6", "foot", "1", None, 4),
            // A context that does not run (any more): a host window.
            win("0x7", "foot", "2", Some("gone"), 6),
            win("0x8", "xterm", "kp", Some("kp"), 7),
        ]
    }

    fn in_kp<'a>(
        favorites: &'a [String],
        windows: &'a [WindowState],
        apps: &'a [AppItem],
        own: Option<&'a [String]>,
    ) -> DashInput<'a> {
        static CONTEXTS: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| vec![s("kp"), s("privat")]);
        DashInput {
            favorites,
            windows,
            contexts: &CONTEXTS,
            active: Some("kp"),
            apps,
            context_favorites: own,
            desktop_id: &resolve,
            host_name: &host_name,
        }
    }

    #[test]
    fn in_a_context_only_its_apps_gnomes_favorites_as_far_as_it_has_them() {
        let (favorites, windows, apps) = (gnome(), windows(), fedora());
        let entries = dash(&in_kp(&favorites, &windows, &apps, None));
        assert_eq!(
            summary(&entries),
            [
                // Firefox by the last part of its id, its window by its id.
                (s("org.mozilla.firefox.desktop"), true, vec![s("0x1")]),
                // In view before stashed.
                (
                    s("org.gnome.Nautilus.desktop"),
                    true,
                    vec![s("0x4"), s("0x3")]
                ),
                // Ptyxis by its name; no Calculator in the context.
                (s("foot.desktop"), true, vec![]),
                (s("org.gnome.TextEditor.desktop"), false, vec![s("0x5")]),
                // No program of the context: its app id, a guessed file.
                (s("xterm"), false, vec![s("0x8")]),
            ]
        );
        assert!(entries[1].windows[1].stashed);
        assert_eq!(
            entries[0].item.as_ref().map(|i| i.name.as_str()),
            Some("Firefox")
        );
        assert!(entries.iter().all(|e| e.context.as_deref() == Some("kp")));
        assert!(entries.iter().all(|e| e.accent == Accent::Context(s("kp"))));
        assert_eq!(entries[4].id.as_deref(), Some("xterm.desktop"));
        assert_eq!(entries[4].item, None);
    }

    #[test]
    fn a_context_with_its_own_favorites() {
        let (favorites, windows, apps) = (gnome(), windows(), fedora());
        let own = [s("org.gnome.TextEditor.desktop"), s("foot.desktop")];
        let entries = dash(&in_kp(&favorites, &windows, &apps, Some(&own)));
        assert_eq!(
            summary(&entries)
                .iter()
                .map(|e| (e.0.as_str(), e.1))
                .collect::<Vec<_>>(),
            [
                ("org.gnome.TextEditor.desktop", true),
                ("foot.desktop", true),
                ("org.gnome.Nautilus.desktop", false),
                ("org.mozilla.firefox.desktop", false),
                ("xterm", false),
            ]
        );
        assert_eq!(
            pinned(&entries, "org.mozilla.firefox.desktop", true),
            [
                "org.gnome.TextEditor.desktop",
                "foot.desktop",
                "org.mozilla.firefox.desktop"
            ]
        );
        assert_eq!(
            pinned(&entries, "foot.desktop", false),
            ["org.gnome.TextEditor.desktop"]
        );
    }

    #[test]
    fn on_a_host_workspace_only_the_hosts_apps() {
        let (favorites, windows) = (gnome(), windows());
        let contexts = [s("kp"), s("privat")];
        let entries = dash(&DashInput {
            favorites: &favorites,
            windows: &windows,
            contexts: &contexts,
            active: None,
            apps: &[],
            context_favorites: None,
            desktop_id: &resolve,
            host_name: &host_name,
        });
        assert_eq!(
            summary(&entries),
            [
                (s("firefox.desktop"), true, vec![]),
                (s("org.gnome.Nautilus.desktop"), true, vec![]),
                (s("org.gnome.Ptyxis.desktop"), true, vec![]),
                (s("org.gnome.Calculator.desktop"), true, vec![]),
                (s("foot"), false, vec![s("0x6"), s("0x7")]),
            ]
        );
        assert!(
            entries
                .iter()
                .all(|e| e.accent == Accent::Host && e.context.is_none())
        );
        // A host app without a desktop file cannot be started again.
        assert_eq!(entries[4].id, None);
    }
}
