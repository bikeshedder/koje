//! What the shell shows, without GTK (ADR 0042: decisions stay pure). The
//! backend turns kojed's and Hyprland's answers into [`Update`]s, [`Shell`]
//! keeps the latest of each, [`Shell::bar`] says what a monitor's bar shows
//! and [`Action`]s say what clicks do.

use koje_core::color::Color;
use koje_core::config::{BarConfig, WorkspaceStyle};
use koje_core::ports::CompositorCommand;

use std::collections::BTreeMap;

use crate::dash::{DashInput, dash};
use crate::menu::{HistoryReading, MenuInput, MenuView, NEW_CONTEXT, menu};
use crate::overview::{OverviewView, Stash, WindowCard, on_workspace};
use crate::quick::{QuickView, System, quick};
use crate::search::{AppItem, AppSection, SearchView, grid, search};
use crate::system::Change;

/// Something the shell learned from kojed or Hyprland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Monitors and workspaces (Hyprland).
    Desktop(Desktop),
    /// The running contexts (kojed's `RunningContexts`).
    Contexts(Vec<RunningContext>),
    /// Every context with its status (kojed's `Contexts`).
    AllContexts(Vec<ContextEntry>),
    /// The memory of every running context (kojed's `Memory`).
    Memory(Vec<MemoryReading>),
    /// The active context's memory over the last minutes.
    History(HistoryReading),
    /// A command for a context started (`koje up`, `down`, …).
    Busy { context: String, busy: Busy },
    /// It ended: its error, if any. Creating a new context reports under
    /// [`NEW_CONTEXT`].
    Done {
        context: String,
        error: Option<String>,
    },
    /// kojed does not own its bus name (not started, crashed, restarting).
    KojedGone,
    /// Sound, network, battery, … for Quick Settings.
    System(System),
    /// `config.toml` changed.
    Bar(BarConfig),
    /// GNOME's favorite apps (`org.gnome.shell favorite-apps`).
    Favorites(Vec<String>),
    /// A running context's programs (`koje apps --json`).
    Apps { context: String, apps: Vec<AppItem> },
    /// The host's programs.
    HostApps(Vec<AppItem>),
    /// A context's own favorites for the dash (ADR 0053); `None`: unset.
    ContextFavorites {
        context: String,
        favorites: Option<Vec<String>>,
    },
    /// GNOME Settings changed the screens: ask to keep them within that many
    /// seconds (M8.53). Not part of the bar's state.
    DisplayChangePending(u32),
    /// koje's plugin flew the windows into the overview on this monitor.
    /// Not part of the state.
    OverviewLanded(String),
    /// A window's picture (the screen sharing picker). Not part of the state.
    Thumbnail {
        address: String,
        thumb: crate::capture::Thumbnail,
    },
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

/// What a context is doing (kojed's `Contexts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Running,
    Paused,
    Stopped,
}

/// One entry of kojed's `Contexts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEntry {
    pub name: String,
    /// `#RRGGBB`.
    pub color: String,
    pub status: Status,
}

impl From<(String, String, String)> for ContextEntry {
    fn from((name, color, status): (String, String, String)) -> Self {
        let status = match status.as_str() {
            "running" => Status::Running,
            "paused" => Status::Paused,
            _ => Status::Stopped,
        };
        Self {
            name,
            color,
            status,
        }
    }
}

/// A command the shell runs for a context, while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Starting,
    Stopping,
    Restarting,
    Pausing,
    Waking,
    Creating,
    ChangingLimits,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "Starting…",
            Self::Stopping => "Stopping…",
            Self::Restarting => "Restarting…",
            Self::Pausing => "Pausing…",
            Self::Waking => "Waking…",
            Self::Creating => "Creating…",
            Self::ChangingLimits => "Changing limits…",
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
    pub windows: Vec<WindowState>,
    /// koje's Hyprland plugin is loaded: it draws the overview's windows
    /// (ADR 0051).
    pub plugin: bool,
}

