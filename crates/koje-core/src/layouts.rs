//! Operations on the layouts of a state (concept "Layouts bearbeiten", ADR 0004,
//! ADR 0011). All functions are pure: they change a [`State`] in memory; the
//! caller saves it and moves windows (see [`crate::layout::plan_switch`]).
//!
//! "Arrangement" means the live geometry of the windows currently shown on the
//! context's workspace, as reported by the compositor.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::layout::{Layout, Placement};
use crate::name::{LayoutName, WindowId};
use crate::state::{State, WindowSpec};

/// Live geometry of the visible windows.
pub type Arrangement = BTreeMap<WindowId, Placement>;

fn unknown(name: &LayoutName) -> Error {
    Error::UnknownLayout(name.clone())
}

fn ensure_new(state: &State, name: &LayoutName) -> Result<()> {
    if state.layout.contains_key(name) {
        Err(Error::LayoutExists(name.clone()))
    } else {
        Ok(())
    }
}

/// `layout new <name> [--copy]`: append a layout and make it active.
///
/// Without `copy` it starts empty, so every running window goes to the stash.
/// With `copy` it takes the current arrangement.
pub fn create(state: &mut State, name: LayoutName, copy: Option<&Arrangement>) -> Result<()> {
    ensure_new(state, &name)?;
    let mut layout = Layout::default();
    if let Some(arrangement) = copy {
        layout.windows = known(state, arrangement);
    }
    state.layout.insert(name.clone(), layout);
    state.active_layout = name;
    Ok(())
}

/// `layout mv <from> <to>`: rename in place, keeping the position in the cycle.
pub fn rename(state: &mut State, from: &LayoutName, to: LayoutName) -> Result<()> {
    let index = state
        .layout
        .get_index_of(from)
        .ok_or_else(|| unknown(from))?;
    ensure_new(state, &to)?;
    if state.active_layout == *from {
        state.active_layout = to.clone();
    }
    // Cannot fail: `to` is not a key (checked above).
    let _ = state.layout.replace_index(index, to);
    Ok(())
}

/// `layout rm <name>`: the active layout cannot be removed.
pub fn remove(state: &mut State, name: &LayoutName) -> Result<()> {
    if !state.layout.contains_key(name) {
        return Err(unknown(name));
    }
    if state.active_layout == *name {
        return Err(Error::ActiveLayout(name.clone()));
    }
    state.layout.shift_remove(name);
    Ok(())
}

/// `layout autosave <name> on|off`.
pub fn set_autosave(state: &mut State, name: &LayoutName, on: bool) -> Result<()> {
    state
        .layout
        .get_mut(name)
        .ok_or_else(|| unknown(name))?
        .autosave = on;
    Ok(())
}

/// `layout next`: the layout after the active one in file order, wrapping around.
pub fn next(state: &State) -> Result<LayoutName> {
    step(state, |i, len| (i + 1) % len)
}

/// `layout prev`: the layout before the active one, wrapping around.
pub fn prev(state: &State) -> Result<LayoutName> {
    step(state, |i, len| (i + len - 1) % len)
}

fn step(state: &State, to: impl Fn(usize, usize) -> usize) -> Result<LayoutName> {
    let active = &state.active_layout;
    let index = state
        .layout
        .get_index_of(active)
        .ok_or_else(|| unknown(active))?;
    let (name, _) = state
        .layout
        .get_index(to(index, state.layout.len()))
        .ok_or_else(|| unknown(active))?;
    Ok(name.clone())
}

/// Make `name` the active layout (the caller then executes `plan_switch`).
pub fn activate(state: &mut State, name: &LayoutName) -> Result<()> {
    if !state.layout.contains_key(name) {
        return Err(unknown(name));
    }
    state.active_layout = name.clone();
    Ok(())
}

/// `layout save`: the current arrangement becomes the active layout's content.
/// Windows the state does not know are ignored.
pub fn save_active(state: &mut State, arrangement: &Arrangement) {
    let windows = known(state, arrangement);
    if let Some(layout) = state.layout.get_mut(&state.active_layout) {
        layout.windows = windows;
    }
}

