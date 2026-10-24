//! `koje doctor` (ADR 0021, 0022): check what koje needs on this host and say
//! how to fix what is missing. Exits with an error when a check fails.

use std::fmt;

use anyhow::{Result, bail};
use koje_core::paths::Paths;
use koje_core::setup::{Action, Mark};
use koje_core::spec::{NVIDIA_CDI_GENERATE, NvidiaCdi};

use crate::setup;

/// Hyprland versions before this use another rule syntax (M2.2).
const MIN_HYPRLAND: (u32, u32) = (0, 53);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub what: String,
    /// How to fix it (empty when fine).
    pub hint: String,
}

impl Check {
    fn ok(what: impl Into<String>) -> Self {
        Self {
            level: Level::Ok,
            what: what.into(),
            hint: String::new(),
        }
    }

    fn warn(what: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            level: Level::Warn,
            what: what.into(),
            hint: hint.into(),
        }
    }

    fn fail(what: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            level: Level::Fail,
            what: what.into(),
            hint: hint.into(),
        }
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.level {
            Level::Ok => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        write!(f, "{tag}  {}", self.what)?;
        if !self.hint.is_empty() {
            write!(f, "\n      → {}", self.hint)?;
        }
        Ok(())
    }
}

/// `v0.53.3` → whether it is at least `min`; `None` if unreadable.
pub fn version_at_least(tag: &str, min: (u32, u32)) -> Option<bool> {
    let mut parts = tag.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor) >= min)
}

fn packages() -> Check {
    let missing = setup::missing_packages();
    if missing.is_empty() {
        Check::ok("system packages")
    } else {
        Check::fail(
            format!("missing packages: {}", missing.join(" ")),
            setup::install_hint(&missing),
        )
    }
}

fn hyprland() -> Check {
    let reply = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|rt| {
            rt.block_on(koje_hyprland::Hyprland::from_env().request("j/version"))
                .map_err(anyhow::Error::from)
        });
    let reply = match reply {
        Ok(r) => r,
        Err(e) => {
            return Check::warn(
                format!("Hyprland: not reachable ({e})"),
                "run koje doctor inside your Hyprland session",
            );
        }
    };
    let tag = serde_json::from_str::<serde_json::Value>(&reply)
        .ok()
        .and_then(|v| v["tag"].as_str().map(str::to_owned))
        .unwrap_or_default();
    match version_at_least(&tag, MIN_HYPRLAND) {
        Some(true) => Check::ok(format!("Hyprland {tag}")),
        Some(false) => Check::fail(
            format!("Hyprland {tag}: too old"),
            format!(
                "koje needs Hyprland {}.{} or newer",
                MIN_HYPRLAND.0, MIN_HYPRLAND.1
            ),
        ),
        None => Check::warn(format!("Hyprland: unknown version {tag:?}"), ""),
    }
}

/// The files `koje setup` installs.
fn installed(paths: &Paths) -> Vec<Check> {
    let steps = match setup::plan(paths) {
        Ok(s) => s,
        Err(e) => return vec![Check::fail(format!("setup plan: {e:#}"), "")],
    };
    steps
        .iter()
        .filter(|s| !s.what.starts_with("base image"))
        .map(|s| {
            let label = s.what.split(':').next().unwrap_or_default();
            let what = setup::tilde(&match &s.action {
                Action::Write { path, .. } => format!("{label}: {}", path.display()),
                Action::SourceLine { path, .. } | Action::HyprlandConf { path, .. } => {
                    format!("{} sources koje's snippet", path.display())
                }
                _ => s.what.clone(),
            });
            match (s.mark, &s.action) {
                (Mark::Unchanged | Mark::Skipped, _) => Check::ok(setup::tilde(&s.what)),
                (_, Action::SourceLine { .. }) => Check::fail(format!("{what}: no"), "koje setup"),
                // Present but nothing of the user's in it: setup writes koje's.
                (Mark::Overwrite, Action::HyprlandConf { .. }) => {
                    Check::warn(format!("{what}: not your own yet"), "koje setup")
                }
                (Mark::New | Mark::Append, _) => {
                    Check::fail(format!("{what}: not installed"), "koje setup")
                }
                (Mark::Overwrite, _) => Check::warn(
                    format!("{what}: differs from this koje version"),
                    "koje setup",
                ),
            }
        })
        .collect()
}

