//! Capturing a running context as a state (M5.1, ADR 0030, 0031): window ids,
//! what each window needs to start again, and the live state kojed keeps.
//! Pure: kojed observes processes and windows and feeds them in.

use std::collections::{BTreeMap, BTreeSet};

use crate::integration::{self, Detect, Integration};
use crate::layout::Placement;
use crate::name::{LayoutName, WindowId};
use crate::state::{State, Where, WindowSpec};

/// A process as kojed sees it in `/proc` (paths as inside the context).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proc {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    /// Direct children (a terminal's shells).
    pub children: Vec<Proc>,
}

/// What kojed knows about a window when it takes the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub app_id: String,
    pub title: String,
    pub process: Option<Proc>,
}

const SHELLS: [&str; 10] = [
    "bash", "zsh", "fish", "sh", "dash", "ksh", "mksh", "tcsh", "nu", "xonsh",
];

fn basename(arg: &str) -> &str {
    let name = arg.rsplit('/').next().unwrap_or(arg);
    // Login shells show as `-bash`.
    name.trim_start_matches('-')
}

/// The id base of a window: the integration's name, else the program's name
/// from the app id (`org.mozilla.firefox` → `firefox`).
pub fn base_id(integration: Option<&str>, app_id: &str) -> String {
    let raw = integration.unwrap_or_else(|| app_id.rsplit('.').next().unwrap_or(app_id));
    let mut s: String = raw
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    s = s
        .trim_start_matches(|c: char| !c.is_ascii_lowercase())
        .to_owned();
    s.truncate(40);
    let s = s.trim_end_matches('-').to_owned();
    if s.is_empty() { "window".to_owned() } else { s }
}

/// `base`, or `base-2`, `base-3`, … if taken; `None` if `base` is not a valid
/// id (it is when it comes from [`base_id`]).
pub fn new_id(taken: &BTreeSet<WindowId>, base: &str) -> Option<WindowId> {
    // Of `taken.len() + 1` candidates at least one is free.
    (1..=taken.len() + 1).find_map(|n| {
        let s = if n == 1 {
            base.to_owned()
        } else {
            format!("{base}-{n}")
        };
        WindowId::new(s).ok().filter(|id| !taken.contains(id))
    })
}

/// A path in a terminal title: the title itself, or what follows `host: `.
fn path_in_title(title: &str) -> Option<String> {
    let t = title.trim();
    let candidate = t.rsplit_once(": ").map_or(t, |(_, p)| p).trim();
    (candidate.starts_with('/') || candidate == "~" || candidate.starts_with("~/"))
        .then(|| candidate.to_owned())
}

