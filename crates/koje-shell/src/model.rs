//! What the shell shows, without GTK (ADR 0042: decisions stay pure). The
//! backend turns kojed's and Hyprland's answers into [`Update`]s, [`Shell`]
//! keeps the latest of each, [`Shell::bar`] says what a monitor's bar shows
//! and [`Action`]s say what clicks do.

use koje_core::color::Color;
use koje_core::config::Limits;
use koje_core::memory::{MemoryLevel, classify};
use koje_core::ports::CompositorCommand;
use koje_core::size::ByteSize;

/// Something the shell learned from kojed or Hyprland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Monitors, workspaces and windows (Hyprland).
    Desktop(Desktop),
    /// The running contexts (kojed's `RunningContexts`).
    Contexts(Vec<RunningContext>),
    /// The active context's memory (kojed's `Memory("")`).
    Memory(MemoryReading),
    /// kojed does not own its bus name (not started, crashed, restarting).
    KojedGone,
}

/// kojed's `Memory`: the working set, what the cgroup charges and the limits
/// of a context; `context` is empty without an active context or before the
/// first reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryReading {
    pub context: String,
    pub used: u64,
    pub charged: u64,
    /// 0 = none.
    pub soft: u64,
    /// 0 = none.
    pub hard: u64,
}

impl From<(String, u64, u64, u64, u64)> for MemoryReading {
    fn from((context, used, charged, soft, hard): (String, u64, u64, u64, u64)) -> Self {
        Self {
            context,
            used,
            charged,
            soft,
            hard,
        }
    }
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

/// Hyprland's monitors, workspaces and windows, as read at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktop {
    pub monitors: Vec<MonitorState>,
    pub workspaces: Vec<WorkspaceState>,
    /// Mapped, visible windows in Hyprland's order.
    pub windows: Vec<Window>,
    /// The focused window's address.
    pub focused: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorState {
    /// The connector name, e.g. `eDP-1` (GDK's `Monitor::connector`).
    pub name: String,
    pub active_workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceState {
    /// Positive for numbered workspaces; named ones get negative ids.
    pub id: i64,
    pub name: String,
    pub monitor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// `0x…`.
    pub address: String,
    pub workspace: String,
    /// The app id; the icon comes from it.
    pub class: String,
    pub title: String,
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
/// (ADR 0030); this is only their latest answers, plus the order in which
/// windows appeared.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shell {
    desktop: Desktop,
    kojed: Kojed,
    /// Window addresses in the order the shell first saw them. Hyprland
    /// lists windows in stacking order, which every raise changes; the task
    /// list must not jump when a window gets focus.
    order: Vec<String>,
    memory: MemoryReading,
}

/// Whose color an element shows as its accent: a context's, or the grey of
/// host workspaces (M8.20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accent {
    Host,
    Context(String),
}

impl Accent {
    /// The CSS class that sets libadwaita's accent variables ([`accent_css`]).
    pub fn css_class(&self) -> String {
        match self {
            Self::Host => "koje-host".to_owned(),
            // Context names are a-z, 0-9 and '-': valid in a class name.
            Self::Context(name) => format!("koje-ctx-{name}"),
        }
    }
}

/// One button of the workspace list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceButton {
    pub name: String,
    pub active: bool,
    pub accent: Accent,
}

/// One window of the task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub address: String,
    pub class: String,
    pub title: String,
    pub focused: bool,
}

/// What one bar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarView {
    /// The monitor's workspaces, numbered ones first.
    pub workspaces: Vec<WorkspaceButton>,
    /// The windows of the monitor's active workspace.
    pub tasks: Vec<Task>,
    /// The accent of the task list: its workspace's.
    pub tasks_accent: Accent,
    /// The active context: a capsule in its color with its memory.
    pub capsule: Option<Capsule>,
    /// A problem worth showing in the bar.
    pub warning: Option<&'static str>,
}

/// The active context's capsule (M8.29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capsule {
    pub name: String,
    pub accent: Accent,
    /// `None` until kojed read the context's memory.
    pub memory: Option<MemoryView>,
}

/// The memory meter: usage against the hard limit, else the soft one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryView {
    /// The used share of the meter in thousandths; `None` without limits
    /// (no meter, only the size).
    pub fill: Option<u16>,
    /// Where the soft limit lies on the meter, in thousandths – only with a
    /// hard limit too.
    pub soft: Option<u16>,
    pub level: MemoryLevel,
    /// `42%`, or the size without limits.
    pub label: String,
    /// Used, charged, the limits ([`koje_core::memory::details`]).
    pub tooltip: String,
}

