//! The compositor kojed drives (ADR 0059): Hyprland, or GNOME Shell through
//! koje's extension. Chosen once at start.

use koje_core::ports::{Compositor, CompositorCommand, CompositorEvent, WindowInfo};
use koje_core::snap::MonitorArea;
use koje_gnome::Gnome;
use koje_hyprland::Hyprland;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hyprland,
    Gnome,
}

/// Which compositor: `KOJE_COMPOSITOR` (`hyprland`, `gnome`) if set; else
/// GNOME when `XDG_CURRENT_DESKTOP` names GNOME (Ubuntu: `ubuntu:GNOME`), it
/// is not koje's own Hyprland session (`koje:GNOME`, ADR 0048) and no
/// Hyprland runs; else Hyprland.
pub fn select(
    koje_compositor: Option<&str>,
    current_desktop: Option<&str>,
    hyprland_instance: bool,
) -> Result<Kind, String> {
    match koje_compositor.map(str::trim).filter(|s| !s.is_empty()) {
        Some("hyprland") => return Ok(Kind::Hyprland),
        Some("gnome") => return Ok(Kind::Gnome),
        Some(other) => {
            return Err(format!(
                "KOJE_COMPOSITOR={other:?}: expected \"hyprland\" or \"gnome\""
            ));
        }
        None => {}
    }
    let desktops: Vec<&str> = current_desktop.unwrap_or_default().split(':').collect();
    let gnome = desktops.iter().any(|d| d.eq_ignore_ascii_case("gnome"));
    let koje = desktops
        .first()
        .is_some_and(|d| d.eq_ignore_ascii_case("koje"));
    Ok(if gnome && !koje && !hyprland_instance {
        Kind::Gnome
    } else {
        Kind::Hyprland
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection() {
        let pick = |k, d, h| select(k, d, h).unwrap();
        assert_eq!(pick(None, Some("GNOME"), false), Kind::Gnome);
        assert_eq!(pick(None, Some("ubuntu:GNOME"), false), Kind::Gnome);
        assert_eq!(pick(None, Some("gnome"), false), Kind::Gnome);
        // koje's own session on Hyprland.
        assert_eq!(pick(None, Some("koje:GNOME"), true), Kind::Hyprland);
        assert_eq!(pick(None, Some("koje:GNOME"), false), Kind::Hyprland);
        assert_eq!(pick(None, Some("Hyprland"), true), Kind::Hyprland);
        // A Hyprland running wins over what the session claims.
        assert_eq!(pick(None, Some("GNOME"), true), Kind::Hyprland);
        assert_eq!(pick(None, None, false), Kind::Hyprland);
        // The variable decides.
        assert_eq!(pick(Some("gnome"), Some("koje:GNOME"), true), Kind::Gnome);
        assert_eq!(pick(Some("hyprland"), Some("GNOME"), false), Kind::Hyprland);
        assert_eq!(pick(Some(""), Some("GNOME"), false), Kind::Gnome);
        assert!(select(Some("mutter"), None, false).is_err());
    }
}
