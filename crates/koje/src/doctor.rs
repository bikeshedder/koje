//! `koje doctor` (ADR 0021): check what koje needs on this host and say
//! how to fix what is missing. Exits with an error when a check fails.

use std::fmt;

use anyhow::{Result, bail};
use koje_core::spec::{NVIDIA_CDI_GENERATE, NvidiaCdi};

use crate::setup;

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

/// The GNOME Shell koje's extension is for (ADR 0059).
const GNOME_SHELL: u32 = 50;

/// `GNOME Shell 50.1` → 50.
pub fn gnome_major(version: &str) -> Option<u32> {
    version
        .trim()
        .strip_prefix("GNOME Shell ")?
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn gnome_shell() -> Check {
    let version = std::process::Command::new("gnome-shell")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    match gnome_major(&version) {
        Some(GNOME_SHELL) => Check::ok(version),
        Some(_) => Check::warn(
            format!("{version}: koje's extension is for GNOME Shell {GNOME_SHELL}"),
            "it may not load; see docs/plan/G-gnome.md",
        ),
        None => Check::fail(
            "GNOME Shell: not found",
            "koje on GNOME needs GNOME Shell 50",
        ),
    }
}

/// Whether `name` has an owner on the session bus.
fn on_session_bus(name: &str) -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        let conn = zbus::Connection::session().await.ok()?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
        dbus.name_has_owner(name.try_into().ok()?).await.ok()
    }) == Some(true)
}

fn gnome_extension() -> Check {
    let uuid = koje_core::setup::GNOME_EXTENSION;
    if on_session_bus("io.koje.Extension") {
        return Check::ok(format!("GNOME Shell extension {uuid}"));
    }
    let installed = std::path::Path::new("/usr/share/gnome-shell/extensions")
        .join(uuid)
        .join("metadata.json")
        .is_file();
    if installed {
        Check::fail(
            format!("GNOME Shell extension {uuid}: not running"),
            "koje setup, then log out and in once",
        )
    } else {
        Check::fail(
            format!("GNOME Shell extension {uuid}: not installed"),
            "install the koje package",
        )
    }
}

fn kojed() -> Check {
    match crate::kojed::call::<_, String>("ActiveContext", &()) {
        Err(e) => Check::fail(format!("kojed: {e:#}"), "systemctl --user start kojed"),
        Ok(_) => Check::ok("kojed"),
    }
}

/// Contexts share Mutter's Wayland socket (ADR 0059, L1), which is fine only
/// while every global it offers is one spike G.2 reviewed.
fn wayland_globals() -> Check {
    match crate::wayland::globals() {
        Err(e) => Check::warn(
            format!("Wayland globals: {e:#}"),
            "run koje doctor in the GNOME session",
        ),
        Ok(globals) => match crate::wayland::unreviewed(&globals)[..] {
            [] => Check::ok(format!(
                "Wayland: all {} globals reviewed (contexts share Mutter's socket, ADR 0059 L1)",
                globals.len()
            )),
            ref new => Check::warn(
                format!(
                    "Wayland: contexts can use globals nobody reviewed: {}",
                    new.join(" ")
                ),
                "contexts share Mutter's socket (ADR 0059, L1); check what they allow, as in docs/spikes/G.2-mutter-wayland-globals.md",
            ),
        },
    }
}

fn image() -> Check {
    let fedora = koje_core::image::Source::BuiltIn(koje_core::name::ImageName::default_image());
    match koje_podman::Podman::new().image_exists(&fedora.user_tag()) {
        Ok(true) => Check::ok("image fedora"),
        Ok(false) => Check::warn(
            "image fedora: not built (the first koje up builds it)",
            "koje image build fedora",
        ),
        Err(e) => Check::fail(format!("Podman: {e}"), ""),
    }
}

/// The koje package (ADR 0045).
fn package() -> Check {
    let version = std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status} ${Version}", "koje"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    match version
        .as_deref()
        .and_then(|v| v.strip_prefix("install ok installed "))
    {
        Some(v) => Check::ok(format!("koje package {v}")),
        None => Check::warn(
            "koje package: not installed",
            "build it in the dev container with `just deb`, then: sudo apt install ./target/debian/koje_*.deb",
        ),
    }
}

fn forwarder() -> Check {
    match koje_runtime::hostbus::forwarder_binary() {
        Some(p) => Check::ok(format!("forwarder {}", p.display())),
        None => Check::warn(
            format!(
                "forwarder {}/koje-forward: missing",
                koje_core::paths::LIB_DIR
            ),
            "the koje package installs it; without it contexts get no host interfaces",
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

pub fn run() -> Result<()> {
    let mut checks = vec![
        package(),
        packages(),
        gnome_shell(),
        gnome_extension(),
        wayland_globals(),
        kojed(),
        image(),
        forwarder(),
        subids(),
    ];
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
    fn gnome_versions() {
        assert_eq!(gnome_major("GNOME Shell 50.1\n"), Some(50));
        assert_eq!(gnome_major("GNOME Shell 49.4"), Some(49));
        assert_eq!(gnome_major(""), None);
        assert_eq!(gnome_major("Hyprland"), None);
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
