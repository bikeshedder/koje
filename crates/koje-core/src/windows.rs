//! Which context each window belongs to, and the compositor commands that
//! follow from it (M4.4–M4.6, ADR 0018): a tag for the border color, and the
//! context's workspace (concept: one context, one Hyprland workspace). Pure: kojed feeds it windows and
//! the contexts their pids resolved to, and sends the commands it returns.

use std::collections::{BTreeMap, BTreeSet};

use crate::color::Color;
use crate::name::ContextName;
use crate::ports::CompositorCommand;

/// The tag of a context's windows, `koje-<context>`.
pub fn tag(ctx: &ContextName) -> String {
    format!("koje-{ctx}")
}

/// The border rule of a context: its color, dimmed for unfocused windows.
pub fn rule(ctx: &ContextName, color: Color) -> CompositorCommand {
    CompositorCommand::ContextRule {
        tag: tag(ctx),
        active: color,
        inactive: color.dimmed(),
    }
}

/// The workspace of a context: named like the context. Context names start
/// with a letter, so they never clash with numbered or special workspaces.
pub fn workspace(ctx: &ContextName) -> String {
    ctx.to_string()
}

pub fn tag_window(address: &str, ctx: &ContextName) -> CompositorCommand {
    CompositorCommand::TagWindow {
        address: address.to_owned(),
        tag: tag(ctx),
    }
}

/// Move a new window of `ctx` from `current` to the context's workspace.
/// Windows on special workspaces (overlays, stash) stay where they are.
pub fn move_to_context(
    address: &str,
    current: &str,
    ctx: &ContextName,
) -> Option<CompositorCommand> {
    let target = workspace(ctx);
    (current != target && !current.starts_with("special:")).then(|| {
        CompositorCommand::MoveToWorkspace {
            address: address.to_owned(),
            workspace: target,
        }
    })
}

/// Show a context: its workspace (created by the compositor if empty).
pub fn focus(ctx: &ContextName) -> CompositorCommand {
    CompositorCommand::FocusWorkspace {
        workspace: workspace(ctx),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    pid: u32,
    /// `None`: a host window (or a container that is not a context).
    context: Option<ContextName>,
}

/// The result of [`WindowMap::retain`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Retained {
    /// Present but not known yet: to resolve and insert.
    pub added: Vec<String>,
    /// Context windows that are gone.
    pub removed: Vec<(String, ContextName)>,
}

/// The windows kojed knows, by compositor address.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WindowMap {
    windows: BTreeMap<String, Tracked>,
}