/// A window was moved or resized while shown. Written into the active layout
/// only if it has autosave on; returns whether the state changed.
pub fn window_moved(state: &mut State, id: &WindowId, placement: Placement) -> bool {
    let Some(layout) = state.layout.get_mut(&state.active_layout) else {
        return false;
    };
    if !layout.autosave || !layout.windows.contains_key(id) {
        return false;
    }
    let old = layout.windows.insert(id.clone(), placement);
    old.as_ref() != layout.windows.get(id)
}

/// A new window appeared: it joins the state and the active layout.
pub fn window_opened(state: &mut State, spec: WindowSpec, placement: Placement) -> Result<()> {
    if state.windows.iter().any(|w| w.id == spec.id) {
        return Err(Error::WindowExists(spec.id));
    }
    if let Some(layout) = state.layout.get_mut(&state.active_layout) {
        layout.windows.insert(spec.id.clone(), placement);
    }
    state.windows.push(spec);
    Ok(())
}

/// A window was closed: it leaves the state and all layouts.
/// Returns whether the window was known.
pub fn window_closed(state: &mut State, id: &WindowId) -> bool {
    let before = state.windows.len();
    state.windows.retain(|w| w.id != *id);
    for layout in state.layout.values_mut() {
        layout.windows.remove(id);
    }
    state.windows.len() != before
}

