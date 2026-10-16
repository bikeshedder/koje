//! Layouts: named arrangements of a context's windows.
//!
//! A window may appear in several layouts with different geometry. Running
//! windows that are not part of the active layout live in the *stash* (a
//! hidden Hyprland special workspace). Switching layouts never starts or stops
//! programs; it only moves windows. See `docs/decisions/0004-layouts-in-states.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::name::WindowId;

/// A rectangle in monitor pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Where a window sits within a layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Monitor connector name, e.g. `DP-1`.
    pub monitor: String,
    #[serde(flatten)]
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// When true, moves and resizes in this layout are written back immediately.
    /// When false, the layout is a template; use `koje layout save` to update it.
    #[serde(default = "yes")]
    pub autosave: bool,
    #[serde(default)]
    pub windows: BTreeMap<WindowId, Placement>,
}

fn yes() -> bool {
    true
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            autosave: true,
            windows: BTreeMap::new(),
        }
    }
}

/// What has to happen to switch to a target layout.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SwitchPlan {
    /// Currently visible windows that are not in the target layout → move to the stash.
    pub stash: Vec<WindowId>,
    /// Running windows of the target layout → move/resize (and unstash if needed).
    pub place: Vec<(WindowId, Placement)>,
    /// Windows the target layout references but which are not running. They stay
    /// out of the plan; a layout switch never starts programs.
    pub missing: Vec<WindowId>,
}

/// Compute a layout switch.
///
/// - `running`: all windows of the context that currently exist (visible or stashed)
/// - `visible`: the subset currently shown on the context's workspace
pub fn plan_switch(
    running: &BTreeSet<WindowId>,
    visible: &BTreeSet<WindowId>,
    target: &Layout,
) -> SwitchPlan {
    let mut plan = SwitchPlan::default();
    for id in visible {
        if !target.windows.contains_key(id) {
            plan.stash.push(id.clone());
        }
    }
    for (id, placement) in &target.windows {
        if running.contains(id) {
            plan.place.push((id.clone(), placement.clone()));
        } else {
            plan.missing.push(id.clone());
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> WindowId {
        WindowId::new(s).unwrap()
    }

    fn at(x: i32) -> Placement {
        Placement {
            monitor: "DP-1".into(),
            rect: Rect {
                x,
                y: 0,
                w: 100,
                h: 100,
            },
        }
    }

    fn set(ids: &[&str]) -> BTreeSet<WindowId> {
        ids.iter().map(|s| id(s)).collect()
    }

    #[test]
    fn code_to_review() {
        // Mirrors the concept: code shows editor, browser, term; review shows browser, editor.
        let running = set(&["editor", "browser", "term", "spec"]);
        let visible = set(&["editor", "browser", "term"]);
        let review = Layout {
            autosave: false,
            windows: [(id("browser"), at(0)), (id("editor"), at(3170))].into(),
        };
        let plan = plan_switch(&running, &visible, &review);
        assert_eq!(plan.stash, vec![id("term")]);
        assert_eq!(plan.place.len(), 2);
        assert_eq!(plan.missing, Vec::<WindowId>::new());
    }

    #[test]
    fn stashed_windows_are_unstashed_by_placing_them() {
        let running = set(&["editor", "spec"]);
        let visible = set(&["editor"]);
        let docs = Layout {
            autosave: true,
            windows: [(id("spec"), at(0)), (id("editor"), at(500))].into(),
        };
        let plan = plan_switch(&running, &visible, &docs);
        assert_eq!(plan.stash, Vec::<WindowId>::new());
        assert!(plan.place.iter().any(|(w, _)| *w == id("spec")));
    }

    #[test]
    fn never_starts_missing_windows() {
        let running = set(&["editor"]);
        let target = Layout {
            autosave: true,
            windows: [(id("editor"), at(0)), (id("gone"), at(10))].into(),
        };
        let plan = plan_switch(&running, &running, &target);
        assert_eq!(plan.missing, vec![id("gone")]);
        assert_eq!(plan.place.len(), 1);
    }

    #[test]
    fn placement_toml_is_flat() {
        let p: Placement =
            toml::from_str("monitor = \"DP-1\"\nx = 0\ny = 0\nw = 2660\nh = 2160\n").unwrap();
        assert_eq!(p.rect.w, 2660);
    }
}
