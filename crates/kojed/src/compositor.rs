//! The compositor kojed drives (ADR 0059): Hyprland, or GNOME Shell through
//! koje's extension. Chosen once at start ([`Kind::from_env`]).

pub use koje_core::compositor::Kind;
use koje_core::ports::{Compositor, CompositorCommand, CompositorEvent, WindowInfo};
use koje_core::snap::MonitorArea;
use koje_gnome::Gnome;
use koje_hyprland::Hyprland;

#[derive(Debug)]
pub enum Desktop {
    Hyprland(Hyprland),
    Gnome(Gnome),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Hyprland(#[from] koje_hyprland::Error),
    #[error(transparent)]
    Gnome(#[from] koje_gnome::Error),
}

impl Desktop {
    pub fn events(&self) -> Events {
        match self {
            Self::Hyprland(h) => Events::Hyprland(h.events()),
            Self::Gnome(g) => Events::Gnome(g.events()),
        }
    }
}

impl Compositor for Desktop {
    type Error = Error;

    async fn windows(&self) -> Result<Vec<WindowInfo>, Error> {
        Ok(match self {
            Self::Hyprland(h) => h.windows().await?,
            Self::Gnome(g) => g.windows().await?,
        })
    }

    async fn active_workspace(&self) -> Result<String, Error> {
        Ok(match self {
            Self::Hyprland(h) => Compositor::active_workspace(h).await?,
            Self::Gnome(g) => g.active_workspace().await?,
        })
    }

    async fn monitors(&self) -> Result<Vec<MonitorArea>, Error> {
        Ok(match self {
            Self::Hyprland(h) => Compositor::monitors(h).await?,
            Self::Gnome(g) => g.monitors().await?,
        })
    }

    fn has_stash(&self) -> bool {
        match self {
            Self::Hyprland(h) => h.has_stash(),
            Self::Gnome(g) => g.has_stash(),
        }
    }

    async fn apply(&self, commands: &[CompositorCommand]) -> Result<(), Error> {
        match self {
            Self::Hyprland(h) => h.apply(commands).await?,
            Self::Gnome(g) => g.apply(commands).await?,
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Events {
    Hyprland(koje_hyprland::Events),
    Gnome(koje_gnome::Events),
}

impl Events {
    pub async fn next(&mut self) -> CompositorEvent {
        match self {
            Self::Hyprland(e) => e.next().await,
            Self::Gnome(e) => e.next().await,
        }
    }
}
