//! States: named snapshots of a context's windows and layouts.
//!
//! Semantics (see `docs/decisions/0005-named-states-and-cli.md`):
//! - `up [-s NAME]` loads a state (default role: `default`).
//! - `down [-s NAME]` writes exactly that one state; `--no-save` writes nothing.
//! - `empty` is reserved, virtual and read-only.
//! - A missing state falls back to `fresh`, then to `empty`.
//!
//! On load, dangling references are repaired and reported; duplicate window
//! ids are an error (`docs/decisions/0010-state-consistency-on-load.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::StateRoles;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::layout::Layout;
use crate::name::{ContextName, LayoutName, StateName, WindowId};
use crate::paths::Paths;

/// Where a window's program runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Where {
    /// In the context container itself.
    #[default]
    Context,
    /// In the context's dev container (inner Podman).
    Dev,
}

/// One window of a state: how to start it again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSpec {
    pub id: WindowId,
    /// Integration that knows how to launch and remember this program (e.g. `editor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
    /// Fallback: the original command line (`generic` integration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, rename = "where")]
    pub where_: Where,
    /// Values an integration remembered, e.g. `path` or `cwd`.
    #[serde(flatten)]
    pub remembered: BTreeMap<String, toml::Value>,
}

/// A stored state: `~/.local/share/koje/<context>/states/<name>.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub active_layout: LayoutName,
    #[serde(default)]
    pub windows: Vec<WindowSpec>,
    /// Layouts in file order; `layout next|prev` follow it (ADR 0011).
    #[serde(default)]
    pub layout: IndexMap<LayoutName, Layout>,
}

/// Problems that make a state inconsistent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateIssue {
    #[error("active layout {0} does not exist")]
    MissingActiveLayout(LayoutName),
    #[error("layout {layout} references unknown window {window}")]
    UnknownWindow {
        layout: LayoutName,
        window: WindowId,
    },
    #[error("window id {0} is used twice")]
    DuplicateWindow(WindowId),
}

/// A state read from disk, with the repairs applied while loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub state: State,
    /// Dangling references that were fixed; the caller should report them.
    pub repaired: Vec<StateIssue>,
}

impl State {
    /// A state without windows whose only layout is `active_layout`.
    pub fn new(active_layout: LayoutName) -> Self {
        Self {
            layout: IndexMap::from([(active_layout.clone(), Layout::default())]),
            active_layout,
            windows: Vec::new(),
        }
    }

    pub fn from_toml(text: &str, path: &Path) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })
    }

    pub fn to_toml(&self, path: &Path) -> Result<String> {
        toml::to_string(self).map_err(|source| Error::Serialize {
            path: path.to_owned(),
            source,
        })
    }

    /// Parse and repair (see [`State::repair`]).
    pub fn parse(text: &str, path: &Path) -> Result<Loaded> {
        let mut state = Self::from_toml(text, path)?;
        let repaired = state.repair().map_err(|issue| Error::InvalidState {
            path: path.to_owned(),
            issue,
        })?;
        Ok(Loaded { state, repaired })
    }

    /// Read a state file; `None` if it does not exist.
    pub fn load(path: &Path) -> Result<Option<Loaded>> {
        fsutil::read_optional(path)?
            .map(|text| Self::parse(&text, path))
            .transpose()
    }

    /// Write atomically. Refuses a state with [`State::issues`].
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(issue) = self.issues().into_iter().next() {
            return Err(Error::InvalidState {
                path: path.to_owned(),
                issue,
            });
        }
        fsutil::write_atomic(path, self.to_toml(path)?.as_bytes())
    }

    /// Fix dangling references and return what was fixed (ADR 0010):
    ///
    /// - a layout entry for an unknown window is dropped (closed windows leave
    ///   all layouts);
    /// - a missing active layout becomes the first layout, or an empty layout of
    ///   that name if there is none.
    ///
    /// Duplicate window ids cannot be repaired without guessing and are returned
    /// as `Err`.
    pub fn repair(&mut self) -> Result<Vec<StateIssue>, StateIssue> {
        let mut repaired = Vec::new();
        for issue in self.issues() {
            match issue {
                StateIssue::DuplicateWindow(_) => return Err(issue),
                StateIssue::UnknownWindow {
                    ref layout,
                    ref window,
                } => {
                    if let Some(l) = self.layout.get_mut(layout) {
                        l.windows.remove(window);
                    }
                    repaired.push(issue);
                }
                StateIssue::MissingActiveLayout(ref name) => {
                    match self.layout.keys().next() {
                        Some(first) => self.active_layout = first.clone(),
                        None => {
                            self.layout.insert(name.clone(), Layout::default());
                        }
                    }
                    repaired.push(issue);
                }
            }
        }
        Ok(repaired)
    }

    /// Consistency checks that serde cannot express.
    pub fn issues(&self) -> Vec<StateIssue> {
        let mut issues = Vec::new();
        let mut seen = BTreeSet::new();
        for w in &self.windows {
            if !seen.insert(&w.id) {
                issues.push(StateIssue::DuplicateWindow(w.id.clone()));
            }
        }
        if !self.layout.contains_key(&self.active_layout) {
            issues.push(StateIssue::MissingActiveLayout(self.active_layout.clone()));
        }
        for (name, layout) in &self.layout {
            for window in layout.windows.keys() {
                if !seen.contains(window) {
                    issues.push(StateIssue::UnknownWindow {
                        layout: name.clone(),
                        window: window.clone(),
                    });
                }
            }
        }
        issues
    }
}

