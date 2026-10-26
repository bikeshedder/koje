//! What the shell shows, without GTK (ADR 0042: decisions stay pure). The
//! backend turns kojed's and Hyprland's answers into [`Update`]s, [`Shell`]
//! keeps the latest of each, and [`Shell::bar`] says what every bar shows.

/// Something the shell learned from kojed or Hyprland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// The focused workspace (Hyprland).
    Workspace(String),
    /// The running contexts (kojed's `RunningContexts`).
    Contexts(Vec<RunningContext>),
    /// kojed does not own its bus name (not started, crashed, restarting).
    KojedGone,
}

/// One entry of kojed's `RunningContexts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningContext {
    pub name: String,
    /// `#RRGGBB`.
    pub color: String,
    pub active: bool,
}

impl From<(String, String, u32, bool)> for RunningContext {
    fn from((name, color, _windows, active): (String, String, u32, bool)) -> Self {
        Self {
            name,
            color,
            active,
        }
    }
}

/// What kojed told the shell last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Kojed {
    /// Not asked yet: the bar does not warn while the shell starts.
    #[default]
    Unknown,
    Gone,
    Running(Vec<RunningContext>),
}

/// The shell's view of the desktop. kojed and Hyprland hold the real state
/// (ADR 0030); this is only their latest answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shell {
    workspace: Option<String>,
    kojed: Kojed,
}

/// What one bar shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarView {
    /// The focused workspace's name; empty until Hyprland answered.
    pub workspace: String,
    /// The active context and its color.
    pub context: Option<(String, String)>,
    /// A problem worth showing in the bar.
    pub warning: Option<&'static str>,
}

pub const KOJED_GONE: &str = "kojed is not running";

impl Shell {
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Workspace(name) => self.workspace = Some(name),
            Update::Contexts(contexts) => self.kojed = Kojed::Running(contexts),
            Update::KojedGone => self.kojed = Kojed::Gone,
        }
    }

    pub fn bar(&self) -> BarView {
        let context = match &self.kojed {
            Kojed::Running(contexts) => contexts
                .iter()
                .find(|c| c.active)
                .map(|c| (c.name.clone(), c.color.clone())),
            Kojed::Unknown | Kojed::Gone => None,
        };
        BarView {
            workspace: self.workspace.clone().unwrap_or_default(),
            context,
            warning: (self.kojed == Kojed::Gone).then_some(KOJED_GONE),
        }
    }
}

/// Which bars to remove (indexes into `have`, descending so they can be
/// removed in order) and which monitors need a new one, when the monitors
/// change from those in `have` to `want`.
pub fn reconcile<'w, M: PartialEq>(have: &[M], want: &'w [M]) -> (Vec<usize>, Vec<&'w M>) {
    let remove = (0..have.len())
        .rev()
        .filter(|&i| !want.contains(&have[i]))
        .collect();
    let add = want.iter().filter(|m| !have.contains(m)).collect();
    (remove, add)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str, active: bool) -> RunningContext {
        RunningContext {
            name: name.into(),
            color: "#3584e4".into(),
            active,
        }
    }

    #[test]
    fn starts_empty_and_without_warning() {
        assert_eq!(Shell::default().bar(), BarView::default());
    }

    #[test]
    fn shows_the_workspace_and_the_active_context() {
        let mut s = Shell::default();
        s.apply(Update::Workspace("kundenportal".into()));
        s.apply(Update::Contexts(vec![
            ctx("privat", false),
            ctx("kundenportal", true),
        ]));
        let bar = s.bar();
        assert_eq!(bar.workspace, "kundenportal");
        assert_eq!(bar.context, Some(("kundenportal".into(), "#3584e4".into())));
        assert_eq!(bar.warning, None);
    }

    #[test]
    fn no_context_when_none_is_active() {
        let mut s = Shell::default();
        s.apply(Update::Contexts(vec![ctx("privat", false)]));
        assert_eq!(s.bar().context, None);
    }

    #[test]
    fn warns_while_kojed_is_gone_and_forgets_its_contexts() {
        let mut s = Shell::default();
        s.apply(Update::Contexts(vec![ctx("privat", true)]));
        s.apply(Update::KojedGone);
        assert_eq!(s.bar().context, None);
        assert_eq!(s.bar().warning, Some(KOJED_GONE));
        s.apply(Update::Contexts(vec![]));
        assert_eq!(s.bar().warning, None);
    }

    #[test]
    fn running_context_from_kojeds_tuple() {
        let c = RunningContext::from(("privat".to_owned(), "#ff0000".to_owned(), 3, true));
        assert_eq!(
            c,
            RunningContext {
                name: "privat".into(),
                color: "#ff0000".into(),
                active: true
            }
        );
    }

    #[test]
    fn reconcile_adds_new_and_removes_gone_monitors() {
        let (remove, add) = reconcile(&["DP-1", "HDMI-A-1", "eDP-1"], &["eDP-1", "DP-2"]);
        assert_eq!(remove, [1, 0]);
        assert_eq!(add, [&"DP-2"]);
    }

    #[test]
    fn reconcile_without_changes() {
        let (remove, add) = reconcile(&["eDP-1"], &["eDP-1"]);
        assert_eq!((remove.len(), add.len()), (0, 0));
    }
}