/// A mapped window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    /// Hyprland's address, `0x…`.
    pub address: String,
    /// The app id (Wayland) or class (X11).
    pub class: String,
    pub title: String,
    /// Its workspace's name; `special:stash-<context>` when stashed.
    pub workspace: String,
    pub at: (i32, i32),
    pub size: (i32, i32),
    /// 0 for the focused window, then by when they had the focus.
    pub focus_history: i32,
    /// The context kojed tagged it with; `None` for host windows.
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorState {
    /// The connector name, e.g. `eDP-1` (GDK's `Monitor::connector`).
    pub name: String,
    pub active_workspace: String,
    /// It has the keyboard focus: new workspaces open there.
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceState {
    /// Positive for numbered workspaces; named ones get negative ids.
    pub id: i64,
    pub name: String,
    pub monitor: String,
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
    desktop: Desktop,
    kojed: Kojed,
    memory: Vec<MemoryReading>,
    history: HistoryReading,
    /// Every context with its status; empty from a kojed without `Contexts`.
    all: Vec<ContextEntry>,
    /// Commands running per context.
    busy: BTreeMap<String, Busy>,
    /// The last failed command per context.
    errors: BTreeMap<String, String>,
    system: System,
    /// `[bar]` of `config.toml`.
    bar: BarConfig,
    favorites: Vec<String>,
    /// The programs of each context that ran when they were read.
    apps: BTreeMap<String, Vec<AppItem>>,
    /// Each context's own favorites (`context.toml`), `None` when unset.
    context_favorites: BTreeMap<String, Option<Vec<String>>>,
    host_apps: Vec<AppItem>,
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
    /// A running context whose workspace Hyprland dropped: it has no
    /// windows (or they are stashed while it sleeps). A click opens it.
    pub empty: bool,
}

/// Lookups in the host's desktop files (GIO, outside the pure model).
pub struct HostApps<'a> {
    /// The desktop file id of an app id.
    pub desktop_id: &'a dyn Fn(&str) -> Option<String>,
    /// A desktop file's name.
    pub name: &'a dyn Fn(&str) -> Option<String>,
}

/// A workspace shortcut, as GNOME's (Super+1…9, Super+Page Up/Down,
/// Ctrl+Alt+Left/Right): through the bar's workspaces in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceStep {
    /// The n-th, from 1.
    Nth(usize),
    Next,
    Prev,
}

impl WorkspaceStep {
    /// `3`, `next`, `prev`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "next" => Some(Self::Next),
            "prev" => Some(Self::Prev),
            n => n.parse().ok().filter(|&n| n > 0).map(Self::Nth),
        }
    }
}

/// What one bar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarView {
    /// The monitor's workspaces, numbered ones first.
    pub workspaces: Vec<WorkspaceButton>,
    /// How to show them (ADR 0047).
    pub style: WorkspaceStyle,
    /// The koje menu; `None` while kojed does not answer.
    pub menu: Option<MenuView>,
    pub quick: QuickView,
    /// A problem worth showing in the bar.
    pub warning: Option<&'static str>,
}

pub const KOJED_GONE: &str = "kojed is not running";

/// What a click asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    FocusWorkspace(String),
    /// Focus and raise a window (the overview).
    FocusWindow(String),
    /// Ask a window to close.
    CloseWindow(String),
    /// Tell koje's plugin where the overview's windows go
    /// ([`crate::overview::plugin_command`]).
    Overview(String),
    /// Start a program in a context by its desktop file id (`koje apps
    /// --start`; the dash).
    Launch {
        context: String,
        id: String,
    },
    /// Bring a stashed window back into its context's active layout
    /// (kojed's `Unstash`).
    Unstash {
        context: String,
        address: String,
    },
    /// Show a context's workspace (kojed's `Focus`).
    FocusContext(String),
    /// Move a window to a workspace, staying where one is (a workspace
    /// shortcut with Shift: the focus follows with `FocusWorkspace`).
    MoveWindow {
        address: String,
        workspace: String,
    },
    /// Read these contexts' programs and favorites again (`koje apps
    /// --json`, `context.toml`).
    RefreshApps(Vec<String>),
    /// Set a context's favorites for the dash (`koje favorites --set`).
    SetFavorites {
        context: String,
        ids: Vec<String>,
    },
    /// Change a context's limits with `koje limit`: sizes as it takes them,
    /// `none` to remove one ([`crate::menu::limit_arg`]).
    SetLimits {
        context: String,
        soft: String,
        hard: String,
    },
    Start(String),
    Stop(String),
    Restart(String),
    Pause(String),
    Wake(String),
    /// `koje new` with a checked name ([`crate::menu::context_name`]) and
    /// a color, else its default; then `koje up`.
    Create {
        name: String,
        color: Option<String>,
    },
    /// A change in Quick Settings.
    Quick(Change),
    /// "Keep Changes" / "Revert Settings" after a display change.
    KeepDisplayChange,
    RevertDisplayChange,
    /// Log out, restart or power off – asked first (M8.47).
    EndSession(crate::end_session::EndSession),
    Suspend,
}

