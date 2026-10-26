//! Search and the app grid in the overview (ADR 0049): the apps of the
//! running contexts and of the host, and the open windows. Pure; the UI
//! renders [`SearchView`] and [`grid`].

use crate::model::{Accent, WindowState};

/// One program (a context's from `koje apps --json`, the host's from its
/// desktop files).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AppItem {
    pub name: String,
    /// Desktop file id.
    pub id: String,
    /// A theme icon's name or a file.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub terminal: bool,
}

/// The apps of one context, or of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSection {
    /// `None` for the host.
    pub context: Option<String>,
    pub accent: Accent,
    pub apps: Vec<AppItem>,
}

/// The app grid: the active context's apps, then the host's, then the other
/// running contexts' – each section sorted by name.
pub fn grid(
    contexts: &[(String, Vec<AppItem>)],
    host: &[AppItem],
    active: Option<&str>,
) -> Vec<AppSection> {
    let mut sections: Vec<AppSection> = contexts
        .iter()
        .map(|(c, apps)| section(Some(c), apps))
        .collect();
    sections.push(section(None, host));
    sections.retain(|s| !s.apps.is_empty());
    sections.sort_by_key(|s| rank(s.context.as_deref(), active));
    sections
}

fn section(context: Option<&String>, apps: &[AppItem]) -> AppSection {
    let mut apps = apps.to_vec();
    apps.sort_by_key(|a| a.name.to_lowercase());
    AppSection {
        context: context.cloned(),
        accent: context.map_or(Accent::Host, |c| Accent::Context(c.clone())),
        apps,
    }
}

/// The active context first, then the host, then the others by name.
fn rank(context: Option<&str>, active: Option<&str>) -> (u8, String) {
    match context {
        c if c.is_some() && c == active => (0, String::new()),
        None => (1, String::new()),
        Some(c) => (2, c.to_owned()),
    }
}

/// What a search finds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchView {
    /// The best matches first; within one match quality the active context's
    /// first.
    pub apps: Vec<AppHit>,
    pub windows: Vec<WindowHit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppHit {
    pub app: AppItem,
    pub context: Option<String>,
    pub accent: Accent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowHit {
    pub address: String,
    pub title: String,
    pub app: String,
    pub accent: Accent,
    pub stashed: bool,
    pub context: Option<String>,
}

const MAX_APPS: usize = 24;
const MAX_WINDOWS: usize = 8;

/// How well `text` matches `query` (lower case): its start, a word's start,
/// anywhere; `None` for no match.
fn quality(text: &str, query: &str) -> Option<u8> {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        Some(0)
    } else if text
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        Some(1)
    } else if text.contains(query) {
        Some(2)
    } else {
        None
    }
}

/// An app's match: by its name, else (worse) by a keyword or its id.
fn app_quality(app: &AppItem, query: &str) -> Option<u8> {
    quality(&app.name, query).or_else(|| {
        app.keywords
            .iter()
            .map(String::as_str)
            .chain([app.id.trim_end_matches(".desktop")])
            .filter_map(|k| quality(k, query))
            .min()
            .map(|q| q + 3)
    })
}

