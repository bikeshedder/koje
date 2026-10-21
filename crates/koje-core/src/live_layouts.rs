//! Layouts and the stash of a running context (M5.5, M5.6, ADR 0004): the
//! operations on kojed's live state and the compositor commands they need.
//! Pure: kojed passes in where the context's windows are now.

use std::collections::{BTreeMap, BTreeSet};

use crate::capture::Live;
use crate::error::{Error, Result};
use crate::layout::{Layout, Placement, plan_switch};
use crate::layouts;
use crate::name::{ContextName, LayoutName, WindowId};
use crate::ports::CompositorCommand;
use crate::state::{State, WindowSpec};
use crate::windows;

/// A context window as the compositor shows it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub address: String,
    pub workspace: String,
    pub placement: Placement,
}

/// The stash of a context: a hidden special workspace (concept "Ablage").
pub fn stash_workspace(ctx: &ContextName) -> String {
    format!("special:stash-{ctx}")
}

/// What a state switch needs done (see [`Live::switch_state`]).
#[derive(Debug, Clone, PartialEq)]
pub struct StateChange {
    /// Windows (addresses) to close.
    pub close: Vec<String>,
    /// Windows to start, as in a restore.
    pub start: Vec<WindowSpec>,
    /// How many windows stay.
    pub kept: usize,
    /// Arranging the windows that stay.
    pub commands: Vec<CompositorCommand>,
}

/// A layout as `koje layout list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    pub name: LayoutName,
    pub active: bool,
    pub autosave: bool,
    pub windows: usize,
}

/// Where the context's windows are now.
struct View {
    by_id: BTreeMap<WindowId, Shown>,
    visible: BTreeSet<WindowId>,
}

impl Live {
    fn view(&self, ctx: &ContextName, shown: &[Shown]) -> View {
        let stash = stash_workspace(ctx);
        let mut by_id = BTreeMap::new();
        let mut visible = BTreeSet::new();
        for s in shown {
            if let Some(w) = self.windows.get(&s.address) {
                if s.workspace != stash {
                    visible.insert(w.id.clone());
                }
                by_id.insert(w.id.clone(), s.clone());
            }
        }
        View { by_id, visible }
    }

    fn active_mut(&mut self) -> Result<&mut Layout> {
        let name = self.state.active_layout.clone();
        self.state
            .layout
            .get_mut(&name)
            .ok_or(Error::UnknownLayout(name))
    }

    /// The active layout takes the current placements if it autosaves
    /// (before it is left).
    fn keep_arrangement(&mut self, view: &View) -> Result<()> {
        let active = self.active_mut()?;
        if active.autosave {
            for id in &view.visible {
                if let Some(s) = view.by_id.get(id) {
                    active.windows.insert(id.clone(), s.placement.clone());
                }
            }
        }
        Ok(())
    }

    /// `layout <name>`: show the layout's windows at their places, stash the
    /// rest. Never starts programs.
    pub fn switch_layout(
        &mut self,
        ctx: &ContextName,
        target: &LayoutName,
        shown: &[Shown],
    ) -> Result<Vec<CompositorCommand>> {
        if !self.state.layout.contains_key(target) {
            return Err(Error::UnknownLayout(target.clone()));
        }
        let view = self.view(ctx, shown);
        self.keep_arrangement(&view)?;
        layouts::activate(&mut self.state, target)?;
        Ok(self.arrange(ctx, &view))
    }

