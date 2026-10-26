//! `koje setup` (ADR 0021, 0045): install koje's per-user files – the package
//! installs the rest – and remove what the old `install.sh` left in
//! `~/.local`. Shows the plan, warns about files it changes, asks once. The
//! files are embedded, so the binary needs no checkout. Decisions:
//! `koje_core::setup`.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use koje_core::paths::Paths;
use koje_core::setup::{self, Action, Mark, Refresh, Service, Step, WaybarFile};

use crate::prompt::confirm;

const SNIPPET: &[u8] = include_bytes!("../../../assets/hyprland/koje.conf");
const HYPRBARS: &[u8] = include_bytes!("../../../assets/hyprland/hyprbars.conf");
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
pub const DEPENDENCIES: [(&str, &str); 18] = [
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
    // The session koje's Hyprland snippet sets up (ADR 0037).
    ("foot", "foot"),
    ("mako", "mako-notifier"),
    ("hypridle", "hypridle"),
    ("hyprlock", "hyprlock"),
    ("grim", "grim"),
    ("slurp", "slurp"),
    ("wl-copy", "wl-clipboard"),
    ("wpctl", "wireplumber"),
];
/// Portal backends (packages, checked with `dpkg-query` where it exists).
pub const PORTALS: [&str; 2] = ["xdg-desktop-portal-hyprland", "xdg-desktop-portal-gtk"];

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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let mut steps = vec![
        file(
            "Hyprland snippet",
            &paths.hyprland_snippet(),
            SNIPPET,
            0o644,
        ),
        file(
            "title bar snippet (scripts/setup-hyprbars.sh)",
            &paths.hyprbars_snippet(),
            HYPRBARS,
            0o644,
        ),
    ];
    let conf = paths.hyprland_conf();
    let text = std::fs::read_to_string(&conf).ok();
    steps.push(setup::hyprland_source(
        &conf,
        &paths.hyprland_snippet(),
        &home,
        text.as_deref(),
        &setup::system_keyboard(
            std::fs::read_to_string("/etc/default/keyboard")
                .ok()
                .as_deref(),
        ),
    ));
    let waybar = paths.waybar_dir();
    let present: Vec<(&str, WaybarFile)> = WAYBAR_READS
        .iter()
        .map(|f| (*f, waybar_state(&waybar.join(f))))
        .collect();
    let colors_present = waybar.join(setup::WAYBAR_COLORS).exists();
    steps.extend(setup::waybar(&waybar, &WAYBAR, &present, colors_present));
    let unit = paths.kojed_unit();
    for (label, path) in setup::old_install(&home, &paths.font_dir(), &unit) {
        steps.extend(setup::remove_old(
            label,
            &path,
            read(&path).as_deref(),
            path == unit,
        ));
    }
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
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .with_context(|| format!("opening {}", path.display()))?;
            f.write_all(format!("\n# added by koje setup\n{line}\n").as_bytes())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        Action::HyprlandConf { path, contents } => {
            if path.exists() {
                let mut backup = path.as_os_str().to_owned();
                backup.push(".bak");
                std::fs::copy(path, &backup)
                    .with_context(|| format!("keeping {} as .bak", path.display()))?;
            } else if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            koje_core::fsutil::write_atomic(path, contents.as_bytes())?;
        }
        Action::Remove { path } => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
            // The old folders go too once empty (`hyprbars.so` keeps its own).
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir);
            }
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
    println!("\nPlan  (+ new  ! overwrite  ~ append  x remove  = unchanged  - skipped)");
    for s in &steps {
        println!("  {}", tilde(&s.to_string()));
    }
    let warnings = steps.iter().filter(|s| s.mark.warns()).count();
    if warnings > 0 {
        println!(
            "\n  {warnings} existing file(s) will be overwritten, changed or removed (marked !, ~ and x)."
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
    let mut refresh = if in_hyprland() {
        setup::refresh(
            &steps,
            &paths.hyprland_snippet(),
            &paths.waybar_dir(),
            &running,
        )
    } else {
        Vec::new()
    };
    if kojed_exe().is_some_and(|exe| setup::kojed_outdated(&exe)) {
        refresh.push(Refresh::Kojed);
    }
    if !refresh.is_empty() {
        println!("\nThen");
        for r in &refresh {
            println!("  {r}");
        }
    }
    if dry_run {
        println!("\ndry run – nothing changed");
        return Ok(());
    }
    if !steps.iter().any(|s| s.mark.changes()) && refresh.is_empty() {
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
    let removed = |p: &Path| {
        steps
            .iter()
            .any(|s| matches!(&s.action, Action::Remove { path } if path.starts_with(p)))
    };
    if removed(&paths.font_dir()) && on_path("fc-cache") {
        let _ = std::process::Command::new("fc-cache").output();
    }
    if removed(&paths.kojed_unit()) {
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("warning: systemctl --user daemon-reload failed");
        }
    }

    if !refresh.is_empty() {
        println!();
        for r in &refresh {
            match refresh_session(r) {
                Ok(()) => println!("  done: {r}"),
                Err(e) => eprintln!("warning: could not {r}: {e:#}"),
            }
        }
    }

    println!("\nNext");
    if !in_hyprland() {
        println!("  in Hyprland: hyprctl reload, or log in again to start koje's session");
    }
    println!("  check everything: koje doctor");
    Ok(())
}

/// The executable of the running kojed (its unit's main process).
fn kojed_exe() -> Option<PathBuf> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", "--property=MainPID", "--value", "kojed"])
        .output()
        .ok()?;
    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    if pid == 0 {
        return None; // not running
    }
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// Setup runs inside a Hyprland session it can refresh.
fn in_hyprland() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|s| !s.is_empty())
        && on_path("hyprctl")
}

fn running(service: &Service) -> bool {
    std::process::Command::new("pgrep")
        .args(service.pgrep)
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn refresh_session(r: &Refresh) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = match r {
        Refresh::Hyprland => ("hyprctl", vec!["reload"]),
        Refresh::Waybar => ("pkill", vec!["-USR2", "-x", "waybar"]),
        // Started by Hyprland, so it gets the session's environment as at login.
        Refresh::Start(s) => ("hyprctl", vec!["dispatch", "exec", s.command]),
        Refresh::Kojed => ("systemctl", vec!["--user", "restart", "kojed"]),
    };
    let out = std::process::Command::new(program)
        .args(&args)
        .output()
        .with_context(|| format!("running {program}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // hyprctl exits 0 on refused commands; it answers "ok".
    if !out.status.success() || (program == "hyprctl" && stdout.trim() != "ok") {
        anyhow::bail!(
            "{program}: {}{}",
            stdout.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