pub fn search(
    query: &str,
    contexts: &[(String, Vec<AppItem>)],
    host: &[AppItem],
    windows: &[WindowState],
    running: &[String],
    active: Option<&str>,
) -> SearchView {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return SearchView::default();
    }
    let mut apps: Vec<(u8, (u8, String), AppHit)> = Vec::new();
    let all = contexts
        .iter()
        .map(|(c, apps)| (Some(c.as_str()), apps.as_slice()))
        .chain([(None, host)]);
    for (context, list) in all {
        for app in list {
            if let Some(q) = app_quality(app, &query) {
                apps.push((
                    q,
                    rank(context, active),
                    AppHit {
                        app: app.clone(),
                        context: context.map(str::to_owned),
                        accent: context.map_or(Accent::Host, |c| Accent::Context(c.to_owned())),
                    },
                ));
            }
        }
    }
    apps.sort_by(|a, b| {
        (a.0, &a.1, a.2.app.name.to_lowercase()).cmp(&(b.0, &b.1, b.2.app.name.to_lowercase()))
    });
    let mut found: Vec<(u8, i32, WindowHit)> = windows
        .iter()
        .filter_map(|w| {
            let q = quality(&w.title, &query)?;
            let context = w
                .context
                .as_deref()
                .filter(|c| running.iter().any(|r| r == c));
            Some((
                q,
                w.focus_history,
                WindowHit {
                    address: w.address.clone(),
                    title: w.title.clone(),
                    app: w.class.clone(),
                    accent: context.map_or(Accent::Host, |c| Accent::Context(c.to_owned())),
                    stashed: w.workspace.starts_with("special:stash-"),
                    context: context.map(str::to_owned),
                },
            ))
        })
        .collect();
    found.sort_by_key(|f| (f.0, f.1));
    SearchView {
        apps: apps.into_iter().take(MAX_APPS).map(|a| a.2).collect(),
        windows: found.into_iter().take(MAX_WINDOWS).map(|f| f.2).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, id: &str, keywords: &[&str]) -> AppItem {
        AppItem {
            name: name.into(),
            id: id.into(),
            icon: None,
            keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
            terminal: false,
        }
    }

    fn contexts() -> Vec<(String, Vec<AppItem>)> {
        vec![
            (
                "kp".into(),
                vec![
                    app(
                        "Files",
                        "org.gnome.Nautilus.desktop",
                        &["File Manager", "folder"],
                    ),
                    app("Firefox", "firefox.desktop", &["Web Browser"]),
                ],
            ),
            (
                "privat".into(),
                vec![app("Firefox", "firefox.desktop", &[])],
            ),
        ]
    }

    fn host() -> Vec<AppItem> {
        vec![
            app("Settings", "org.gnome.Settings.desktop", &["Preferences"]),
            app("Firefox", "firefox.desktop", &[]),
        ]
    }

    fn names(v: &SearchView) -> Vec<(String, Option<String>)> {
        v.apps
            .iter()
            .map(|h| (h.app.name.clone(), h.context.clone()))
            .collect()
    }

    #[test]
    fn the_best_match_first_then_the_active_context_then_the_host() {
        let v = search("fi", &contexts(), &host(), &[], &[], Some("kp"));
        assert_eq!(
            names(&v),
            [
                ("Files".into(), Some("kp".into())),
                ("Firefox".into(), Some("kp".into())),
                ("Firefox".into(), None),
                ("Firefox".into(), Some("privat".into())),
            ]
        );
        // Keywords and ids match, after names.
        let v = search("browser", &contexts(), &host(), &[], &[], Some("kp"));
        assert_eq!(names(&v), [("Firefox".into(), Some("kp".into()))]);
        let v = search("prefer", &contexts(), &host(), &[], &[], None);
        assert_eq!(names(&v), [("Settings".into(), None)]);
        assert_eq!(
            search("  ", &contexts(), &host(), &[], &[], None),
            SearchView::default()
        );
    }

    #[test]
    fn windows_by_title() {
        let w = |address: &str, title: &str, workspace: &str, focus: i32| WindowState {
            address: address.into(),
            class: "foot".into(),
            title: title.into(),
            workspace: workspace.into(),
            at: (0, 0),
            size: (1, 1),
            focus_history: focus,
            context: Some("kp".into()),
        };
        let windows = [
            w("0x1", "notes.md – Text Editor", "special:stash-kp", 3),
            w("0x2", "Release notes – Firefox", "kp", 1),
            w("0x3", "htop", "kp", 0),
        ];
        let v = search("notes", &[], &[], &windows, &["kp".into()], Some("kp"));
        let found: Vec<(&str, bool)> = v
            .windows
            .iter()
            .map(|w| (w.address.as_str(), w.stashed))
            .collect();
        assert_eq!(found, [("0x1", true), ("0x2", false)]);
        assert_eq!(v.windows[0].accent, Accent::Context("kp".into()));
    }

    #[test]
    fn the_grid_is_the_active_context_the_host_then_the_others() {
        let g = grid(&contexts(), &host(), Some("privat"));
        let order: Vec<Option<&str>> = g.iter().map(|s| s.context.as_deref()).collect();
        assert_eq!(order, [Some("privat"), None, Some("kp")]);
        let kp: Vec<&str> = g[2].apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(kp, ["Files", "Firefox"]);
        assert_eq!(grid(&[], &[], None), []);
    }
}