    /// Commands that show the active layout: its windows at their places,
    /// the other visible windows to the stash.
    fn arrange(&self, ctx: &ContextName, view: &View) -> Vec<CompositorCommand> {
        let Some(active) = self.state.layout.get(&self.state.active_layout) else {
            return Vec::new();
        };
        let running: BTreeSet<WindowId> = view.by_id.keys().cloned().collect();
        let plan = plan_switch(&running, &view.visible, active);
        let workspace = windows::workspace(ctx);
        let stash = stash_workspace(ctx);
        let mut commands = Vec::new();
        for id in &plan.stash {
            if let Some(s) = view.by_id.get(id) {
                commands.push(CompositorCommand::MoveToWorkspace {
                    address: s.address.clone(),
                    workspace: stash.clone(),
                });
            }
        }
        for (id, placement) in plan.place {
            let Some(s) = view.by_id.get(&id) else {
                continue;
            };
            if s.workspace != workspace {
                commands.push(CompositorCommand::MoveToWorkspace {
                    address: s.address.clone(),
                    workspace: workspace.clone(),
                });
            }
            commands.push(CompositorCommand::Place {
                address: s.address.clone(),
                placement,
            });
        }
        commands
    }

    /// Switch the running context to another state (M5.9, concept "Zustand
    /// wechseln"): windows whose id is in `target` stay and are arranged by
    /// its active layout, the others are to be closed, the target's missing
    /// windows are to be started (they wait like a restore). Windows asked to
    /// close stay tracked (and visible) until they are gone: a program may ask
    /// to save first.
    pub fn switch_state(
        &mut self,
        ctx: &ContextName,
        target: State,
        shown: &[Shown],
    ) -> StateChange {
        let wanted: BTreeSet<&WindowId> = target.windows.iter().map(|w| &w.id).collect();
        let close: Vec<String> = self
            .windows
            .iter()
            .filter(|(_, w)| !wanted.contains(&w.id))
            .map(|(a, _)| a.clone())
            .collect();
        let open: BTreeSet<&WindowId> = self.windows.values().map(|w| &w.id).collect();
        let start: Vec<WindowSpec> = target
            .windows
            .iter()
            .filter(|w| !open.contains(&w.id))
            .cloned()
            .collect();
        let kept = target.windows.len() - start.len();
        let mut state = target;
        state.windows.clear();
        self.state = state;
        self.pending.clone_from(&start);
        let staying: Vec<Shown> = shown
            .iter()
            .filter(|s| !close.contains(&s.address))
            .cloned()
            .collect();
        let view = self.view(ctx, &staying);
        StateChange {
            commands: self.arrange(ctx, &view),
            close,
            start,
            kept,
        }
    }

    /// `layout next|prev` (`step` 1 or -1), in file order (ADR 0011).
    pub fn cycle_layout(
        &mut self,
        ctx: &ContextName,
        step: i32,
        shown: &[Shown],
    ) -> Result<(LayoutName, Vec<CompositorCommand>)> {
        let target = if step < 0 {
            layouts::prev(&self.state)?
        } else {
            layouts::next(&self.state)?
        };
        let commands = self.switch_layout(ctx, &target, shown)?;
        Ok((target, commands))
    }

    /// `layout new <name> [--copy]`: append a layout and switch to it; empty
    /// (everything to the stash) or a copy of the current arrangement.
    pub fn new_layout(
        &mut self,
        ctx: &ContextName,
        name: LayoutName,
        copy: bool,
        shown: &[Shown],
    ) -> Result<Vec<CompositorCommand>> {
        if self.state.layout.contains_key(&name) {
            return Err(Error::LayoutExists(name));
        }
        let view = self.view(ctx, shown);
        let mut layout = Layout::default();
        if copy {
            for id in &view.visible {
                if let Some(s) = view.by_id.get(id) {
                    layout.windows.insert(id.clone(), s.placement.clone());
                }
            }
        }
        self.state.layout.insert(name.clone(), layout);
        self.switch_layout(ctx, &name, shown)
    }

    /// `layout save`: the current arrangement becomes the active layout.
    pub fn save_layout(&mut self, ctx: &ContextName, shown: &[Shown]) -> Result<()> {
        let view = self.view(ctx, shown);
        let active = self.active_mut()?;
        active.windows = view
            .visible
            .iter()
            .filter_map(|id| {
                view.by_id
                    .get(id)
                    .map(|s| (id.clone(), s.placement.clone()))
            })
            .collect();
        Ok(())
    }

