//! `koje setup` (ADR 0021, 0045, 0061): enable koje's GNOME Shell extension
//! and build the base image – the package installs everything else. Shows the
//! plan, asks once. Decisions: `koje_core::setup`.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::setup::{self, Action, Step};

use koje_runtime::prompt::confirm;

/// Command → Debian/Ubuntu package koje needs on the host.
pub const DEPENDENCIES: [(&str, &str); 5] = [
    ("podman", "podman"),
    ("newuidmap", "uidmap"),
    ("pasta", "passt"),
    ("fuse-overlayfs", "fuse-overlayfs"),
    ("xdg-dbus-proxy", "xdg-dbus-proxy"),
];

pub fn on_path(command: &str) -> bool {
    find_on_path(command).is_some()
}

/// Where `command` is in `PATH`: the first executable file of that name.
pub fn find_on_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Missing packages: commands not on `PATH`.
pub fn missing_packages() -> Vec<&'static str> {
    DEPENDENCIES
        .iter()
        .filter(|(cmd, _)| !on_path(cmd))
        .map(|(_, pkg)| *pkg)
        .collect()
}

/// How to install `missing` on this distribution.
pub fn install_hint(missing: &[&str]) -> String {
    let debian = std::fs::read_to_string("/etc/os-release").is_ok_and(|t| {
        t.lines().any(|l| {
            (l.starts_with("ID=") || l.starts_with("ID_LIKE="))
                && (l.contains("debian") || l.contains("ubuntu"))
        })
    });
    if debian {
        format!("sudo apt install {}", missing.join(" "))
    } else {
        "package names are Debian/Ubuntu's; use your distribution's equivalents".to_owned()
    }
}

/// GNOME's `enabled-extensions`.
fn gnome_enabled_extensions() -> Vec<String> {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| setup::parse_strv(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Enable koje's extension: through the running Shell, else in the settings
/// GNOME reads at the next login (a freshly installed extension).
fn enable_gnome_extension() -> Result<()> {
    let enabled = std::process::Command::new("gnome-extensions")
        .args(["enable", setup::GNOME_EXTENSION])
        .output()
        .is_ok_and(|o| o.status.success());
    if enabled {
        return Ok(());
    }
    let mut list = gnome_enabled_extensions();
    if !list.iter().any(|e| e == setup::GNOME_EXTENSION) {
        list.push(setup::GNOME_EXTENSION.to_owned());
    }
    let ok = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.shell",
            "enabled-extensions",
            &setup::format_strv(&list),
        ])
        .status()
        .context("running gsettings")?
        .success();
    if !ok {
        anyhow::bail!("gsettings could not enable {}", setup::GNOME_EXTENSION);
    }
    Ok(())
}

/// Everything setup would do, in order (the image last).
pub fn plan() -> Vec<Step> {
    vec![
        setup::gnome_extension(&gnome_enabled_extensions()),
        setup::image(image_exists()),
    ]
}

/// Whether koje's base image exists; `None` without Podman.
fn image_exists() -> Option<bool> {
    on_path("podman").then(|| {
        koje_podman::Podman::new()
            .image_exists(koje_podman::image::BASE_IMAGE)
            .unwrap_or(false)
    })
}

fn execute(action: &Action) -> Result<()> {
    match action {
        Action::Nothing => {}
        Action::EnableGnomeExtension => enable_gnome_extension()?,
        Action::BuildImage => {
            // Last step; a failure does not undo the rest.
            if let Err(e) = crate::commands::image_build(false) {
                eprintln!(
                    "warning: the base image was not built ({e:#}) – retry with: koje image build"
                );
            }
        }
    }
    Ok(())
}

pub fn run(dry_run: bool, yes: bool) -> Result<()> {
    println!("Checking dependencies");
    let missing = missing_packages();
    if missing.is_empty() {
        println!("  all found");
    } else {
        println!("  missing: {}", missing.join(" "));
        println!("  install with:  {}", install_hint(&missing));
    }

    let steps = plan();
    println!("\nPlan  (+ new  = unchanged  - skipped)");
    for s in &steps {
        println!("  {s}");
    }
    let restart: Vec<&str> = setup::UNITS
        .iter()
        .filter(|(unit, binary)| main_exe(unit).is_some_and(|exe| setup::outdated(&exe, binary)))
        .map(|(unit, _)| *unit)
        .collect();
    for unit in &restart {
        println!("  then: restart {unit} (it runs an old binary)");
    }
    if dry_run {
        println!("\ndry run – nothing changed");
        return Ok(());
    }
    if !steps.iter().any(|s| s.mark.changes()) && restart.is_empty() {
        println!("\nnothing to do");
        return Ok(());
    }
    if !yes && !confirm("\nContinue?", false)? {
        anyhow::bail!("aborted – nothing changed");
    }

    println!();
    for s in steps.iter().filter(|s| s.mark.changes()) {
        execute(&s.action)?;
        println!("  done: {}", s.what);
    }
    for unit in restart {
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "restart", unit])
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            println!("  done: restart {unit}");
        } else {
            eprintln!("warning: could not restart {unit}");
        }
    }
    println!("\nNext\n  check everything: koje doctor");
    Ok(())
}

/// The executable of a running unit's main process.
fn main_exe(unit: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", "--property=MainPID", "--value", unit])
        .output()
        .ok()?;
    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    if pid == 0 {
        return None; // not running
    }
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}
