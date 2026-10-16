//! States: named snapshots of a context's windows and layouts.
//!
//! Semantics (see `docs/decisions/0005-named-states-and-cli.md`):
//! - `up [-s NAME]` loads a state (default role: `default`).
//! - `down [-s NAME]` writes exactly that one state; `--no-save` writes nothing.
//! - `empty` is reserved, virtual and read-only.
//! - A missing state falls back to `fresh`, then to `empty`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::StateRoles;
use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::name::{LayoutName, StateName, WindowId};

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
pub struct State {
    pub active_layout: LayoutName,
    #[serde(default)]
    pub windows: Vec<WindowSpec>,
    #[serde(default)]
    pub layout: BTreeMap<LayoutName, Layout>,
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

impl State {
    pub fn from_toml(text: &str, path: &Path) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })
    }

    /// Consistency checks that serde cannot express.
    pub fn issues(&self) -> Vec<StateIssue> {
        let mut issues = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
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
