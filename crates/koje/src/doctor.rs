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

/// One entry of `hyprctl -j binds`.
#[derive(Debug, serde::Deserialize)]
struct Bind {
    #[serde(default)]
    modmask: u32,
    #[serde(default)]
    submap: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    keycode: i64,
    #[serde(default)]
    mouse: bool,
    #[serde(default)]
    release: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    dispatcher: String,
    #[serde(default)]
    arg: String,
}

/// `Super+Shift+Return` from Hyprland's modifier mask.
fn combo(modmask: u32, key: &str) -> String {
    const MODS: [(u32, &str); 4] = [(64, "Super"), (4, "Ctrl"), (8, "Alt"), (1, "Shift")];
    let mut parts: Vec<&str> = MODS
        .iter()
        .filter(|(bit, _)| modmask & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    parts.push(key);
    parts.join("+")
}

/// A key combination bound more than once – each press runs all of its
/// bindings, e.g. koje's Super+Return and an own one open two terminals.
#[derive(Debug, PartialEq, Eq)]
pub struct Duplicate {
    /// `Super+Return`.
    pub combo: String,
    /// What each binding does: its description, else its command.
    pub what: Vec<String>,
}

pub fn duplicate_bindings(json: &str) -> Vec<Duplicate> {
    let Ok(binds) = serde_json::from_str::<Vec<Bind>>(json) else {
        return Vec::new();
    };
    let mut groups: std::collections::BTreeMap<_, Vec<&Bind>> = std::collections::BTreeMap::new();
    for b in &binds {
        let key = (&b.submap, b.modmask, &b.key, b.keycode, b.mouse, b.release);
        groups.entry(key).or_default().push(b);
    }
    groups
        .into_values()
        .filter(|g| clashes(g))
        .map(|g| Duplicate {
            combo: combo(g[0].modmask, &g[0].key),
            what: g
                .iter()
                .map(|b| {
                    if b.description.is_empty() {
                        format!("{} {}", b.dispatcher, b.arg).trim().to_owned()
                    } else {
                        b.description.clone()
                    }
                })
                .collect(),
        })
        .collect()
}

/// One key's bindings clash unless they are one binding in several lines:
/// koje's Alt+Tab runs `cyclenext` and `alterzorder` under one description.
/// The same line twice – a snippet sourced twice – clashes.
fn clashes(binds: &[&Bind]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let repeated = binds
        .iter()
        .any(|b| !seen.insert((&b.description, &b.dispatcher, &b.arg)));
    let one = !binds[0].description.is_empty()
        && binds.iter().all(|b| b.description == binds[0].description);
    binds.len() > 1 && (repeated || !one)
}

/// The doctor line: what each binding does while that stays short, else
/// only the combinations.
fn duplicates_line(dups: &[Duplicate]) -> String {
    if dups.len() <= 3 {
        let each: Vec<String> = dups
            .iter()
            .map(|d| format!("{} ({})", d.combo, d.what.join("; ")))
            .collect();
        format!("key bindings bound twice: {}", each.join(", "))
    } else {
        let combos: Vec<&str> = dups.iter().map(|d| d.combo.as_str()).collect();
        format!(
            "{} key combinations bound twice: {}",
            dups.len(),
            combos.join(", ")
        )
    }
}

/// Whether `hyprctl plugin list` (JSON) has koje's plugin.
pub fn plugin_listed(json: &str) -> bool {
    #[derive(serde::Deserialize)]
    struct Plugin {
        name: String,
    }
    serde_json::from_str::<Vec<Plugin>>(json).is_ok_and(|ps| ps.iter().any(|p| p.name == "koje"))
}

/// In the koje session: koje's plugin draws the overview's windows (ADR 0051).
fn plugin() -> Option<Check> {
    if std::env::var("XDG_SESSION_DESKTOP").as_deref() != Ok("koje") {
        return None;
    }
    let reply = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?
        .block_on(koje_hyprland::Hyprland::from_env().request("j/plugin list"))
        .ok()?;
    Some(if plugin_listed(&reply) {
        Check::ok("plugin: koje's Hyprland plugin is loaded")
    } else if std::path::Path::new(koje_core::session::PLUGIN).is_file() {
        Check::warn(
            "plugin: koje's Hyprland plugin is not loaded – the overview shows icons",
            "it loads at login (koje session start); after a Hyprland update rebuild koje (just deb) – \
             Hyprland shows why it refused it",
        )
    } else {
        Check::warn(
            format!("plugin: {} is missing", koje_core::session::PLUGIN),
            "reinstall the koje package",
        )
    })
}

/// Key combinations the running Hyprland binds twice.
fn bindings() -> Option<Check> {
    let reply = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?
        .block_on(koje_hyprland::Hyprland::from_env().request("j/binds"))
        .ok()?;
    let dups = duplicate_bindings(&reply);
    Some(if dups.is_empty() {
        Check::ok("key bindings: none bound twice")
    } else {
        Check::warn(
            duplicates_line(&dups),
            "your hyprland.conf binds keys koje's snippet binds too – remove yours, or move \
             the file away and run koje setup for a fresh one (hyprctl binds shows all)",
        )
    })
}

/// The connected screens as GNOME knows them, and whether monitors.xml has
/// their settings (ADR 0046).
fn screens(paths: &Paths) -> Vec<Check> {
    use crate::screens::Source;
    let connected = crate::screens::connected();
    let mut checks: Vec<Check> = connected
        .iter()
        .map(|s| {
            Check::ok(format!(
                "screen {}: {} {} {}",
                s.connector, s.id.vendor, s.id.product, s.id.serial
            ))
        })
        .collect();
    if connected.is_empty() {
        return checks;
    }
    let (_, source) = crate::screens::lines(paths, &connected);
    checks.push(match source {
        Source::Exact => Check::ok("screens: settings from monitors.xml"),
        Source::PerScreen => Check::ok("screens: each screen's last settings from monitors.xml"),
        Source::None(why) => Check::warn(
            format!("screens: Hyprland's defaults ({why})"),
            "set them in GNOME Settings' Displays (ADR 0046), or a monitor line in ~/.config/koje/hyprland.conf",
        ),
    });
    checks
}

/// In the koje session: its target and services run (ADR 0048).
fn session_units() -> Vec<Check> {
    if std::env::var("XDG_SESSION_DESKTOP").as_deref() != Ok("koje") {
        return Vec::new();
    }
    let active = |unit: &str| {
        std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", unit])
            .status()
            .is_ok_and(|s| s.success())
    };
    [
        "koje-session.target",
        "kojed.service",
        "koje-shell.service",
        "koje-idle.service",
        "koje-polkit-agent.service",
    ]
    .iter()
    .map(|unit| {
        if active(unit) {
            Check::ok(format!("session: {unit}"))
        } else {
            Check::warn(
                format!("session: {unit} is not running"),
                format!("systemctl --user status {unit}; koje session restart starts them again"),
            )
        }
    })
    .collect()
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
                (Mark::Remove, _) => Check::warn(
                    format!("{what}: left by install.sh, the package replaces it"),
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
    match crate::hostbus::forwarder_binary() {
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

pub fn run(paths: &Paths) -> Result<()> {
    let mut checks = vec![package(), packages(), hyprland()];
    checks.extend(bindings());
    checks.extend(plugin());
    checks.extend(screens(paths));
    checks.extend(session_units());
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
    fn bindings_bound_twice() {
        let json = r#"[
            {"modmask": 64, "submap": "", "key": "Return", "keycode": 0, "mouse": false, "release": false,
             "description": "Terminal in the context", "dispatcher": "exec", "arg": "koje terminal"},
            {"modmask": 64, "submap": "", "key": "Return", "keycode": 0, "mouse": false, "release": false,
             "description": "", "dispatcher": "exec", "arg": "foot"},
            {"modmask": 65, "submap": "", "key": "Return", "keycode": 0, "mouse": false, "release": false,
             "description": "", "dispatcher": "exec", "arg": "kitty"},
            {"modmask": 64, "submap": "", "key": "Super_L", "keycode": 0, "mouse": false, "release": true,
             "description": "Launcher", "dispatcher": "exec", "arg": "menu"},
            {"modmask": 64, "submap": "", "key": "Super_L", "keycode": 0, "mouse": false, "release": false,
             "description": "", "dispatcher": "exec", "arg": "other"}
        ]"#;
        let dups = duplicate_bindings(json);
        assert_eq!(
            dups,
            [Duplicate {
                combo: "Super+Return".into(),
                what: vec!["Terminal in the context".into(), "exec foot".into()]
            }]
        );
        assert_eq!(
            duplicates_line(&dups),
            "key bindings bound twice: Super+Return (Terminal in the context; exec foot)"
        );
        assert_eq!(duplicate_bindings("not json"), []);
    }

    #[test]
    fn one_binding_in_several_lines_is_not_bound_twice() {
        let bind = |dispatcher: &str, arg: &str| {
            format!(
                r#"{{"modmask": 8, "submap": "", "key": "Tab", "keycode": 0, "mouse": false, "release": false,
                    "description": "Next window on the workspace", "dispatcher": "{dispatcher}", "arg": "{arg}"}}"#
            )
        };
        let koje = format!(
            "[{}, {}]",
            bind("cyclenext", ""),
            bind("alterzorder", "top")
        );
        assert_eq!(duplicate_bindings(&koje), []);
        // koje's snippet sourced twice.
        let twice = format!(
            "[{}, {}, {}, {}]",
            bind("cyclenext", ""),
            bind("alterzorder", "top"),
            bind("cyclenext", ""),
            bind("alterzorder", "top")
        );
        assert_eq!(duplicate_bindings(&twice).len(), 1);
    }

    #[test]
    fn the_plugin_in_hyprlands_list() {
        assert!(plugin_listed(
            r#"[{"name": "koje", "author": "koje", "handle": "7f01", "version": "1", "description": ""}]"#
        ));
        assert!(!plugin_listed(r#"[{"name": "hyprbars"}]"#));
        assert!(!plugin_listed("no plugins loaded"));
    }

    #[test]
    fn many_duplicates_list_only_the_combinations() {
        let dup = |combo: &str| Duplicate {
            combo: combo.into(),
            what: vec!["a".into(), "b".into()],
        };
        let dups = [dup("Super+1"), dup("Super+2"), dup("Super+Q"), dup("Print")];
        assert_eq!(
            duplicates_line(&dups),
            "4 key combinations bound twice: Super+1, Super+2, Super+Q, Print"
        );
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
