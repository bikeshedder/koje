//! The connected screens and their configuration from GNOME's
//! `monitors.xml` (ADR 0046): read from `/sys/class/drm` (connector and EDID),
//! so they are known before Hyprland starts. Decisions:
//! `koje_core::monitors`.

use std::collections::BTreeSet;
use std::path::Path;

use koje_core::monitors::{self, MonitorsXml, Spec};
use koje_core::paths::Paths;

const DRM: &str = "/sys/class/drm";

/// The connected screens with Mutter's identity; screens without a readable
/// EDID are left out (Mutter could not tell them apart either).
pub fn connected() -> BTreeSet<Spec> {
    let Ok(entries) = std::fs::read_dir(DRM) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let connector = monitors::connector_of(&name)?.to_owned();
            let status = std::fs::read_to_string(e.path().join("status")).ok()?;
            if status.trim() != "connected" {
                return None;
            }
            // sysfs reports the file as empty; reading gives the bytes.
            let edid = std::fs::read(e.path().join("edid")).ok()?;
            Some(Spec {
                connector,
                id: koje_core::edid::identity(&edid)?,
            })
        })
        .collect()
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
