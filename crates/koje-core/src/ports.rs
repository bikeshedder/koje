//! Ports: the boundaries to Hyprland and Podman.
//!
//! Logic in `koje-core` depends only on these traits. Real adapters live in
//! the binaries (or future adapter crates); tests use in-memory fakes. Keep
//! the traits small and add methods only when a milestone needs them.
//!
//! These are synchronous on purpose for now. Whether `kojed` becomes async is
//! an open question (`docs/open-questions.md`).

use crate::layout::Placement;
use crate::name::ContextName;
use crate::size::ByteSize;

/// A window as reported by the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Compositor-specific address (Hyprland: hex address like `0x55d1…`).
    pub address: String,
    pub pid: u32,
    /// Wayland app id / class.
    pub app_id: String,
    pub title: String,
    pub workspace: String,
    pub placement: Placement,
}

/// Window manager operations koje needs (implemented for Hyprland).
pub trait Compositor {
    type Error: std::error::Error + Send + Sync + 'static;

    fn windows(&self) -> Result<Vec<WindowInfo>, Self::Error>;
    /// Move and resize in one step; may also move across workspaces.
    fn place(
        &self,
        address: &str,
        workspace: &str,
        placement: &Placement,
    ) -> Result<(), Self::Error>;
    fn set_border_color(
        &self,
        address: &str,
        active: &str,
        inactive: &str,
    ) -> Result<(), Self::Error>;
    fn focus_workspace(&self, workspace: &str) -> Result<(), Self::Error>;
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
    fn start(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn stop(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn pause(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    fn unpause(&self, ctx: &ContextName) -> Result<(), Self::Error>;
    /// Current memory usage of the context's cgroup.
    fn memory_usage(&self, ctx: &ContextName) -> Result<ByteSize, Self::Error>;
    /// Map a host PID to the context whose container owns it, if any.
    fn context_of_pid(&self, pid: u32) -> Result<Option<ContextName>, Self::Error>;
}
