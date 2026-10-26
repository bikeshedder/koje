//! The screens from GNOME's `monitors.xml` while Hyprland runs (ADR 0046):
//! after a screen was plugged in or out, and when the file changes (GNOME
//! Settings, a GNOME session on the same account, an edit). `koje session
//! start` set them at login; kojed keeps them current – the runtime file the
//! session config sources, so a config reload keeps them, and Hyprland's
//! `keyword monitor` for now. Screens the user's own file sets are left alone.
//! Decisions: `koje_core::monitors`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use koje_core::monitors;
use koje_core::paths::Paths;
use koje_hyprland::events::Line;
use koje_hyprland::{Hyprland, requests};

const DRM: &str = "/sys/class/drm";
/// How often monitors.xml is looked at.
const POLL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(5);

/// The screens' lines for the connected set (configuration, else each
/// screen's last settings, else none).
fn lines(paths: &Paths) -> Vec<String> {
    let connected = monitors::read_connected(Path::new(DRM));
    let Ok(text) = std::fs::read_to_string(paths.monitors_xml()) else {
        return Vec::new();
    };
    let xml = match monitors::parse(&text) {
        Ok(xml) => xml,
        Err(e) => {
            tracing::warn!("{e}");
            return Vec::new();
        }
    };
    match monitors::find(&xml, &connected) {
        Some(c) => monitors::hyprland_lines(c),
        None => monitors::fallback_lines(&xml, &connected),
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Follow hotplug and monitors.xml; apply what changed.
pub async fn run(hypr: Hyprland, paths: Paths, runtime: PathBuf) {
    let xml = paths.monitors_xml();
    let user_conf = paths.session_hyprland_conf();
    let runtime_conf = runtime.join("koje/monitors.conf");
    let mut events = hypr.events();
    let mut seen = modified(&xml);
    // What login set: no need to apply it again.
    let mut applied = lines(&paths);
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let check = tokio::select! {
            line = events.next_line() => match line {
                // Hyprland (re)started: it read the runtime file.
                Line::Connected => false,
                Line::Event(l) => is_hotplug(&l),
            },
            _ = poll.tick() => {
                let now = modified(&xml);
                let changed = now != seen;
                seen = now;
                changed
            }
        };
        if !check {
            continue;
        }
        // A plugged-in screen's EDID may appear a moment after the event.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let now = lines(&paths);
        if now == applied {
            continue;
        }
        if let Err(e) =
            koje_core::fsutil::write_atomic(&runtime_conf, monitors::hyprland_conf(&now).as_bytes())
        {
            tracing::warn!("writing {}: {e}", runtime_conf.display());
        }
        let own = std::fs::read_to_string(&user_conf)
            .map(|t| monitors::user_monitors(&t))
            .unwrap_or_default();
        let send = monitors::without(now.clone(), &own);
        match apply(&hypr, &send).await {
            Ok(()) => {
                tracing::info!(
                    screens = send.len(),
                    "applied the screens from monitors.xml"
                );
                applied = now;
            }
            Err(e) => tracing::warn!("applying the screens: {e:#}"),
        }
    }
}

/// A screen came or went.
fn is_hotplug(line: &str) -> bool {
    let name = line.split_once(">>").map_or(line, |(n, _)| n);
    matches!(
        name,
        "monitoradded" | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2"
    )
}

async fn apply(hypr: &Hyprland, lines: &[String]) -> anyhow::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let keywords: Vec<String> = lines
        .iter()
        .map(|l| format!("keyword monitor {l}"))
        .collect();
    let request = requests::batch(&keywords);
    let reply = tokio::time::timeout(TIMEOUT, hypr.request(&request)).await??;
    requests::check_reply(&request, &reply)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotplug_events() {
        assert!(is_hotplug("monitoraddedv2>>1,DP-2,Dell"));
        assert!(is_hotplug("monitorremoved>>DP-2"));
        assert!(!is_hotplug("workspacev2>>1,1"));
    }
}
