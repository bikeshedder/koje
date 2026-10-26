//! The connected screens and their configuration from GNOME's
//! `monitors.xml` (ADR 0046): read from `/sys/class/drm` (connector and EDID),
//! so they are known before Hyprland starts. Decisions:
//! `koje_core::monitors`.

use std::collections::BTreeSet;
use std::path::Path;

use koje_core::monitors::{self, MonitorsXml, Spec};
use koje_core::paths::Paths;

const DRM: &str = "/sys/class/drm";

/// The connected screens with Mutter's identity.
pub fn connected() -> BTreeSet<Spec> {
    monitors::read_connected(Path::new(DRM))
}

/// Where the screens' settings came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A configuration for exactly these screens.
    Exact,
    /// Each screen's last settings: no configuration for this set yet.
    PerScreen,
    /// Nothing usable: Hyprland's defaults.
    None(String),
}

/// Hyprland's `monitor` values for the connected screens, and why.
pub fn lines(paths: &Paths, connected: &BTreeSet<Spec>) -> (Vec<String>, Source) {
    let path = paths.monitors_xml();
    let xml = match read(&path) {
        Ok(Some(xml)) => xml,
        Ok(None) => return (Vec::new(), Source::None("no monitors.xml yet".to_owned())),
        Err(e) => return (Vec::new(), Source::None(e)),
    };
    if let Some(c) = monitors::find(&xml, connected) {
        return (monitors::hyprland_lines(c), Source::Exact);
    }
    let fallback = monitors::fallback_lines(&xml, connected);
    if fallback.is_empty() {
        (
            Vec::new(),
            Source::None("no settings for these screens".to_owned()),
        )
    } else {
        (fallback, Source::PerScreen)
    }
}

fn read(path: &Path) -> Result<Option<MonitorsXml>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => monitors::parse(&text).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