fn kojed() -> Vec<Check> {
    match crate::kojed::call::<_, String>("WaylandStatus", &()) {
        Err(e) => vec![Check::fail(
            format!("kojed: {e:#}"),
            "systemctl --user start kojed",
        )],
        Ok(status) if status.is_empty() => vec![
            Check::ok("kojed"),
            Check::ok("Wayland security contexts (ADR 0022)"),
        ],
        Ok(status) => vec![
            Check::ok("kojed"),
            Check::fail(
                format!("Wayland security contexts: {status}"),
                "contexts need a compositor with security-context-v1 (Hyprland 0.53+)",
            ),
        ],
    }
}

fn image() -> Check {
    match koje_podman::Podman::new().image_exists(koje_podman::image::BASE_IMAGE) {
        Ok(true) => Check::ok("base image koje/base"),
        Ok(false) => Check::warn("base image koje/base: not built", "koje image build"),
        Err(e) => Check::fail(format!("Podman: {e}"), ""),
    }
}

fn forwarder() -> Check {
    match crate::hostbus::forwarder_binary() {
        Some(p) => Check::ok(format!("forwarder {}", p.display())),
        None => Check::warn(
            "forwarder ~/.local/lib/koje/koje-forward: missing",
            "./install.sh builds it; without it contexts get no host interfaces",
        ),
    }
}

fn subids() -> Check {
    let Ok(user) = koje_podman::host::image_user() else {
        return Check::warn("sub-ids: unknown user", "");
    };
    let has = |file: &str| {
        std::fs::read_to_string(file).is_ok_and(|t| {
            t.lines().any(|l| {
                l.split(':')
                    .next()
                    .is_some_and(|n| n == user.name || n == user.uid.to_string())
            })
        })
    };
    if has("/etc/subuid") && has("/etc/subgid") {
        Check::ok("sub-ids for rootless Podman")
    } else {
        Check::fail(
            format!("no sub-ids for {} in /etc/subuid or /etc/subgid", user.name),
            format!(
                "sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 {}",
                user.name
            ),
        )
    }
}

fn nvidia() -> Option<Check> {
    let host = koje_podman::host::detect().ok()?;
    let nvidia = host.nvidia?;
    Some(match nvidia.cdi {
        NvidiaCdi::Current => Check::ok("NVIDIA CDI spec"),
        NvidiaCdi::Missing => Check::warn(
            "NVIDIA: no CDI spec, contexts render in software",
            NVIDIA_CDI_GENERATE,
        ),
        NvidiaCdi::Outdated => Check::warn(
            "NVIDIA: CDI spec is for another driver version",
            NVIDIA_CDI_GENERATE,
        ),
    })
}

pub fn run(paths: &Paths) -> Result<()> {
    let mut checks = vec![packages(), hyprland()];
    checks.extend(installed(paths));
    checks.extend(kojed());
    checks.extend([image(), forwarder(), subids()]);
    checks.extend(nvidia());
    for c in &checks {
        println!("{c}");
    }
    let failed = checks.iter().filter(|c| c.level == Level::Fail).count();
    if failed > 0 {
        bail!("{failed} check(s) failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_at_least("v0.53.3", (0, 53)), Some(true));
        assert_eq!(version_at_least("v0.54.0", (0, 53)), Some(true));
        assert_eq!(version_at_least("v1.0.0", (0, 53)), Some(true));
        assert_eq!(version_at_least("v0.52.9", (0, 53)), Some(false));
        assert_eq!(version_at_least("", (0, 53)), None);
        assert_eq!(version_at_least("git-abc", (0, 53)), None);
    }

    #[test]
    fn output() {
        assert_eq!(Check::ok("kojed").to_string(), "ok    kojed");
        assert_eq!(
            Check::fail("kojed: not running", "systemctl --user start kojed").to_string(),
            "FAIL  kojed: not running\n      → systemctl --user start kojed"
        );
    }
}