/// Outcome of resolving which state `up` should load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// Load this stored state.
    Load {
        name: StateName,
        /// True if `name` is not what was requested (fallback to `fresh`).
        fell_back: bool,
    },
    /// Start the context without windows.
    Empty {
        /// True if `empty` was not requested explicitly.
        fell_back: bool,
    },
}

/// Decide which state to load: requested → `fresh` role → `empty`.
pub fn resolve_load(
    requested: &StateName,
    roles: &StateRoles,
    exists: impl Fn(&StateName) -> bool,
) -> Resolved {
    if requested.is_empty_state() {
        return Resolved::Empty { fell_back: false };
    }
    if exists(requested) {
        return Resolved::Load {
            name: requested.clone(),
            fell_back: false,
        };
    }
    if exists(&roles.fresh) {
        return Resolved::Load {
            name: roles.fresh.clone(),
            fell_back: true,
        };
    }
    Resolved::Empty { fell_back: true }
}

/// Read a stored state of a context; `None` if it does not exist.
/// `empty` is virtual and never read from disk.
pub fn read(paths: &Paths, ctx: &ContextName, name: &StateName) -> Result<Option<Loaded>> {
    if name.is_empty_state() {
        return Ok(Some(Loaded {
            state: State::new(LayoutName::initial()),
            repaired: Vec::new(),
        }));
    }
    State::load(&paths.state_file(ctx, name))
}

/// Write a state of a context atomically; `empty` is refused.
pub fn write(paths: &Paths, ctx: &ContextName, name: &StateName, state: &State) -> Result<()> {
    ensure_writable(name)?;
    state.save(&paths.state_file(ctx, name))
}