    pub fn layouts(&self) -> Vec<LayoutInfo> {
        self.state
            .layout
            .iter()
            .map(|(name, l)| LayoutInfo {
                name: name.clone(),
                active: *name == self.state.active_layout,
                autosave: l.autosave,
                windows: l.windows.len(),
            })
            .collect()
    }

    /// `stash`: the window leaves the active layout and goes to the stash.
    pub fn stash(&mut self, ctx: &ContextName, address: &str) -> Result<Vec<CompositorCommand>> {
        let id = self
            .windows
            .get(address)
            .map(|w| w.id.clone())
            .ok_or_else(|| Error::UnknownWindow(address.to_owned()))?;
        self.active_mut()?.windows.remove(&id);
        Ok(vec![CompositorCommand::MoveToWorkspace {
            address: address.to_owned(),
            workspace: stash_workspace(ctx),
        }])
    }

    /// The address of a window by id.
    pub fn address_of(&self, id: &WindowId) -> Option<&str> {
        self.windows
            .iter()
            .find(|(_, w)| w.id == *id)
            .map(|(a, _)| a.as_str())
    }

    /// `unstash <window> [--to <layout>]`: the window joins `to` (default:
    /// the active layout) where it is; shown if that is the active layout.
    pub fn unstash(
        &mut self,
        ctx: &ContextName,
        id: &WindowId,
        to: Option<&LayoutName>,
        shown: &[Shown],
    ) -> Result<Vec<CompositorCommand>> {
        let address = self
            .address_of(id)
            .ok_or_else(|| Error::UnknownWindow(id.to_string()))?
            .to_owned();
        let target = to
            .cloned()
            .unwrap_or_else(|| self.state.active_layout.clone());
        let s = shown
            .iter()
            .find(|s| s.address == address)
            .ok_or_else(|| Error::UnknownWindow(id.to_string()))?;
        self.state
            .layout
            .get_mut(&target)
            .ok_or_else(|| Error::UnknownLayout(target.clone()))?
            .windows
            .insert(id.clone(), s.placement.clone());
        if target != self.state.active_layout || s.workspace == windows::workspace(ctx) {
            return Ok(Vec::new());
        }
        Ok(vec![CompositorCommand::MoveToWorkspace {
            address,
            workspace: windows::workspace(ctx),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration;
    use crate::layout::Rect;

    fn ctx() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    fn id(s: &str) -> WindowId {
        WindowId::new(s).unwrap()
    }

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

    fn shown(address: &str, workspace: &str, x: i32) -> Shown {
        Shown {
            address: address.into(),
            workspace: workspace.into(),
            placement: at(x),
        }
    }

    /// Two terminals (`terminal`, `terminal-2`) in `main`.
    fn live() -> Live {
        let all = integration::built_in();
        let mut live = Live::new();
        live.add("0xa", "foot", &all, at(0));
        live.add("0xb", "foot", &all, at(100));
        live
    }

    fn mv(address: &str, workspace: &str) -> CompositorCommand {
        CompositorCommand::MoveToWorkspace {
            address: address.into(),
            workspace: workspace.into(),
        }
    }

    #[test]
    fn new_layout_switch_and_back() {
        let mut live = live();
        let now = [shown("0xa", "kp", 5), shown("0xb", "kp", 105)];
        // A new empty layout: everything goes to the stash; `main` keeps
        // the current places (autosave).
        let review = LayoutName::new("review").unwrap();
        let c = live
            .new_layout(&ctx(), review.clone(), false, &now)
            .unwrap();
        assert_eq!(
            c,
            [mv("0xa", "special:stash-kp"), mv("0xb", "special:stash-kp")]
        );
        assert_eq!(live.state.active_layout, review);
        let main = LayoutName::initial();
        assert_eq!(live.state.layout[&main].windows[&id("terminal")], at(5));

        // Unstash one into `review`: shown again.
        let stashed = [
            shown("0xa", "special:stash-kp", 5),
            shown("0xb", "special:stash-kp", 105),
        ];
        let c = live
            .unstash(&ctx(), &id("terminal-2"), None, &stashed)
            .unwrap();
        assert_eq!(c, [mv("0xb", "kp")]);

        // Back to `main`: both shown at their places.
        let now = [shown("0xa", "special:stash-kp", 5), shown("0xb", "kp", 300)];
        let (name, c) = live.cycle_layout(&ctx(), 1, &now).unwrap();
        assert_eq!(name, main);
        assert_eq!(
            c,
            [
                mv("0xa", "kp"),
                CompositorCommand::Place {
                    address: "0xa".into(),
                    placement: at(5)
                },
                CompositorCommand::Place {
                    address: "0xb".into(),
                    placement: at(105)
                },
            ]
        );
        assert_eq!(
            live.state.layout[&review].windows[&id("terminal-2")],
            at(300),
            "review autosaved"
        );
        let infos = live.layouts();
        assert_eq!(
            infos
                .iter()
                .map(|i| (i.name.as_str(), i.active, i.windows))
                .collect::<Vec<_>>(),
            [("main", true, 2), ("review", false, 1)]
        );
    }

    #[test]
    fn stash_and_errors() {
        let mut live = live();
        assert_eq!(
            live.stash(&ctx(), "0xa").unwrap(),
            [mv("0xa", "special:stash-kp")]
        );
        assert!(
            !live.state.layout[&LayoutName::initial()]
                .windows
                .contains_key(&id("terminal"))
        );
        assert!(live.stash(&ctx(), "0xzz").is_err());
        let nope = LayoutName::new("nope").unwrap();
        assert!(live.switch_layout(&ctx(), &nope, &[]).is_err());
        assert!(
            live.new_layout(&ctx(), LayoutName::initial(), false, &[])
                .is_err()
        );
        // `layout save` with autosave off keeps exactly what is shown.
        layouts::set_autosave(&mut live.state, &LayoutName::initial(), false).unwrap();
        live.save_layout(&ctx(), &[shown("0xb", "kp", 42)]).unwrap();
        assert_eq!(live.state.layout[&LayoutName::initial()].windows.len(), 1);
    }

    #[test]
    fn switch_state_keeps_closes_starts() {
        let mut live = live();
        let target = crate::state::State::parse(
            r#"
active_layout = "talk"
[[windows]]
id = "terminal"
command = "foot"
app_id = "foot"
[[windows]]
id = "slides"
command = "evince talk.pdf"
app_id = "org.gnome.Evince"
[layout.talk.windows]
terminal = { monitor = "DP-1", x = 9, y = 0, w = 800, h = 600 }
slides = { monitor = "DP-1", x = 900, y = 0, w = 800, h = 600 }
"#,
            std::path::Path::new("talk.toml"),
        )
        .unwrap()
        .state;
        let now = [shown("0xa", "kp", 0), shown("0xb", "kp", 100)];
        let change = live.switch_state(&ctx(), target, &now);
        assert_eq!(change.close, ["0xb"], "terminal-2 is not in the target");
        assert_eq!(
            change
                .start
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            ["slides"]
        );
        assert_eq!(change.kept, 1);
        assert_eq!(
            change.commands,
            [CompositorCommand::Place {
                address: "0xa".into(),
                placement: at(9)
            }],
            "the closing window is left alone, not stashed"
        );
        assert_eq!(live.state.active_layout.as_str(), "talk");
        // The started window takes its waiting id and place.
        let added = live
            .add("0xc", "org.gnome.Evince", &integration::built_in(), at(0))
            .unwrap();
        assert_eq!((added.id.as_str(), added.place), ("slides", Some(at(900))));
    }
}