impl MemoryView {
    pub fn from_reading(m: &MemoryReading) -> Self {
        let limit = |b: u64| (b > 0).then_some(ByteSize(b));
        let limits = Limits {
            memory_soft: limit(m.soft),
            memory_hard: limit(m.hard),
        };
        let used = u128::from(m.used);
        let max = limits
            .memory_hard
            .or(limits.memory_soft)
            .map(|l| u128::from(l.0.max(1)));
        let thousandths =
            |v: u128, max: u128| u16::try_from((v * 1000 / max).min(1000)).unwrap_or(1000);
        Self {
            fill: max.map(|max| thousandths(used, max)),
            soft: limits
                .memory_hard
                .and(limits.memory_soft)
                .zip(max)
                .map(|(s, max)| thousandths(u128::from(s.0), max)),
            level: classify(ByteSize(m.used), &limits),
            label: max.map_or_else(
                || ByteSize(m.used).human(),
                |max| format!("{}%", (used * 100 + max / 2) / max),
            ),
            tooltip: koje_core::memory::details(
                &m.context,
                ByteSize(m.used),
                ByteSize(m.charged),
                &limits,
            ),
        }
    }
}

pub const KOJED_GONE: &str = "kojed is not running";

/// What a click asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    FocusWorkspace(String),
    /// Focus and raise a window.
    FocusWindow(String),
}

impl Action {
    pub fn command(self) -> CompositorCommand {
        match self {
            Self::FocusWorkspace(workspace) => CompositorCommand::FocusWorkspace { workspace },
            Self::FocusWindow(address) => CompositorCommand::FocusWindow { address },
        }
    }
}

impl Shell {
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Desktop(desktop) => {
                self.order
                    .retain(|a| desktop.windows.iter().any(|w| &w.address == a));
                for w in &desktop.windows {
                    if !self.order.contains(&w.address) {
                        self.order.push(w.address.clone());
                    }
                }
                self.desktop = desktop;
            }
            Update::Contexts(contexts) => self.kojed = Kojed::Running(contexts),
            Update::Memory(reading) => self.memory = reading,
            Update::KojedGone => {
                self.kojed = Kojed::Gone;
                self.memory = MemoryReading::default();
            }
        }
    }

    fn contexts(&self) -> &[RunningContext] {
        match &self.kojed {
            Kojed::Running(contexts) => contexts,
            Kojed::Unknown | Kojed::Gone => &[],
        }
    }

    /// The accent of a workspace: its context's while that runs.
    fn accent(&self, workspace: &str) -> Accent {
        if self.contexts().iter().any(|c| c.name == workspace) {
            Accent::Context(workspace.to_owned())
        } else {
            Accent::Host
        }
    }

    /// The bar of the monitor with connector `monitor`; the first monitor
    /// when GDK does not know the connector.
    pub fn bar(&self, monitor: Option<&str>) -> BarView {
        let d = &self.desktop;
        let mon = monitor
            .and_then(|name| d.monitors.iter().find(|m| m.name == name))
            .or_else(|| d.monitors.first());
        let active = mon.map(|m| m.active_workspace.as_str());

        let mut workspaces: Vec<&WorkspaceState> = d
            .workspaces
            .iter()
            .filter(|w| !w.name.starts_with("special:"))
            .filter(|w| mon.is_none_or(|m| w.monitor == m.name))
            .collect();
        workspaces.sort_by(|a, b| {
            let key = |w: &WorkspaceState| (w.id <= 0, if w.id > 0 { w.id } else { 0 });
            key(a).cmp(&key(b)).then_with(|| a.name.cmp(&b.name))
        });

        let mut windows: Vec<&Window> = d
            .windows
            .iter()
            .filter(|w| Some(w.workspace.as_str()) == active)
            .collect();
        windows.sort_by_key(|w| self.order.iter().position(|a| a == &w.address));
        let tasks = windows
            .into_iter()
            .map(|w| Task {
                address: w.address.clone(),
                class: w.class.clone(),
                title: w.title.clone(),
                focused: d.focused.as_ref() == Some(&w.address),
            })
            .collect();

        BarView {
            workspaces: workspaces
                .into_iter()
                .map(|w| WorkspaceButton {
                    name: w.name.clone(),
                    active: Some(w.name.as_str()) == active,
                    accent: self.accent(&w.name),
                })
                .collect(),
            tasks,
            tasks_accent: active.map_or(Accent::Host, |a| self.accent(a)),
            capsule: self.contexts().iter().find(|c| c.active).map(|c| Capsule {
                name: c.name.clone(),
                accent: Accent::Context(c.name.clone()),
                // A reading of another context is from before a switch.
                memory: (self.memory.context == c.name)
                    .then(|| MemoryView::from_reading(&self.memory)),
            }),
            warning: (self.kojed == Kojed::Gone).then_some(KOJED_GONE),
        }
    }

    /// The accent classes for the running contexts and the host.
    pub fn accent_css(&self) -> String {
        accent_css(self.contexts())
    }
}

