//! Ports: the boundaries to Hyprland and Podman.
//!
//! Logic in `koje-core` depends only on these traits. Real adapters live in
//! the binaries (or future adapter crates); tests use in-memory fakes. Keep
//! the traits small and add methods only when a milestone needs them.
//!
//! The compositor port is asynchronous (kojed runs on tokio, ADR 0026); the
//! container runtime is synchronous for now (kojed calls it in
//! `spawn_blocking`).

use std::future::Future;

use crate::color::Color;
use crate::layout::Placement;
use crate::memory::MemoryUsage;
use crate::name::ContextName;
use crate::spec::ContainerSpec;

/// A window as reported by the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Compositor-specific address (Hyprland: `0x55d1…`, always with `0x`).
    pub address: String,
    pub pid: u32,
    /// Wayland app id / class.
    pub app_id: String,
    pub title: String,
    pub workspace: String,
    pub placement: Placement,
}

/// One step for the compositor; [`Compositor::apply`] sends a list as one batch
/// (Hyprland `[[BATCH]]`: 10 commands in 0.1 ms, M2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositorCommand {
    /// Border colors for every window carrying `tag` (ADR 0018). Defining it
    /// again replaces the colors.
    ContextRule {
        tag: String,
        active: Color,
        inactive: Color,
    },
    /// Border colors of one window, overriding rules. Rules apply only when a
    /// window opens, so a context's color change reaches its open windows this way.
    BorderColors {
        address: String,
        active: Color,
        inactive: Color,
    },
    /// Give a window a tag (it then gets the tag's rule).
    TagWindow {
        address: String,
        tag: String,
    },
    /// Move and resize a window.
    Place {
        address: String,
        placement: Placement,
    },
    /// Move a window to a workspace without following it.
    MoveToWorkspace {
        address: String,
        workspace: String,
    },
    FocusWindow {
        address: String,
    },
    /// Show or hide a special workspace (a context's stash), by its name
    /// without `special:`.
    ToggleSpecial {
        name: String,
    },
    /// Ask a window to close (the program may ask to save first).
    CloseWindow {
        address: String,
    },
    FocusWorkspace {
        workspace: String,
    },
    /// Show a workspace with a window going along: it stays on screen and
    /// focused while the workspace changes behind it, as in GNOME. Only
    /// for a floating window that is neither fullscreen nor maximized.
    CarryToWorkspace {
        address: String,
        workspace: String,
    },
    /// A context fell asleep or woke up, on a compositor without a stash
    /// (GNOME, ADR 0059): its windows stay and get the sleep overlay.
    SleepOverlay {
        context: String,
        asleep: bool,
    },
}

/// What the compositor reports (Hyprland: socket2 events, M2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositorEvent {
    WindowOpened {
        address: String,
        workspace: String,
        class: String,
        title: String,
    },
    WindowClosed {
        address: String,
    },
    /// The focused window changed; `None` when nothing has focus.
    WindowFocused {
        address: Option<String>,
    },
    WindowTitle {
        address: String,
        title: String,
    },
    /// The focused workspace changed (its name).
    WorkspaceFocused {
        name: String,
    },
    /// The compositor reloaded its config: runtime rules are gone (ADR 0018).
    ConfigReloaded,
    /// The event connection is up, first or again after it was lost (the
    /// compositor restarted): windows may have changed unseen, resync.
    Connected,
}

/// Window manager operations koje needs (implemented for Hyprland).
pub trait Compositor {
    type Error: std::error::Error + Send + Sync + 'static;

    fn windows(&self) -> impl Future<Output = Result<Vec<WindowInfo>, Self::Error>> + Send;
    /// The name of the focused workspace.
    fn active_workspace(&self) -> impl Future<Output = Result<String, Self::Error>> + Send;
    /// The monitors with the room bars reserve (snapping, M8.41).
    fn monitors(
        &self,
    ) -> impl Future<Output = Result<Vec<crate::snap::MonitorArea>, Self::Error>> + Send;
    /// Apply all commands in one request.
    fn apply(
        &self,
        commands: &[CompositorCommand],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Whether the compositor has hidden workspaces for a context's stash
    /// (Hyprland). Without them (GNOME, ADR 0059) a sleeping context's
    /// windows stay where they are under the sleep overlay.
    fn has_stash(&self) -> bool {
        true
    }
}

/// Lifecycle of a context container as seen from koje.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerStatus {
    Missing,
    Stopped,
    Running,
    Paused,
}

/// Container operations koje needs (implemented for rootless Podman).
pub trait ContainerRuntime {
    type Error: std::error::Error + Send + Sync + 'static;

    fn status(&self, ctx: &ContextName) -> Result<ContainerStatus, Self::Error>;
    /// Create (not start) a context container from a spec (`crate::spec::build`).
    fn create(&self, spec: &ContainerSpec) -> Result<(), Self::Error>;
    /// Remove a stopped context container. Its home volume stays (M3.8 removes it).
    fn remove(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn start(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn stop(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn pause(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn unpause(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    /// Memory of the context's cgroup (ADR 0035; dev containers included, M2.6).
    fn memory_usage(&self, ctx: &ContextName) -> Result<MemoryUsage, Self::Error>;
    /// Start `command` (for `sh -c`) in the running context, detached, in
    /// `workdir` if given (restoring a state, M5.3).
    fn launch(
        &self,
        ctx: &ContextName,
        command: &str,
        workdir: Option<&str>,
    ) -> Result<(), Self::Error>;
    /// Run `argv` in the running context and return its output (asking a
    /// program over the context's bus, M5.12).
    fn query(&self, ctx: &ContextName, argv: &[&str]) -> Result<String, Self::Error>;
    /// Ask a context's process (host pid) to quit: SIGTERM (M5.4).
    fn terminate(&self, pid: u32) -> Result<(), Self::Error>;
    /// Map a host PID to the context whose container owns it, if any.
    fn context_of_pid(&self, pid: u32) -> Result<Option<ContextName>, Self::Error>;
}