impl WindowMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// The context of `pid` if another window of the same process is known,
    /// so kojed asks the container runtime once per process. The cache lives as
    /// long as one of the process's windows: pids are reused.
    pub fn known_pid(&self, pid: u32) -> Option<Option<ContextName>> {
        (pid != 0)
            .then(|| self.windows.values().find(|w| w.pid == pid))
            .flatten()
            .map(|w| w.context.clone())
    }

    pub fn contains(&self, address: &str) -> bool {
        self.windows.contains_key(address)
    }

    /// Record a window (see [`tag_window`], [`move_to_context`] for what follows).
    pub fn insert(&mut self, address: &str, pid: u32, context: Option<ContextName>) {
        self.windows
            .insert(address.to_owned(), Tracked { pid, context });
    }

    /// Forget a closed window; its context, if it had one.
    pub fn remove(&mut self, address: &str) -> Option<ContextName> {
        self.windows.remove(address).and_then(|w| w.context)
    }

    /// Keep only the windows in `present` (after a reconnect, windows may
    /// have closed unseen).
    pub fn retain<'a>(&mut self, present: impl IntoIterator<Item = &'a str>) -> Retained {
        let present: BTreeSet<&str> = present.into_iter().collect();
        let mut removed = Vec::new();
        self.windows.retain(|a, w| {
            let keep = present.contains(a.as_str());
            if !keep && let Some(ctx) = &w.context {
                removed.push((a.clone(), ctx.clone()));
            }
            keep
        });
        let added = present
            .into_iter()
            .filter(|a| !self.windows.contains_key(*a))
            .map(str::to_owned)
            .collect();
        Retained { added, removed }
    }

    pub fn context_of(&self, address: &str) -> Option<&ContextName> {
        self.windows.get(address).and_then(|w| w.context.as_ref())
    }

    /// The contexts that have windows.
    pub fn contexts(&self) -> BTreeSet<&ContextName> {
        self.windows
            .values()
            .filter_map(|w| w.context.as_ref())
            .collect()
    }

    /// Tag commands for every context window (after a config reload).
    pub fn tags(&self) -> Vec<CompositorCommand> {
        self.windows
            .iter()
            .filter_map(|(address, w)| w.context.as_ref().map(|ctx| tag_window(address, ctx)))
            .collect()
    }

    /// Windows per context, for status output.
    pub fn counts(&self) -> BTreeMap<&ContextName, usize> {
        let mut counts = BTreeMap::new();
        for ctx in self.windows.values().filter_map(|w| w.context.as_ref()) {
            *counts.entry(ctx).or_default() += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(s: &str) -> ContextName {
        ContextName::new(s).unwrap()
    }

    fn tag_cmd(address: &str, t: &str) -> CompositorCommand {
        CompositorCommand::TagWindow {
            address: address.into(),
            tag: t.into(),
        }
    }

    #[test]
    fn rules_and_tags() {
        assert_eq!(tag(&ctx("work")), "koje-work");
        assert_eq!(
            rule(&ctx("work"), Color::rgb(0x2e, 0x7d, 0x5b)),
            CompositorCommand::ContextRule {
                tag: "koje-work".into(),
                active: Color::rgb(0x2e, 0x7d, 0x5b),
                inactive: Color::rgb(0x2b, 0x52, 0x41),
            }
        );
    }

    #[test]
    fn workspaces() {
        let work = ctx("work");
        assert_eq!(workspace(&work), "work");
        assert_eq!(
            move_to_context("0xa", "1", &work),
            Some(CompositorCommand::MoveToWorkspace {
                address: "0xa".into(),
                workspace: "work".into()
            })
        );
        assert_eq!(move_to_context("0xa", "work", &work), None);
        assert_eq!(move_to_context("0xa", "special:mail", &work), None);
        assert_eq!(
            focus(&work),
            CompositorCommand::FocusWorkspace {
                workspace: "work".into()
            }
        );
    }

    #[test]
    fn tracks_windows_and_caches_pids() {
        let mut m = WindowMap::new();
        assert_eq!(m.known_pid(7), None);
        m.insert("0xa", 7, Some(ctx("work")));
        m.insert("0xb", 9, None);
        assert_eq!(m.known_pid(7), Some(Some(ctx("work"))));
        assert_eq!(m.known_pid(9), Some(None));
        assert_eq!(m.known_pid(0), None, "pid 0 is unknown, never cached");
        assert_eq!(m.tags(), [tag_cmd("0xa", "koje-work")]);
        assert_eq!(m.context_of("0xa"), Some(&ctx("work")));
        assert_eq!(m.counts(), BTreeMap::from([(&ctx("work"), 1)]));

        assert_eq!(m.remove("0xa"), Some(ctx("work")));
        assert_eq!(m.known_pid(7), None, "cache ends with the last window");
        assert_eq!(m.remove("0xb"), None);
        assert!(m.contexts().is_empty());
    }

    #[test]
    fn retain_after_reconnect() {
        let mut m = WindowMap::new();
        m.insert("0xa", 7, Some(ctx("a")));
        m.insert("0xb", 8, Some(ctx("b")));
        let r = m.retain(["0xb", "0xc"]);
        assert_eq!(r.added, ["0xc"]);
        assert_eq!(r.removed, [("0xa".to_owned(), ctx("a"))]);
        assert!(!m.contains("0xa"));
        assert!(m.contains("0xb"));
    }
}