/// Fail if `name` may not be written (`empty` is read-only).
pub fn ensure_writable(name: &StateName) -> Result<()> {
    if name.is_empty_state() {
        Err(Error::ReservedState(name.to_string()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str) -> StateName {
        StateName::new(n).unwrap()
    }

    /// The example from the concept document.
    const EXAMPLE: &str = r#"
active_layout = "code"

windows = [
  { id = "editor",  integration = "editor",   path = "~/src/kundenportal" },
  { id = "browser", integration = "browser" },
  { id = "term",    integration = "terminal", where = "dev", cwd = "/workspace" },
  { id = "spec",    command = "evince docs/spezifikation.pdf" },
]

[layout.code.windows]
editor  = { monitor = "DP-1", x = 0,    y = 0,    w = 2660, h = 2160 }
browser = { monitor = "DP-1", x = 2660, y = 0,    w = 2460, h = 1240 }
term    = { monitor = "DP-1", x = 2660, y = 1240, w = 2460, h = 920 }

[layout.review]
autosave = false
[layout.review.windows]
browser = { monitor = "DP-1", x = 0,    y = 0, w = 3170, h = 2160 }
editor  = { monitor = "DP-1", x = 3170, y = 0, w = 1950, h = 2160 }
"#;

    #[test]
    fn parses_concept_example() {
        let st = State::from_toml(EXAMPLE, Path::new("default.toml")).unwrap();
        assert_eq!(st.windows.len(), 4);
        assert_eq!(st.windows[2].where_, Where::Dev);
        assert_eq!(
            st.windows[0].remembered["path"].as_str(),
            Some("~/src/kundenportal")
        );
        assert!(!st.layout[&LayoutName::new("review").unwrap()].autosave);
        assert!(st.issues().is_empty(), "{:?}", st.issues());
    }

    fn ctx() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    fn temp_paths(tmp: &tempfile::TempDir) -> Paths {
        let r = tmp.path();
        Paths::with_roots(r.join("c"), r.join("d"), r.join("s"))
    }

    #[test]
    fn round_trips_concept_example() {
        let st = State::from_toml(EXAMPLE, Path::new("x")).unwrap();
        let text = st.to_toml(Path::new("x")).unwrap();
        assert_eq!(State::from_toml(&text, Path::new("x")).unwrap(), st);
        assert!(text.find("[layout.code").unwrap() < text.find("[layout.review").unwrap());
    }

    #[test]
    fn keeps_layout_file_order() {
        let text = EXAMPLE
            .replace("[layout.code.windows]", "[layout.zeta.windows]")
            .replace("\"code\"", "\"zeta\"");
        let st = State::from_toml(&text, Path::new("x")).unwrap();
        let names: Vec<_> = st.layout.keys().map(LayoutName::as_str).collect();
        assert_eq!(names, ["zeta", "review"]);
    }

    #[test]
    fn save_and_read_through_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        let st = State::from_toml(EXAMPLE, Path::new("x")).unwrap();
        assert_eq!(read(&paths, &ctx(), &s("default")).unwrap(), None);
        write(&paths, &ctx(), &s("default"), &st).unwrap();
        let loaded = read(&paths, &ctx(), &s("default")).unwrap().unwrap();
        assert_eq!(loaded.state, st);
        assert_eq!(loaded.repaired, []);
    }

    #[test]
    fn writing_empty_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        let st = State::new(LayoutName::initial());
        assert!(matches!(
            write(&paths, &ctx(), &s("empty"), &st),
            Err(Error::ReservedState(_))
        ));
        assert!(!paths.states_dir(&ctx()).exists());
    }

    #[test]
    fn empty_is_virtual() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = read(&temp_paths(&tmp), &ctx(), &s("empty"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.state.windows, []);
        assert_eq!(loaded.state.issues(), []);
    }

    #[test]
    fn saving_inconsistent_state_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut st = State::from_toml(EXAMPLE, Path::new("x")).unwrap();
        st.active_layout = LayoutName::new("docs").unwrap();
        let path = tmp.path().join("x.toml");
        assert!(matches!(st.save(&path), Err(Error::InvalidState { .. })));
        assert!(!path.exists());
    }

    #[test]
    fn load_drops_unknown_windows_from_layouts() {
        let text = EXAMPLE.replace(
            "term    = { monitor = \"DP-1\", x = 2660, y = 1240",
            "ghost   = { monitor = \"DP-1\", x = 2660, y = 1240",
        );
        let loaded = State::parse(&text, Path::new("x")).unwrap();
        assert!(matches!(
            &loaded.repaired[..],
            [StateIssue::UnknownWindow { window, .. }] if window.as_str() == "ghost"
        ));
        let code = &loaded.state.layout[&LayoutName::new("code").unwrap()];
        assert_eq!(code.windows.len(), 2);
        assert_eq!(loaded.state.issues(), []);
    }

    #[test]
    fn load_replaces_missing_active_layout() {
        let text = EXAMPLE.replace("active_layout = \"code\"", "active_layout = \"docs\"");
        let loaded = State::parse(&text, Path::new("x")).unwrap();
        assert_eq!(loaded.state.active_layout.as_str(), "code");
        assert_eq!(loaded.repaired.len(), 1);

        let bare = State::parse("active_layout = \"docs\"\n", Path::new("x")).unwrap();
        assert_eq!(bare.state.layout.len(), 1);
        assert_eq!(bare.state.issues(), []);
    }

    #[test]
    fn load_rejects_duplicate_window_ids() {
        let text = EXAMPLE.replace("{ id = \"browser\"", "{ id = \"editor\"");
        assert!(matches!(
            State::parse(&text, Path::new("x")),
            Err(Error::InvalidState {
                issue: StateIssue::DuplicateWindow(_),
                ..
            })
        ));
    }

    #[test]
    fn detects_dangling_references() {
        let text = EXAMPLE.replace("active_layout = \"code\"", "active_layout = \"docs\"");
        let st = State::from_toml(&text, Path::new("x")).unwrap();
        assert!(matches!(
            st.issues()[..],
            [StateIssue::MissingActiveLayout(_)]
        ));
    }

    #[test]
    fn resolve_prefers_requested() {
        let r = resolve_load(&s("review"), &StateRoles::default(), |_| true);
        assert_eq!(
            r,
            Resolved::Load {
                name: s("review"),
                fell_back: false
            }
        );
    }

    #[test]
    fn resolve_falls_back_to_fresh_then_empty() {
        let roles = StateRoles::default();
        let only_fresh = |n: &StateName| n.as_str() == "fresh";
        assert_eq!(
            resolve_load(&s("default"), &roles, only_fresh),
            Resolved::Load {
                name: s("fresh"),
                fell_back: true
            }
        );
        assert_eq!(
            resolve_load(&s("default"), &roles, |_| false),
            Resolved::Empty { fell_back: true }
        );
    }

    #[test]
    fn explicit_empty_is_not_a_fallback() {
        let r = resolve_load(&s("empty"), &StateRoles::default(), |_| true);
        assert_eq!(r, Resolved::Empty { fell_back: false });
    }

    #[test]
    fn empty_is_read_only() {
        assert!(ensure_writable(&s("empty")).is_err());
        assert!(ensure_writable(&s("default")).is_ok());
    }
}