impl Action {
    /// The Hyprland command of a click, if it is one.
    pub fn command(&self) -> Option<CompositorCommand> {
        match self {
            Self::FocusWorkspace(workspace) => Some(CompositorCommand::FocusWorkspace {
                workspace: workspace.clone(),
            }),
            Self::FocusWindow(address) => Some(CompositorCommand::FocusWindow {
                address: address.clone(),
            }),
            Self::CloseWindow(address) => Some(CompositorCommand::CloseWindow {
                address: address.clone(),
            }),
            Self::MoveWindow { address, workspace } => Some(CompositorCommand::MoveToWorkspace {
                address: address.clone(),
                workspace: workspace.clone(),
            }),
            _ => None,
        }
    }

    /// The context it is for, what it is doing meanwhile and the `koje`
    /// commands it runs, in order – `None` for clicks that are no command.
    pub fn koje(&self) -> Option<(String, Busy, Vec<Vec<String>>)> {
        let cmd = |args: &[&str]| args.iter().map(|&a| a.to_owned()).collect::<Vec<_>>();
        Some(match self {
            Self::SetLimits {
                context,
                soft,
                hard,
            } => (
                context.clone(),
                Busy::ChangingLimits,
                vec![cmd(&["limit", context, "--soft", soft, "--hard", hard])],
            ),
            Self::Start(c) => (c.clone(), Busy::Starting, vec![cmd(&["up", c])]),
            Self::Stop(c) => (c.clone(), Busy::Stopping, vec![cmd(&["down", c])]),
            Self::Restart(c) => (
                c.clone(),
                Busy::Restarting,
                vec![cmd(&["down", c]), cmd(&["up", c])],
            ),
            Self::Pause(c) => (c.clone(), Busy::Pausing, vec![cmd(&["sleep", c])]),
            Self::Wake(c) => (c.clone(), Busy::Waking, vec![cmd(&["wake", c])]),
            Self::Create { name, color } => {
                let mut new = cmd(&["new", name]);
                if let Some(color) = color {
                    new.extend(cmd(&["--color", color]));
                }
                (
                    NEW_CONTEXT.to_owned(),
                    Busy::Creating,
                    vec![new, cmd(&["up", name])],
                )
            }
            Self::FocusWorkspace(_)
            | Self::FocusWindow(_)
            | Self::CloseWindow(_)
            | Self::MoveWindow { .. }
            | Self::Unstash { .. }
            | Self::Launch { .. }
            | Self::RefreshApps(_)
            | Self::SetFavorites { .. }
            | Self::Overview(_)
            | Self::FocusContext(_)
            | Self::Quick(_)
            | Self::KeepDisplayChange
            | Self::RevertDisplayChange
            | Self::EndSession(_)
            | Self::Suspend => return None,
        })
    }
}