/// libadwaita's slate accent: host workspaces (M8.20 chose grey there).
const HOST_ACCENT: Color = Color::rgb(0x6f, 0x83, 0x96);

/// CSS that makes each context's color the accent of the elements carrying
/// its [`Accent::css_class`] (libadwaita's accent variables, ADR 0043).
pub fn accent_css(contexts: &[RunningContext]) -> String {
    let mut css = rule(&Accent::Host, HOST_ACCENT);
    for c in contexts {
        if let Ok(color) = c.color.parse::<Color>() {
            css.push_str(&rule(&Accent::Context(c.name.clone()), color));
        }
    }
    css
}

fn rule(accent: &Accent, color: Color) -> String {
    let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    format!(
        ".{} {{ --accent-bg-color: {hex}; --accent-color: {hex}; --accent-fg-color: {}; }}\n",
        accent.css_class(),
        foreground(color)
    )
}

/// Text on a color: white unless the color is light.
fn foreground(c: Color) -> &'static str {
    // Relative luminance (sRGB, linearized).
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
    if l > 0.4 {
        "rgb(0 0 6 / 80%)"
    } else {
        "#ffffff"
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

    fn ws(id: i64, name: &str, monitor: &str) -> WorkspaceState {
        WorkspaceState {
            id,
            name: name.into(),
            monitor: monitor.into(),
        }
    }

    fn win(address: &str, workspace: &str, title: &str) -> Window {
        Window {
            address: address.into(),
            workspace: workspace.into(),
            class: "foot".into(),
            title: title.into(),
        }
    }

    /// Two monitors: eDP-1 shows `kp`, DP-1 shows `3`.
    fn desktop() -> Desktop {
        Desktop {
            monitors: vec![
                MonitorState {
                    name: "eDP-1".into(),
                    active_workspace: "kp".into(),
                },
                MonitorState {
                    name: "DP-1".into(),
                    active_workspace: "3".into(),
                },
            ],
            workspaces: vec![
                ws(-1337, "kp", "eDP-1"),
                ws(2, "2", "eDP-1"),
                ws(-1338, "admin", "eDP-1"),
                ws(1, "1", "eDP-1"),
                ws(-98, "special:koje-stash-kp", "eDP-1"),
                ws(3, "3", "DP-1"),
            ],
            windows: vec![
                win("0xa", "kp", "editor"),
                win("0xb", "1", "host"),
                win("0xc", "kp", "browser"),
                win("0xd", "3", "other monitor"),
            ],
            focused: Some("0xc".into()),
        }
    }

    fn shell() -> Shell {
        let mut s = Shell::default();
        s.apply(Update::Desktop(desktop()));
        s.apply(Update::Contexts(vec![ctx("kp", true)]));
        s
    }

    #[test]
    fn starts_empty_and_without_warning() {
        let bar = Shell::default().bar(None);
        assert_eq!((bar.workspaces.len(), bar.tasks.len()), (0, 0));
        assert_eq!(bar.warning, None);
        assert_eq!(bar.tasks_accent, Accent::Host);
    }

    #[test]
    fn workspaces_of_the_monitor_numbered_first_without_special() {
        let bar = shell().bar(Some("eDP-1"));
        let names: Vec<_> = bar.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["1", "2", "admin", "kp"]);
        let kp = &bar.workspaces[3];
        assert!(kp.active);
        assert_eq!(kp.accent, Accent::Context("kp".into()));
        // `admin` is no running context: grey.
        assert_eq!(bar.workspaces[2].accent, Accent::Host);
    }

    #[test]
    fn tasks_are_the_active_workspaces_windows() {
        let bar = shell().bar(Some("eDP-1"));
        let tasks: Vec<_> = bar
            .tasks
            .iter()
            .map(|t| (t.title.as_str(), t.focused))
            .collect();
        assert_eq!(tasks, [("editor", false), ("browser", true)]);
        assert_eq!(bar.tasks_accent, Accent::Context("kp".into()));
    }

    #[test]
    fn raising_a_window_keeps_the_task_order() {
        let mut s = shell();
        // Focus and raise `editor`: Hyprland now lists it last.
        let mut d = desktop();
        let editor = d.windows.remove(0);
        d.windows.push(editor);
        d.focused = Some("0xa".into());
        s.apply(Update::Desktop(d.clone()));
        let titles = |s: &Shell| -> Vec<String> {
            s.bar(Some("eDP-1"))
                .tasks
                .into_iter()
                .map(|t| t.title)
                .collect()
        };
        assert_eq!(titles(&s), ["editor", "browser"]);

        // A new window goes last; a closed one is forgotten.
        d.windows.insert(0, win("0xe", "kp", "new"));
        d.windows.retain(|w| w.address != "0xc");
        s.apply(Update::Desktop(d));
        assert_eq!(titles(&s), ["editor", "new"]);
    }

    #[test]
    fn each_monitor_gets_its_own_bar() {
        let bar = shell().bar(Some("DP-1"));
        let names: Vec<_> = bar.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["3"]);
        assert_eq!(bar.tasks.len(), 1);
        assert_eq!(bar.tasks_accent, Accent::Host);
    }

    #[test]
    fn unknown_connector_shows_the_first_monitor() {
        assert_eq!(shell().bar(None), shell().bar(Some("eDP-1")));
    }

    fn reading(context: &str, used: u64, soft: u64, hard: u64) -> MemoryReading {
        MemoryReading {
            context: context.into(),
            used,
            charged: used + (100 << 20),
            soft,
            hard,
        }
    }

    const GIB: u64 = 1 << 30;

    #[test]
    fn shows_the_active_context_with_its_memory() {
        let mut s = shell();
        let capsule = s.bar(None).capsule.unwrap();
        assert_eq!(capsule.name, "kp");
        assert_eq!(capsule.accent, Accent::Context("kp".into()));
        assert_eq!(capsule.memory, None, "no reading yet");

        s.apply(Update::Memory(reading("kp", 3 * GIB, 6 * GIB, 8 * GIB)));
        let m = s.bar(None).capsule.unwrap().memory.unwrap();
        assert_eq!(m.fill, Some(375));
        assert_eq!(m.soft, Some(750));
        assert_eq!(m.level, MemoryLevel::Normal);
        assert_eq!(m.label, "38%");
        assert!(
            m.tooltip.starts_with("kp: memory\nused 3.0 GiB"),
            "{}",
            m.tooltip
        );
        assert_eq!(s.bar(None).warning, None);
    }

    #[test]
    fn a_reading_of_another_context_is_not_shown() {
        let mut s = shell();
        s.apply(Update::Memory(reading("privat", GIB, 0, 0)));
        assert_eq!(s.bar(None).capsule.unwrap().memory, None);
    }

    #[test]
    fn memory_levels_limits_and_none() {
        let view = |used, soft, hard| MemoryView::from_reading(&reading("kp", used, soft, hard));
        // Only a soft limit: the meter goes up to it, no mark.
        let soft_only = view(5 * GIB + GIB / 2, 6 * GIB, 0);
        assert_eq!((soft_only.fill, soft_only.soft), (Some(916), None));
        assert_eq!(soft_only.level, MemoryLevel::NearSoft);
        let over = view(7 * GIB, 6 * GIB, 8 * GIB);
        assert_eq!(over.level, MemoryLevel::OverSoft);
        let full = view(9 * GIB, 6 * GIB, 8 * GIB);
        assert_eq!((full.fill, full.level), (Some(1000), MemoryLevel::OverHard));
        assert_eq!(full.label, "113%");
        // No limits: no meter, the size.
        let free = view(3 * GIB, 0, 0);
        assert_eq!((free.fill, free.label.as_str()), (None, "3.0 GiB"));
    }

    #[test]
    fn warns_while_kojed_is_gone_and_forgets_its_contexts() {
        let mut s = shell();
        s.apply(Update::KojedGone);
        let bar = s.bar(None);
        assert_eq!(bar.capsule, None);
        assert_eq!(bar.warning, Some(KOJED_GONE));
        // Without kojed no context colors: all grey.
        assert_eq!(bar.tasks_accent, Accent::Host);
        s.apply(Update::Contexts(vec![]));
        assert_eq!(s.bar(None).warning, None);
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
    fn accent_css_per_context_with_readable_text() {
        let css = accent_css(&[
            RunningContext {
                name: "kp".into(),
                color: "#1c71d8".into(),
                active: false,
            },
            RunningContext {
                name: "light".into(),
                color: "#f6d32d".into(),
                active: false,
            },
            RunningContext {
                name: "bad".into(),
                color: "blue".into(),
                active: false,
            },
        ]);
        assert!(
            css.contains(".koje-host { --accent-bg-color: #6f8396;"),
            "{css}"
        );
        assert!(
            css.contains(".koje-ctx-kp { --accent-bg-color: #1c71d8; --accent-color: #1c71d8; --accent-fg-color: #ffffff; }"),
            "{css}"
        );
        assert!(css.contains(".koje-ctx-light { --accent-bg-color: #f6d32d; --accent-color: #f6d32d; --accent-fg-color: rgb(0 0 6 / 80%); }"), "{css}");
        assert!(!css.contains("koje-ctx-bad"), "{css}");
    }

    #[test]
    fn clicks_focus_workspaces_and_windows() {
        assert_eq!(
            Action::FocusWorkspace("kp".into()).command(),
            CompositorCommand::FocusWorkspace {
                workspace: "kp".into()
            }
        );
        assert_eq!(
            Action::FocusWindow("0xa".into()).command(),
            CompositorCommand::FocusWindow {
                address: "0xa".into()
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
