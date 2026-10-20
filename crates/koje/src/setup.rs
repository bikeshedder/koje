//! `koje setup` (ADR 0021): install koje's files for the current user. Shows
//! the plan, warns about files it changes, asks once. The files are embedded,
//! so the installed binary needs no checkout. Decisions: `koje_core::setup`.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use koje_core::paths::Paths;
use koje_core::setup::{self, Action, Mark, Step, WaybarFile};

use crate::prompt::confirm;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/lucide/lucide.ttf");
const FONT_LICENSE: &[u8] = include_bytes!("../../../assets/fonts/lucide/LICENSE");
const SNIPPET: &[u8] = include_bytes!("../../../assets/hyprland/koje.conf");
const PICKER: &[u8] = include_bytes!("../../../assets/bin/koje-window-picker");
const UNIT: &[u8] = include_bytes!("../../../assets/systemd/kojed.service");
const WAYBAR: [(&str, &[u8]); 2] = [
    (
        "config.jsonc",
        include_bytes!("../../../assets/waybar/config.jsonc"),
    ),
    (
        "style.css",
        include_bytes!("../../../assets/waybar/style.css"),
    ),
];
/// Files Waybar reads; any of them not koje's means the user has an own config.
const WAYBAR_READS: [&str; 3] = ["config", "config.jsonc", "style.css"];

/// Command → Debian/Ubuntu package koje needs on the host.
pub const DEPENDENCIES: [(&str, &str); 10] = [
    ("podman", "podman"),
    ("newuidmap", "uidmap"),
    ("pasta", "passt"),
    ("fuse-overlayfs", "fuse-overlayfs"),
    ("Hyprland", "hyprland"),
    ("waybar", "waybar"),
    ("fc-cache", "fontconfig"),
    ("xdg-dbus-proxy", "xdg-dbus-proxy"),
    ("fuzzel", "fuzzel"),
    ("jq", "jq"),
];
/// Portal backends (packages, checked with `dpkg-query` where it exists).
pub const PORTALS: [&str; 2] = ["xdg-desktop-portal-hyprland", "xdg-desktop-portal-gtk"];

/// `~/.local/lib/koje` (the snippet refers to the picker there).
pub fn lib_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/lib/koje"))
}

pub fn on_path(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            std::fs::metadata(dir.join(command))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// Missing packages: commands not on `PATH`, portals not installed.
pub fn missing_packages() -> Vec<&'static str> {
    let mut missing: Vec<&str> = DEPENDENCIES
        .iter()
        .filter(|(cmd, _)| !on_path(cmd))
        .map(|(_, pkg)| *pkg)
        .collect();
    if on_path("dpkg-query") {
        for p in PORTALS {
            let installed = std::process::Command::new("dpkg-query")
                .args(["-W", "-f=${Status}", p])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("install ok installed"));
            if !installed {
                missing.push(p);
            }
        }
    }
    missing
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

fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn waybar_state(path: &Path) -> WaybarFile {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return WaybarFile::Missing;
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path).unwrap_or_default();
        // A link to the M2.1 spike draft is koje's.
        if target
            .to_string_lossy()
            .contains("/docs/spikes/setup/waybar/")
        {
            return WaybarFile::Koje(Vec::new());
        }
    }
    match read(path) {
        Some(c) if setup::is_koje_waybar(&c) => WaybarFile::Koje(c),
        _ => WaybarFile::Foreign,
    }
}

/// Everything setup would do, in order (the image last).
pub fn plan(paths: &Paths) -> Result<Vec<Step>> {
    let lib = lib_dir().context("HOME is not set")?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let fonts = paths.font_dir();
    let mut steps = vec![
        file("icon font", &fonts.join("lucide.ttf"), FONT, 0o644),
        file(
            "icon font license",
            &fonts.join("LICENSE"),
            FONT_LICENSE,
            0o644,
        ),
        file(
            "Hyprland snippet",
            &paths.hyprland_snippet(),
            SNIPPET,
            0o644,
        ),
        file("systemd user unit", &paths.kojed_unit(), UNIT, 0o644),
        file(
            "window picker (Super+Tab)",
            &lib.join("koje-window-picker"),
            PICKER,
            0o755,
        ),
    ];
    let conf = paths.hyprland_conf();
    let text = std::fs::read_to_string(&conf).ok();
    steps.push(setup::hyprland_source(
        &conf,
        &paths.hyprland_snippet(),
        &home,
        text.as_deref(),
    ));
    let waybar = paths.waybar_dir();
    let present: Vec<(&str, WaybarFile)> = WAYBAR_READS
        .iter()
        .map(|f| (*f, waybar_state(&waybar.join(f))))
        .collect();
    steps.extend(setup::waybar(&waybar, &WAYBAR, &present));
    let image = on_path("podman").then(|| {
        koje_podman::Podman::new()
            .image_exists(koje_podman::image::BASE_IMAGE)
            .unwrap_or(false)
    });
    steps.push(setup::image(image));
    Ok(steps)
}