impl Shell {
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Desktop(desktop) => self.desktop = desktop,
            Update::Bar(bar) => self.bar = bar,
            Update::Favorites(favorites) => self.favorites = favorites,
            Update::Apps { context, apps } => {
                self.apps.insert(context, apps);
            }
            Update::HostApps(apps) => self.host_apps = apps,
            Update::ContextFavorites { context, favorites } => {
                self.context_favorites.insert(context, favorites);
            }
            Update::DisplayChangePending(_)
            | Update::OverviewLanded(_)
            | Update::Thumbnail { .. } => {}
            Update::Contexts(contexts) => self.kojed = Kojed::Running(contexts),
            Update::Memory(readings) => self.memory = readings,
            Update::History(history) => self.history = history,
            Update::AllContexts(all) => self.all = all,
            Update::Busy { context, busy } => {
                self.errors.remove(&context);
                self.busy.insert(context, busy);
            }
            Update::Done { context, error } => {
                self.busy.remove(&context);
                match error {
                    Some(e) => self.errors.insert(context, e),
                    None => self.errors.remove(&context),
                };
            }
            Update::System(system) => self.system = system,
            Update::KojedGone => {
                self.kojed = Kojed::Gone;
                self.memory.clear();
                self.history = HistoryReading::default();
                self.all.clear();
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

    /// The monitor with connector `monitor`; the first one when GDK does
    /// not know the connector.
    fn monitor(&self, monitor: Option<&str>) -> Option<&MonitorState> {
        let d = &self.desktop;
        monitor
            .and_then(|name| d.monitors.iter().find(|m| m.name == name))
            .or_else(|| d.monitors.first())
    }

    /// A monitor's workspaces, numbered ones first, with the running
    /// contexts Hyprland has no workspace for on the focused monitor.
    fn workspace_buttons(&self, mon: Option<&MonitorState>) -> Vec<WorkspaceButton> {
        let d = &self.desktop;
        let active = mon.map(|m| m.active_workspace.as_str());
        // Numbered workspaces first, then named ones by name.
        let key = |id: i64, name: &str| (id <= 0, if id > 0 { id } else { 0 }, name.to_owned());
        let mut workspaces: Vec<((bool, i64, String), WorkspaceButton)> = d
            .workspaces
            .iter()
            .filter(|w| !w.name.starts_with("special:"))
            .filter(|w| mon.is_none_or(|m| w.monitor == m.name))
            .map(|w| {
                (
                    key(w.id, &w.name),
                    WorkspaceButton {
                        name: w.name.clone(),
                        active: Some(w.name.as_str()) == active,
                        accent: self.accent(&w.name),
                        empty: false,
                    },
                )
            })
            .collect();
        // Running contexts Hyprland has no workspace for (it drops empty
        // ones, ADR 0028): on the focused monitor, where it would open them.
        if mon.is_none_or(|m| m.focused) {
            for c in self.contexts() {
                if !d.workspaces.iter().any(|w| w.name == c.name) {
                    workspaces.push((
                        key(0, &c.name),
                        WorkspaceButton {
                            name: c.name.clone(),
                            active: false,
                            accent: Accent::Context(c.name.clone()),
                            empty: true,
                        },
                    ));
                }
            }
        }
        workspaces.sort_by(|a, b| a.0.cmp(&b.0));
        workspaces.into_iter().map(|(_, b)| b).collect()
    }

    /// The bar of the monitor with connector `monitor`; the first monitor
    /// when GDK does not know the connector.
    pub fn bar(&self, monitor: Option<&str>) -> BarView {
        let mon = self.monitor(monitor);
        BarView {
            workspaces: self.workspace_buttons(mon),
            style: self.bar.workspaces,
            menu: matches!(self.kojed, Kojed::Running(_)).then(|| {
                menu(&MenuInput {
                    running: self.contexts(),
                    all: &self.all,
                    memory: &self.memory,
                    history: &self.history,
                    busy: &self.busy,
                    errors: &self.errors,
                })
            }),
            quick: quick(&self.system),
            warning: (self.kojed == Kojed::Gone).then_some(KOJED_GONE),
        }
    }

    /// Every mapped window.
    pub fn windows(&self) -> &[WindowState] {
        &self.desktop.windows
    }

    /// The names of the running contexts.
    pub fn running(&self) -> Vec<String> {
        self.contexts().iter().map(|c| c.name.clone()).collect()
    }

    /// The context shown on the focused monitor, if one is.
    fn active_context(&self) -> Option<&str> {
        let mon = self.monitor(self.focused_monitor())?;
        match self.accent(&mon.active_workspace) {
            Accent::Context(_) => Some(mon.active_workspace.as_str()),
            Accent::Host => None,
        }
    }

    /// The programs of the running contexts.
    fn running_apps(&self) -> Vec<(String, Vec<AppItem>)> {
        self.contexts()
            .iter()
            .filter_map(|c| Some((c.name.clone(), self.apps.get(&c.name)?.clone())))
            .collect()
    }

    /// What the overview's search finds (ADR 0049).
    pub fn search(&self, query: &str) -> SearchView {
        search(
            query,
            &self.running_apps(),
            &self.host_apps,
            &self.desktop.windows,
            &self.running(),
            self.active_context(),
        )
    }

    /// The app grid (Super+A, ADR 0049).
    pub fn app_grid(&self) -> Vec<AppSection> {
        grid(&self.running_apps(), &self.host_apps, self.active_context())
    }

    /// What a workspace shortcut does on the focused monitor: the bar's
    /// workspaces in their order, contexts without a workspace among them;
    /// no step past either end. `carry`: the focused window goes along –
    /// only where it belongs (a host window to host workspaces, a context's
    /// to its own, as M8.71's dragging), else nothing happens.
    pub fn workspace_step(&self, step: WorkspaceStep, carry: bool) -> Vec<Action> {
        let mon = self.monitor(self.focused_monitor());
        let buttons = self.workspace_buttons(mon);
        let current = buttons.iter().position(|b| b.active);
        let target = match (step, current) {
            (WorkspaceStep::Nth(n), _) => n - 1,
            (WorkspaceStep::Next, Some(i)) => i + 1,
            (WorkspaceStep::Next, None) => 0,
            (WorkspaceStep::Prev, Some(i)) if i > 0 => i - 1,
            (WorkspaceStep::Prev, _) => return Vec::new(),
        };
        let Some(target) = buttons.get(target).filter(|_| Some(target) != current) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        let focused = self.desktop.windows.iter().find(|w| {
            w.focus_history == 0 && mon.is_none_or(|m| w.workspace == m.active_workspace)
        });
        if carry && let Some(w) = focused {
            let fits = match &target.accent {
                Accent::Context(c) => w.context.as_ref() == Some(c),
                Accent::Host => w.context.is_none(),
            };
            if !fits {
                return Vec::new();
            }
            actions.push(Action::MoveWindow {
                address: w.address.clone(),
                workspace: target.name.clone(),
            });
        }
        // A context without a workspace opens through kojed – unless a
        // window was just moved there, which made its workspace.
        actions.push(if target.empty && actions.is_empty() {
            Action::FocusContext(target.name.clone())
        } else {
            Action::FocusWorkspace(target.name.clone())
        });
        actions
    }

    /// The monitor with the keyboard focus.
    pub fn focused_monitor(&self) -> Option<&str> {
        self.desktop
            .monitors
            .iter()
            .find(|m| m.focused)
            .map(|m| m.name.as_str())
    }

    /// The overview of the monitor with connector `monitor` (ADR 0049);
    /// the dash only on the focused one. `host` finds the host's desktop
    /// files.
    pub fn overview(&self, monitor: Option<&str>, host: &HostApps<'_>) -> OverviewView {
        let mon = self.monitor(monitor);
        let windows = &self.desktop.windows;
        let current = mon.map_or("", |m| m.active_workspace.as_str());
        let accent = self.accent(current);
        let stash = match &accent {
            Accent::Context(name) => name
                .parse::<koje_core::name::ContextName>()
                .ok()
                .map(|ctx| {
                    let stash = koje_core::live_layouts::stash_workspace(&ctx);
                    let mut stashed: Vec<&WindowState> =
                        windows.iter().filter(|w| w.workspace == stash).collect();
                    stashed.sort_by_key(|w| w.focus_history);
                    Stash {
                        context: name.clone(),
                        windows: stashed
                            .into_iter()
                            .map(|w| WindowCard::new(w, accent.clone()))
                            .collect(),
                    }
                }),
            Accent::Host => None,
        };
        let focused = mon.is_none_or(|m| m.focused);
        let contexts: Vec<String> = self.contexts().iter().map(|c| c.name.clone()).collect();
        let active = match &accent {
            Accent::Context(name) => Some(name.as_str()),
            Accent::Host => None,
        };
        let dash = if focused {
            dash(&DashInput {
                favorites: &self.favorites,
                windows,
                contexts: &contexts,
                active,
                apps: active
                    .and_then(|c| self.apps.get(c))
                    .map_or(&[], Vec::as_slice),
                context_favorites: active
                    .and_then(|c| self.context_favorites.get(c))
                    .and_then(Option::as_deref),
                desktop_id: host.desktop_id,
                host_name: host.name,
            })
        } else {
            Vec::new()
        };
        OverviewView {
            live: self.desktop.plugin,
            dash,
            workspaces: self.workspace_buttons(mon),
            windows: on_workspace(windows, current)
                .into_iter()
                .map(|w| WindowCard::new(w, accent.clone()))
                .collect(),
            stash,
        }
    }

    /// The accent classes for every context (stopped ones are in the koje
    /// menu too) and the host.
    pub fn accent_css(&self) -> String {
        if self.all.is_empty() {
            return accent_css(self.contexts());
        }
        let all: Vec<RunningContext> = self
            .all
            .iter()
            .map(|c| RunningContext {
                name: c.name.clone(),
                color: c.color.clone(),
                active: false,
            })
            .collect();
        accent_css(&all)
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

    /// Two monitors: eDP-1 shows `kp`, DP-1 shows `3`.
    fn desktop() -> Desktop {
        Desktop {
            monitors: vec![
                MonitorState {
                    name: "eDP-1".into(),
                    active_workspace: "kp".into(),
                    focused: true,
                },
                MonitorState {
                    name: "DP-1".into(),
                    active_workspace: "3".into(),
                    focused: false,
                },
            ],
            workspaces: vec![
                ws(-1337, "kp", "eDP-1"),
                ws(2, "2", "eDP-1"),
                ws(-1338, "admin", "eDP-1"),
                ws(1, "1", "eDP-1"),
                ws(-98, "special:stash-kp", "eDP-1"),
                ws(3, "3", "DP-1"),
            ],
            windows: vec![
                win("0xa", "kp", (960, 40), 1),
                win("0xb", "kp", (0, 40), 0),
                win("0xc", "special:stash-kp", (0, 40), 3),
                win("0xd", "special:stash-kp", (0, 40), 2),
                win("0xe", "1", (0, 40), 4),
            ],
            plugin: false,
        }
    }

    fn win(address: &str, workspace: &str, at: (i32, i32), focus_history: i32) -> WindowState {
        WindowState {
            address: address.into(),
            class: "foot".into(),
            title: format!("title {address}"),
            workspace: workspace.into(),
            at,
            size: (960, 1040),
            focus_history,
            context: workspace
                .strip_prefix("special:stash-")
                .or((workspace == "kp").then_some("kp"))
                .map(str::to_owned),
        }
    }

    const NO_HOST: HostApps<'static> = HostApps {
        desktop_id: &|_| None,
        name: &|_| None,
    };

    fn shell() -> Shell {
        let mut s = Shell::default();
        s.apply(Update::Desktop(desktop()));
        s.apply(Update::Contexts(vec![ctx("kp", true)]));
        s
    }

    #[test]
    fn the_overview_shows_the_workspace_and_the_contexts_stash() {
        let o = shell().overview(Some("eDP-1"), &NO_HOST);
        let addresses = |cards: &[WindowCard]| -> Vec<String> {
            cards.iter().map(|c| c.address.clone()).collect()
        };
        // Left to right on the screen.
        assert_eq!(addresses(&o.windows), ["0xb", "0xa"]);
        assert!(
            o.windows
                .iter()
                .all(|c| c.accent == Accent::Context("kp".into()))
        );
        assert_eq!(o.windows[0].size, (960, 1040));
        let stash = o.stash.expect("kp's stash");
        assert_eq!(stash.context, "kp");
        // Most recently used first.
        assert_eq!(addresses(&stash.windows), ["0xd", "0xc"]);
        // The strip is the bar's list.
        assert_eq!(o.workspaces, shell().bar(Some("eDP-1")).workspaces);
    }

    #[test]
    fn a_host_workspace_has_no_stash() {
        let o = shell().overview(Some("DP-1"), &NO_HOST);
        assert_eq!(o.windows, []);
        assert_eq!(o.stash, None);
        let mut s = shell();
        let mut d = desktop();
        d.monitors[0].active_workspace = "1".into();
        s.apply(Update::Desktop(d));
        let o = s.overview(Some("eDP-1"), &NO_HOST);
        assert_eq!(o.windows.len(), 1);
        assert_eq!(o.windows[0].accent, Accent::Host);
        assert_eq!(o.stash, None);
    }

    #[test]
    fn starts_empty_and_without_warning() {
        let bar = Shell::default().bar(None);
        assert_eq!(bar.workspaces.len(), 0);
        assert_eq!(bar.warning, None);
        assert_eq!(bar.style, WorkspaceStyle::ColoredDots);
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
    fn workspace_shortcuts_go_through_the_bars_order() {
        let mut s = shell();
        s.apply(Update::Contexts(vec![ctx("kp", true), ctx("leer", false)]));
        // eDP-1: 1, 2, admin, kp (active), leer (no workspace).
        let go = |w: &str| vec![Action::FocusWorkspace(w.into())];
        assert_eq!(s.workspace_step(WorkspaceStep::Nth(1), false), go("1"));
        assert_eq!(s.workspace_step(WorkspaceStep::Prev, false), go("admin"));
        assert_eq!(
            s.workspace_step(WorkspaceStep::Next, false),
            [Action::FocusContext("leer".into())]
        );
        // Not past the ends, nothing for the one shown.
        assert_eq!(s.workspace_step(WorkspaceStep::Nth(6), false), []);
        assert_eq!(s.workspace_step(WorkspaceStep::Nth(4), false), []);
        let mut d = desktop();
        d.monitors[0].active_workspace = "1".into();
        s.apply(Update::Desktop(d));
        assert_eq!(s.workspace_step(WorkspaceStep::Prev, false), []);
    }

    #[test]
    fn with_shift_the_focused_window_goes_along() {
        let mut s = shell();
        s.apply(Update::Contexts(vec![ctx("kp", true), ctx("leer", false)]));
        // kp's window stays in kp.
        assert_eq!(s.workspace_step(WorkspaceStep::Nth(2), true), []);
        assert_eq!(s.workspace_step(WorkspaceStep::Next, true), []);
        // A host window goes to host workspaces.
        let mut d = desktop();
        d.monitors[0].active_workspace = "1".into();
        d.windows[4].focus_history = 0;
        d.windows[1].focus_history = 5;
        s.apply(Update::Desktop(d));
        assert_eq!(
            s.workspace_step(WorkspaceStep::Next, true),
            [
                Action::MoveWindow {
                    address: "0xe".into(),
                    workspace: "2".into()
                },
                Action::FocusWorkspace("2".into())
            ]
        );
        assert_eq!(s.workspace_step(WorkspaceStep::Nth(4), true), []);
    }

    #[test]
    fn workspace_steps_parse() {
        assert_eq!(WorkspaceStep::parse("3"), Some(WorkspaceStep::Nth(3)));
        assert_eq!(WorkspaceStep::parse("next"), Some(WorkspaceStep::Next));
        assert_eq!(WorkspaceStep::parse("prev"), Some(WorkspaceStep::Prev));
        assert_eq!(WorkspaceStep::parse("0"), None);
        assert_eq!(WorkspaceStep::parse("up"), None);
    }

    #[test]
    fn running_contexts_without_a_workspace_show_on_the_focused_monitor() {
        let mut s = shell();
        s.apply(Update::Contexts(vec![
            ctx("kp", true),
            ctx("leer", false),
            ctx("admin", false),
        ]));
        let names = |bar: &BarView| -> Vec<(String, bool)> {
            bar.workspaces
                .iter()
                .map(|w| (w.name.clone(), w.empty))
                .collect()
        };
        let focused = s.bar(Some("eDP-1"));
        assert_eq!(
            names(&focused),
            [
                ("1".into(), false),
                ("2".into(), false),
                ("admin".into(), false),
                ("kp".into(), false),
                ("leer".into(), true),
            ]
        );
        let leer = &focused.workspaces[4];
        assert_eq!(
            (leer.active, &leer.accent),
            (false, &Accent::Context("leer".into()))
        );
        // Not on the other monitor, and not twice when it exists there.
        assert_eq!(names(&s.bar(Some("DP-1"))), [("3".into(), false)]);
    }

    #[test]
    fn each_monitor_gets_its_own_bar() {
        let bar = shell().bar(Some("DP-1"));
        let names: Vec<_> = bar.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["3"]);
    }

    #[test]
    fn unknown_connector_shows_the_first_monitor() {
        assert_eq!(shell().bar(None), shell().bar(Some("eDP-1")));
    }

    #[test]
    fn the_menu_follows_kojed() {
        assert_eq!(Shell::default().bar(None).menu, None, "kojed not asked yet");
        let mut s = shell();
        let menu = s.bar(None).menu.unwrap();
        assert_eq!(menu.accent, Accent::Context("kp".into()));
        s.apply(Update::Memory(vec![MemoryReading {
            context: "kp".into(),
            used: 1 << 30,
            charged: 1 << 30,
            soft: 0,
            hard: 2 << 30,
        }]));
        assert_eq!(s.bar(None).menu.unwrap().meter.unwrap().label, "50%");
        let error = |s: &Shell| match s.bar(None).menu.unwrap().panel {
            crate::menu::Panel::Context(p) => p.error,
            crate::menu::Panel::ContextList(_) => None,
        };
        s.apply(Update::Busy {
            context: "kp".into(),
            busy: Busy::ChangingLimits,
        });
        s.apply(Update::Done {
            context: "kp".into(),
            error: Some("bad".into()),
        });
        assert_eq!(error(&s), Some("bad".into()));
        s.apply(Update::Busy {
            context: "kp".into(),
            busy: Busy::ChangingLimits,
        });
        assert_eq!(error(&s), None, "a new try clears the error");
    }

    #[test]
    fn warns_while_kojed_is_gone_and_forgets_its_contexts() {
        let mut s = shell();
        s.apply(Update::KojedGone);
        let bar = s.bar(None);
        assert_eq!(bar.menu, None);
        assert_eq!(bar.warning, Some(KOJED_GONE));
        // Without kojed no context colors: all grey.
        assert!(bar.workspaces.iter().all(|w| w.accent == Accent::Host));
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
            Some(CompositorCommand::FocusWorkspace {
                workspace: "kp".into()
            })
        );
        assert_eq!(Action::FocusContext("kp".into()).command(), None);
        assert_eq!(Action::FocusContext("kp".into()).koje(), None);
    }

