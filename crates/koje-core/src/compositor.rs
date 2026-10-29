//! Which compositor koje runs on (ADR 0059): Hyprland, or GNOME Shell with
//! koje's extension. kojed picks its adapter by it, the CLI refuses what
//! GNOME does not have. Pure: the caller reads the environment.

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

impl Kind {
    /// From the process environment.
    pub fn from_env() -> Result<Self, String> {
        select(
            std::env::var("KOJE_COMPOSITOR").ok().as_deref(),
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
            std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        )
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