/// The part of `arrangement` that refers to windows of `state`.
fn known(state: &State, arrangement: &Arrangement) -> BTreeMap<WindowId, Placement> {
    arrangement
        .iter()
        .filter(|(id, _)| state.windows.iter().any(|w| w.id == **id))
        .map(|(id, p)| (id.clone(), p.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::layout::Rect;

    /// The concept example: layouts `code` (active) and `review`.
    const EXAMPLE: &str = r#"
active_layout = "code"
windows = [
  { id = "editor",  integration = "editor" },
  { id = "browser", integration = "browser" },
  { id = "term",    integration = "terminal" },
  { id = "spec",    command = "evince spec.pdf" },
]
[layout.code.windows]
editor  = { monitor = "DP-1", x = 0,    y = 0, w = 2660, h = 2160 }
browser = { monitor = "DP-1", x = 2660, y = 0, w = 2460, h = 1240 }
term    = { monitor = "DP-1", x = 2660, y = 1240, w = 2460, h = 920 }
[layout.review]
autosave = false
[layout.review.windows]
browser = { monitor = "DP-1", x = 0,    y = 0, w = 3170, h = 2160 }
editor  = { monitor = "DP-1", x = 3170, y = 0, w = 1950, h = 2160 }
"#;

    fn state() -> State {
        State::from_toml(EXAMPLE, Path::new("x")).unwrap()
    }

    fn l(s: &str) -> LayoutName {
        LayoutName::new(s).unwrap()
    }

    fn w(s: &str) -> WindowId {
        WindowId::new(s).unwrap()
    }

    fn at(x: i32) -> Placement {
        Placement {
            monitor: "DP-1".into(),
            rect: Rect {
                x,
                y: 0,
                w: 10,
                h: 10,
            },
        }
    }

    fn order(st: &State) -> Vec<&str> {
        st.layout.keys().map(LayoutName::as_str).collect()
    }

    #[test]
    fn new_layout_is_appended_active_and_empty() {
        let mut st = state();
        create(&mut st, l("docs"), None).unwrap();
        assert_eq!(order(&st), ["code", "review", "docs"]);
        assert_eq!(st.active_layout, l("docs"));
        assert!(st.layout[&l("docs")].windows.is_empty());
        assert!(st.layout[&l("docs")].autosave);
        assert!(matches!(
            create(&mut st, l("code"), None),
            Err(Error::LayoutExists(_))
        ));
        assert_eq!(st.issues(), []);
    }

    #[test]
    fn new_layout_can_copy_the_arrangement() {
        let mut st = state();
        let live = Arrangement::from([(w("spec"), at(5)), (w("stranger"), at(9))]);
        create(&mut st, l("docs"), Some(&live)).unwrap();
        let docs = &st.layout[&l("docs")].windows;
        assert_eq!(docs.keys().collect::<Vec<_>>(), [&w("spec")]);
        assert_eq!(st.issues(), []);
    }

    #[test]
    fn rename_keeps_position_and_active() {
        let mut st = state();
        rename(&mut st, &l("code"), l("write")).unwrap();
        assert_eq!(order(&st), ["write", "review"]);
        assert_eq!(st.active_layout, l("write"));
        assert!(matches!(
            rename(&mut st, &l("write"), l("review")),
            Err(Error::LayoutExists(_))
        ));
        assert!(matches!(
            rename(&mut st, &l("nope"), l("x")),
            Err(Error::UnknownLayout(_))
        ));
        assert_eq!(st.issues(), []);
    }

    #[test]
    fn active_layout_cannot_be_removed() {
        let mut st = state();
        assert!(matches!(
            remove(&mut st, &l("code")),
            Err(Error::ActiveLayout(_))
        ));
        remove(&mut st, &l("review")).unwrap();
        assert_eq!(order(&st), ["code"]);
        assert!(matches!(
            remove(&mut st, &l("review")),
            Err(Error::UnknownLayout(_))
        ));
    }

    #[test]
    fn autosave_toggles() {
        let mut st = state();
        set_autosave(&mut st, &l("review"), true).unwrap();
        assert!(st.layout[&l("review")].autosave);
        assert!(set_autosave(&mut st, &l("nope"), true).is_err());
    }

    #[test]
    fn next_and_prev_wrap_in_file_order() {
        let mut st = state();
        create(&mut st, l("docs"), None).unwrap();
        activate(&mut st, &l("code")).unwrap();
        assert_eq!(next(&st).unwrap(), l("review"));
        assert_eq!(prev(&st).unwrap(), l("docs"));
        activate(&mut st, &l("docs")).unwrap();
        assert_eq!(next(&st).unwrap(), l("code"));
        let single = State::new(LayoutName::initial());
        assert_eq!(next(&single).unwrap(), LayoutName::initial());
        assert_eq!(prev(&single).unwrap(), LayoutName::initial());
        assert!(activate(&mut st, &l("nope")).is_err());
    }

    #[test]
    fn save_replaces_the_active_layout() {
        let mut st = state();
        save_active(&mut st, &Arrangement::from([(w("spec"), at(1))]));
        assert_eq!(
            st.layout[&l("code")].windows,
            BTreeMap::from([(w("spec"), at(1))])
        );
        // Other layouts are untouched.
        assert_eq!(st.layout[&l("review")].windows.len(), 2);
    }

    #[test]
    fn moves_are_kept_only_with_autosave() {
        let mut st = state();
        assert!(window_moved(&mut st, &w("editor"), at(7)));
        assert_eq!(st.layout[&l("code")].windows[&w("editor")], at(7));
        // Same placement again: nothing changes.
        assert!(!window_moved(&mut st, &w("editor"), at(7)));
        // A stashed window is not part of the active layout.
        assert!(!window_moved(&mut st, &w("spec"), at(7)));
        activate(&mut st, &l("review")).unwrap();
        let before = st.layout[&l("review")].clone();
        assert!(!window_moved(&mut st, &w("editor"), at(9)));
        assert_eq!(st.layout[&l("review")], before);
    }

    #[test]
    fn new_window_joins_the_active_layout() {
        let mut st = state();
        activate(&mut st, &l("review")).unwrap();
        let spec: WindowSpec = toml::from_str("id = \"chat\"\ncommand = \"chat\"\n").unwrap();
        window_opened(&mut st, spec.clone(), at(3)).unwrap();
        assert_eq!(st.layout[&l("review")].windows[&w("chat")], at(3));
        assert!(!st.layout[&l("code")].windows.contains_key(&w("chat")));
        assert!(matches!(
            window_opened(&mut st, spec, at(3)),
            Err(Error::WindowExists(_))
        ));
        assert_eq!(st.issues(), []);
    }

    #[test]
    fn closed_window_leaves_all_layouts() {
        let mut st = state();
        assert!(window_closed(&mut st, &w("editor")));
        assert!(st.windows.iter().all(|x| x.id != w("editor")));
        for layout in st.layout.values() {
            assert!(!layout.windows.contains_key(&w("editor")));
        }
        assert!(!window_closed(&mut st, &w("editor")));
        assert_eq!(st.issues(), []);
    }
}