    #[test]
    fn actions_run_koje() {
        let args = |a: Action| {
            let (context, busy, cmds) = a.koje().unwrap();
            let cmds: Vec<String> = cmds.iter().map(|c| c.join(" ")).collect();
            (context, busy, cmds)
        };
        assert_eq!(
            args(Action::Restart("kp".into())),
            (
                "kp".into(),
                Busy::Restarting,
                vec!["down kp".into(), "up kp".into()]
            )
        );
        assert_eq!(
            args(Action::Pause("kp".into())),
            ("kp".into(), Busy::Pausing, vec!["sleep kp".into()])
        );
        assert_eq!(
            args(Action::Create {
                name: "neu".into(),
                color: Some("#3584e4".into())
            }),
            (
                NEW_CONTEXT.into(),
                Busy::Creating,
                vec!["new neu --color #3584e4".into(), "up neu".into()]
            )
        );
        assert_eq!(
            args(Action::Create {
                name: "neu".into(),
                color: None
            })
            .2[0],
            "new neu"
        );
        assert_eq!(
            args(Action::SetLimits {
                context: "kp".into(),
                soft: "6G".into(),
                hard: "none".into()
            })
            .2,
            ["limit kp --soft 6G --hard none"]
        );
    }

    #[test]
    fn context_entries_from_kojed() {
        let e = ContextEntry::from(("kp".to_owned(), "#ff0000".to_owned(), "paused".to_owned()));
        assert_eq!(e.status, Status::Paused);
        let odd = ContextEntry::from(("kp".to_owned(), "#ff0000".to_owned(), "weird".to_owned()));
        assert_eq!(odd.status, Status::Stopped);
    }

    #[test]
    fn the_workspace_style_follows_config_toml() {
        let mut s = shell();
        s.apply(Update::Bar(BarConfig {
            workspaces: WorkspaceStyle::Names,
        }));
        assert_eq!(s.bar(None).style, WorkspaceStyle::Names);
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