fn detect(method: Detect, w: &Observed) -> Option<String> {
    let p = w.process.as_ref();
    match method {
        Detect::ArgvLast => p
            .filter(|p| p.argv.len() > 1)
            .and_then(|p| p.argv.last())
            .filter(|a| !a.starts_with('-'))
            .cloned(),
        Detect::Cwd => p.and_then(|p| p.cwd.clone()),
        Detect::ShellCwd => {
            let shells: Vec<&Proc> = p
                .map(|p| {
                    p.children
                        .iter()
                        .filter(|c| {
                            c.argv
                                .first()
                                .is_some_and(|a| SHELLS.contains(&basename(a)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            match shells.as_slice() {
                [one] => one.cwd.clone(),
                _ => path_in_title(&w.title),
            }
        }
    }
}

/// A command line as one string that `sh -c` splits back into `argv`.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// How to start `w` again.
pub fn spec(id: WindowId, w: &Observed, integrations: &[Integration]) -> WindowSpec {
    let found = integration::find(integrations, &w.app_id);
    let command = w
        .process
        .as_ref()
        .filter(|p| !p.argv.is_empty())
        .map(|p| shell_join(&p.argv));
    let mut remembered = BTreeMap::new();
    if let Some(i) = found {
        for key in &i.remember {
            if let Some(value) = i.detect.get(key).and_then(|m| detect(*m, w)) {
                remembered.insert(key.clone(), toml::Value::String(value));
            }
        }
    }
    WindowSpec {
        id,
        integration: found.map(|i| i.name.clone()),
        command,
        app_id: Some(w.app_id.clone()),
        where_: Where::Context,
        remembered,
    }
}

/// The first word of a command line, unquoted (`'my app' x` → `my app`).
pub fn program(command: &str) -> String {
    let c = command.trim_start();
    if let Some(rest) = c.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default().to_owned();
    }
    c.split_whitespace().next().unwrap_or_default().to_owned()
}

/// Whether the restored window `p` may take a window with `app_id` that has
/// no exact match: one of the same integration; without an integration, by
/// the program's name (also for states written before app ids were stored).
fn waits_for(p: &WindowSpec, app_id: &str, integration: Option<&str>) -> bool {
    match (&p.integration, integration) {
        (Some(want), Some(got)) => want == got,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => {
            p.app_id.is_none()
                && p.command
                    .as_deref()
                    .is_some_and(|c| base_id(None, basename(&program(c))) == base_id(None, app_id))
        }
    }
}

/// A window in the live state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveWindow {
    pub id: WindowId,
    pub app_id: String,
}

/// kojed's live state of one running context (ADR 0030).
#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    pub state: State,
    /// Compositor address → window.
    pub windows: BTreeMap<String, LiveWindow>,
    /// Windows of a restored state that have not appeared yet, in start order.
    pub pending: Vec<WindowSpec>,
}

/// A window that joined the live state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub id: WindowId,
    /// Where the active layout wants it: a restored window (M5.3).
    pub place: Option<Placement>,
}

impl Live {
    pub fn new() -> Self {
        Self {
            state: State::new(LayoutName::initial()),
            windows: BTreeMap::new(),
            pending: Vec::new(),
        }
    }

    /// The live state while `state` is being restored: its windows wait for
    /// their windows to appear ([`Live::add`]).
    pub fn restoring(state: State) -> Self {
        let mut state = state;
        let pending = std::mem::take(&mut state.windows);
        Self {
            state,
            windows: BTreeMap::new(),
            pending,
        }
    }

    fn taken(&self) -> BTreeSet<WindowId> {
        self.windows
            .values()
            .map(|w| w.id.clone())
            .chain(self.pending.iter().map(|p| p.id.clone()))
            .collect()
    }

    /// Restored windows that did not appear (their program failed or opened
    /// fewer windows): they leave the state and its layouts.
    pub fn expire_pending(&mut self) -> Vec<WindowId> {
        let gone: Vec<WindowId> = self.pending.drain(..).map(|p| p.id).collect();
        for layout in self.state.layout.values_mut() {
            for id in &gone {
                layout.windows.remove(id);
            }
        }
        gone
    }

    /// A window of the context appeared. A restored window waiting with the
    /// same app id takes it (first in start order) and gets its place from the
    /// active layout; otherwise it gets a new id and joins the active layout
    /// where it is (ADR 0004).
    pub fn add(
        &mut self,
        address: &str,
        app_id: &str,
        integrations: &[Integration],
        placement: Placement,
    ) -> Option<Added> {
        if let Some(w) = self.windows.get(address) {
            return Some(Added {
                id: w.id.clone(),
                place: None,
            });
        }
        let integration = integration::find(integrations, app_id).map(|i| i.name.as_str());
        // The same app id first; then the same integration: programs may
        // change their app id after the window opens (Firefox: host test).
        let waiting = self
            .pending
            .iter()
            .position(|p| p.app_id.as_deref() == Some(app_id))
            .or_else(|| {
                self.pending
                    .iter()
                    .position(|p| waits_for(p, app_id, integration))
            });
        if let Some(i) = waiting {
            let spec = self.pending.remove(i);
            self.windows.insert(
                address.to_owned(),
                LiveWindow {
                    id: spec.id.clone(),
                    app_id: app_id.to_owned(),
                },
            );
            let place = self
                .state
                .layout
                .get(&self.state.active_layout)
                .and_then(|l| l.windows.get(&spec.id))
                .cloned();
            return Some(Added { id: spec.id, place });
        }
        let name = integration::find(integrations, app_id).map(|i| i.name.as_str());
        let id = new_id(&self.taken(), &base_id(name, app_id))?;
        self.windows.insert(
            address.to_owned(),
            LiveWindow {
                id: id.clone(),
                app_id: app_id.to_owned(),
            },
        );
        let active = self.state.active_layout.clone();
        self.state
            .layout
            .entry(active)
            .or_default()
            .windows
            .insert(id.clone(), placement);
        Some(Added { id, place: None })
    }

    /// A window closed: it leaves the state and every layout (ADR 0004).
    pub fn remove(&mut self, address: &str) -> Option<WindowId> {
        let w = self.windows.remove(address)?;
        self.state.windows.retain(|s| s.id != w.id);
        for layout in self.state.layout.values_mut() {
            layout.windows.remove(&w.id);
        }
        Some(w.id)
    }

    /// The state with fresh specs and, if the active layout autosaves, the
    /// current placements. `observe` gives what kojed sees of a window now.
    pub fn snapshot(
        &mut self,
        integrations: &[Integration],
        mut observe: impl FnMut(&str) -> Option<(Observed, Placement)>,
    ) -> State {
        let active = self.state.active_layout.clone();
        let autosave = self.state.layout.get(&active).is_none_or(|l| l.autosave);
        let mut specs = Vec::new();
        for (address, w) in &self.windows {
            let Some((observed, placement)) = observe(address) else {
                continue;
            };
            specs.push(spec(w.id.clone(), &observed, integrations));
            if autosave
                && let Some(layout) = self.state.layout.get_mut(&active)
                && let Some(p) = layout.windows.get_mut(&w.id)
            {
                *p = placement;
            }
        }
        // Restored windows still on their way stay part of the state.
        specs.extend(self.pending.iter().cloned());
        specs.sort_by(|a, b| a.id.cmp(&b.id));
        self.state.windows = specs;
        self.state.clone()
    }
}

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::Rect;

    use super::*;

    fn at(x: i32) -> Placement {
        Placement {
            monitor: "DP-1".into(),
            rect: Rect {
                x,
                y: 0,
                w: 800,
                h: 600,
            },
        }
    }

    fn proc(argv: &[&str], cwd: &str) -> Proc {
        Proc {
            argv: argv.iter().map(|s| (*s).to_owned()).collect(),
            cwd: Some(cwd.into()),
            children: Vec::new(),
        }
    }

    fn id(s: &str) -> WindowId {
        WindowId::new(s).unwrap()
    }

    #[test]
    fn ids() {
        assert_eq!(base_id(Some("editor"), "code"), "editor");
        assert_eq!(base_id(None, "org.mozilla.firefox"), "firefox");
        assert_eq!(base_id(None, "org.gnome.Nautilus"), "nautilus");
        assert_eq!(base_id(None, "Alacritty"), "alacritty");
        assert_eq!(base_id(None, "1Password"), "password");
        assert_eq!(base_id(None, "steam_app_42"), "steam-app-42");
        assert_eq!(base_id(None, ""), "window");
        let taken = BTreeSet::from([id("firefox"), id("firefox-2")]);
        assert_eq!(new_id(&taken, "firefox"), Some(id("firefox-3")));
        assert_eq!(new_id(&taken, "term"), Some(id("term")));
        assert_eq!(new_id(&taken, "Bad"), None);
    }

    #[test]
    fn quoting() {
        let argv: Vec<String> = ["evince", "docs/spec v2.pdf", "it's"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(shell_join(&argv), "evince 'docs/spec v2.pdf' 'it'\\''s'");
        assert_eq!(
            shell_join(&["code".into(), "/home/mj/src/kp".into()]),
            "code /home/mj/src/kp"
        );
    }

    #[test]
    fn terminal_cwd() {
        let all = integration::built_in();
        let mut term = Observed {
            app_id: "foot".into(),
            title: "mj@kp: ~/src/kp".into(),
            process: Some(Proc {
                children: vec![proc(&["-bash"], "/home/mj/src/kp/api")],
                ..proc(&["foot"], "/home/mj")
            }),
        };
        let s = spec(id("terminal"), &term, &all);
        assert_eq!(s.integration.as_deref(), Some("terminal"));
        assert_eq!(
            s.remembered["cwd"].as_str(),
            Some("/home/mj/src/kp/api"),
            "one shell: its cwd"
        );
        // foot server mode: several shells, the title decides.
        if let Some(p) = term.process.as_mut() {
            p.children.push(proc(&["zsh"], "/tmp"));
        }
        assert_eq!(
            spec(id("terminal"), &term, &all).remembered["cwd"].as_str(),
            Some("~/src/kp")
        );
        term.title = "htop".into();
        assert!(
            !spec(id("terminal"), &term, &all)
                .remembered
                .contains_key("cwd")
        );
    }

    #[test]
    fn editor_and_generic() {
        let all = integration::built_in();
        let editor = Observed {
            app_id: "code".into(),
            title: "kp – Visual Studio Code".into(),
            process: Some(proc(
                &["/usr/share/code/code", "/home/mj/src/kp"],
                "/home/mj",
            )),
        };
        let s = spec(id("editor"), &editor, &all);
        assert_eq!(s.remembered["path"].as_str(), Some("/home/mj/src/kp"));
        assert_eq!(
            s.command.as_deref(),
            Some("/usr/share/code/code /home/mj/src/kp")
        );
        let no_folder = Observed {
            process: Some(proc(&["code", "--disable-gpu"], "/")),
            ..editor
        };
        assert!(spec(id("editor"), &no_folder, &all).remembered.is_empty());
        let viewer = Observed {
            app_id: "org.gnome.Evince".into(),
            title: String::new(),
            process: Some(proc(&["evince", "spec.pdf"], "/home/mj")),
        };
        let g = spec(id("evince"), &viewer, &all);
        assert_eq!(
            (g.integration, g.command.as_deref()),
            (None, Some("evince spec.pdf"))
        );
    }

    #[test]
    fn restore_matches_windows_by_app_id() {
        let all = integration::built_in();
        let main = LayoutName::initial();
        let spec_of = |i: &str, app: &str| WindowSpec {
            id: id(i),
            integration: None,
            command: Some(app.into()),
            app_id: Some(app.into()),
            where_: Where::Context,
            remembered: BTreeMap::new(),
        };
        let mut saved = State::new(main.clone());
        saved.windows = vec![
            spec_of("foot", "foot"),
            spec_of("foot-2", "foot"),
            spec_of("viewer", "evince"),
        ];
        for (i, w) in ["foot", "foot-2", "viewer"].iter().enumerate() {
            saved.layout[&main]
                .windows
                .insert(id(w), at(i32::try_from(i).unwrap() * 100));
        }
        let mut live = Live::restoring(saved);
        // A new program's window while restoring: a fresh id that avoids the waiting ones.
        assert_eq!(
            live.add("0x1", "org.mozilla.firefox", &all, at(9)).unwrap(),
            Added {
                id: id("browser"),
                place: None
            }
        );
        let a = live.add("0x2", "foot", &all, at(5)).unwrap();
        assert_eq!(
            a,
            Added {
                id: id("foot"),
                place: Some(at(0))
            },
            "first waiting foot, placed from the layout"
        );
        let b = live.add("0x3", "foot", &all, at(5)).unwrap();
        assert_eq!(
            b,
            Added {
                id: id("foot-2"),
                place: Some(at(100))
            }
        );
        let c = live.add("0x4", "foot", &all, at(5)).unwrap();
        assert_eq!(c.id, id("terminal"), "no foot waiting any more");
        // Saved meanwhile: the window still on its way stays in the state.
        let state = live.snapshot(&all, |_| None);
        assert!(state.windows.iter().any(|w| w.id == id("viewer")));
        assert_eq!(live.expire_pending(), [id("viewer")]);
        assert!(!live.state.layout[&main].windows.contains_key(&id("viewer")));
    }

    /// Host test of M5.3: Firefox's window opened as `org.mozilla.firefox`,
    /// the state had `firefox`; the window got `browser-2` and no place.
    #[test]
    fn an_app_id_change_still_matches_by_integration() {
        let all = integration::built_in();
        let main = LayoutName::initial();
        let mut saved = State::new(main.clone());
        saved.windows = vec![WindowSpec {
            id: id("browser"),
            integration: Some("browser".into()),
            command: Some("/usr/lib64/firefox/firefox".into()),
            app_id: Some("firefox".into()),
            where_: Where::Context,
            remembered: BTreeMap::new(),
        }];
        saved.layout[&main].windows.insert(id("browser"), at(1756));
        let mut live = Live::restoring(saved);
        let added = live.add("0x1", "org.mozilla.firefox", &all, at(0)).unwrap();
        assert_eq!(
            added,
            Added {
                id: id("browser"),
                place: Some(at(1756))
            }
        );
        // Not across integrations: a terminal does not take a waiting viewer.
        let mut saved = State::new(main);
        saved.windows = vec![WindowSpec {
            id: id("viewer"),
            integration: None,
            command: Some("evince".into()),
            app_id: Some("org.gnome.Evince".into()),
            where_: Where::Context,
            remembered: BTreeMap::new(),
        }];
        let mut live = Live::restoring(saved);
        assert_eq!(
            live.add("0x2", "foot", &all, at(0)).unwrap().id,
            id("terminal")
        );
    }

    #[test]
    fn states_without_app_ids_still_match() {
        let all = integration::built_in();
        let old = |i: &str, integration: Option<&str>, command: &str| WindowSpec {
            id: id(i),
            integration: integration.map(str::to_owned),
            command: Some(command.into()),
            app_id: None,
            where_: Where::Context,
            remembered: BTreeMap::new(),
        };
        let mut saved = State::new(LayoutName::initial());
        saved.windows = vec![
            old("terminal", Some("terminal"), "foot -D /home/mj"),
            old("nautilus", None, "nautilus"),
        ];
        let mut live = Live::restoring(saved);
        assert_eq!(
            live.add("0x1", "org.gnome.Nautilus", &all, at(0))
                .unwrap()
                .id,
            id("nautilus")
        );
        assert_eq!(
            live.add("0x2", "kitty", &all, at(0)).unwrap().id,
            id("terminal"),
            "any terminal"
        );
        assert_eq!(program("'my app' --x"), "my app");
        assert_eq!(
            program("/usr/lib64/firefox/firefox"),
            "/usr/lib64/firefox/firefox"
        );
    }

    #[test]
    fn live_state() {
        let all = integration::built_in();
        let mut live = Live::new();
        assert_eq!(
            live.add("0xa", "foot", &all, at(0)).map(|a| a.id),
            Some(id("terminal"))
        );
        assert_eq!(
            live.add("0xb", "foot", &all, at(100)).map(|a| a.id),
            Some(id("terminal-2"))
        );
        assert_eq!(
            live.add("0xa", "foot", &all, at(5)).map(|a| a.id),
            Some(id("terminal")),
            "known window keeps its id"
        );
        let main = LayoutName::initial();
        assert_eq!(live.state.layout[&main].windows.len(), 2);

        let observed = || Observed {
            app_id: "foot".into(),
            title: String::new(),
            process: Some(proc(&["foot"], "/")),
        };
        let state = live.snapshot(&all, |a| {
            Some((observed(), at(if a == "0xa" { 42 } else { 7 })))
        });
        assert_eq!(
            state
                .windows
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            ["terminal", "terminal-2"]
        );
        assert_eq!(
            state.layout[&main].windows[&id("terminal")].rect.x,
            42,
            "autosave: current placement"
        );
        assert!(state.issues().is_empty(), "{:?}", state.issues());

        assert_eq!(live.remove("0xb"), Some(id("terminal-2")));
        assert!(
            !live.state.layout[&main]
                .windows
                .contains_key(&id("terminal-2"))
        );
        assert_eq!(
            live.add("0xc", "foot", &all, at(0)).map(|a| a.id),
            Some(id("terminal-2")),
            "a freed id is reused"
        );
    }
}