fn file(label: &str, path: &Path, contents: &'static [u8], mode: u32) -> Step {
    setup::file(label, path, contents, mode, read(path).as_deref())
}

fn execute(action: &Action) -> Result<()> {
    match action {
        Action::Nothing => {}
        Action::Write {
            path,
            contents,
            mode,
        } => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            koje_core::fsutil::write_atomic(path, contents)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode))
                .with_context(|| format!("setting the mode of {}", path.display()))?;
        }
        Action::SourceLine { path, line } => {
            use std::io::Write as _;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            let new = !path.exists();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening {}", path.display()))?;
            let text = if new {
                format!(
                    "# Hyprland config – your own settings go here.\n\n# added by koje setup\n{line}\n"
                )
            } else {
                format!("\n# added by koje setup\n{line}\n")
            };
            f.write_all(text.as_bytes())
                .with_context(|| format!("writing {}", path.display()))?;
        }
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

pub fn tilde(s: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => s.replace(&home, "~"),
        _ => s.to_owned(),
    }
}

pub fn run(paths: &Paths, dry_run: bool, yes: bool) -> Result<()> {
    println!("Checking dependencies");
    let missing = missing_packages();
    if missing.is_empty() {
        println!("  all found");
    } else {
        println!("  missing: {}", missing.join(" "));
        println!("  install with:  {}", install_hint(&missing));
    }

    let steps = plan(paths)?;
    println!("\nPlan  (+ new  ! overwrite  ~ append  = unchanged  - skipped)");
    for s in &steps {
        println!("  {}", tilde(&s.to_string()));
    }
    let warnings = steps.iter().filter(|s| s.mark.warns()).count();
    if warnings > 0 {
        println!(
            "\n  {warnings} existing file(s) will be overwritten or changed (marked ! and ~)."
        );
    }
    if steps
        .iter()
        .any(|s| s.mark == Mark::Skipped && s.what.starts_with("Waybar"))
    {
        println!(
            "  To use koje's bar instead: move your Waybar config away and run koje setup again."
        );
    }
    if dry_run {
        println!("\ndry run – nothing changed");
        return Ok(());
    }
    if !steps.iter().any(|s| s.mark.changes()) {
        println!("\nnothing to do");
        return Ok(());
    }
    if !yes && !confirm("\nContinue?", false)? {
        anyhow::bail!("aborted – nothing changed");
    }

    println!();
    for s in steps.iter().filter(|s| s.mark.changes()) {
        execute(&s.action)?;
        println!("  done: {}", tilde(&s.what));
    }
    let wrote = |p: &Path| {
        steps
            .iter()
            .any(|s| matches!(&s.action, Action::Write { path, .. } if path.starts_with(p)))
    };
    if wrote(&paths.font_dir()) && on_path("fc-cache") {
        let _ = std::process::Command::new("fc-cache")
            .arg("-f")
            .arg(paths.font_dir())
            .output();
    }
    if wrote(&paths.kojed_unit()) {
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("warning: systemctl --user daemon-reload failed");
        }
    }

    println!("\nNext");
    println!("  in Hyprland: hyprctl reload; restart Waybar: pkill waybar; waybar &");
    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "kojed"])
        .status()
        .is_ok_and(|s| s.success());
    if !active {
        println!("  start kojed (Hyprland does it at login): systemctl --user start kojed");
    }
    println!("  check everything: koje doctor");
    Ok(())
}
