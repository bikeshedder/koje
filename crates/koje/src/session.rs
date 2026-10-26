//! `koje session` (ADR 0048): the koje session from the login screen.
//! `start` runs Hyprland with koje's config and cleans up after it;
//! `ready` (from Hyprland's `exec-once`) hands the environment to systemd and
//! starts koje's services; `restart` restarts them and loads koje's
//! Hyprland plugin again. Decisions:
//! `koje_core::session`.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use koje_core::paths::Paths;
use koje_core::session::{self, Cmd};

use crate::cli::{EndKind, SessionCmd};

pub fn run(paths: &Paths, cmd: &SessionCmd) -> Result<()> {
    match cmd {
        SessionCmd::Start => start(paths),
        SessionCmd::Ready => run_all(&session::ready_commands()),
        SessionCmd::Restart => restart(),
        SessionCmd::End { what } => end(*what),
    }
}

/// The shell asks first (its `GApplication` action, M8.47); without the
/// shell, logging out ends Hyprland at once – restart and power off do not
/// happen unasked.
fn end(what: EndKind) -> Result<()> {
    let action = match what {
        EndKind::LogOut => "log-out",
        EndKind::Restart => "restart",
        EndKind::PowerOff => "power-off",
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the D-Bus client")?;
    let asked = runtime.block_on(async {
        let conn = zbus::Connection::session().await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        if !dbus.name_has_owner("io.koje.Shell".try_into()?).await? {
            return anyhow::Ok(false);
        }
        let none: Vec<zbus::zvariant::Value<'_>> = Vec::new();
        let platform: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
            std::collections::HashMap::new();
        conn.call_method(
            Some("io.koje.Shell"),
            "/io/koje/Shell",
            Some("org.gtk.Actions"),
            "Activate",
            &(action, none, platform),
        )
        .await
        .context("asking koje-shell")?;
        anyhow::Ok(true)
    });
    match (asked, what) {
        (Ok(true), _) => Ok(()),
        (_, EndKind::LogOut) => {
            runtime.block_on(koje_hyprland::Hyprland::from_env().request("dispatch exit"))?;
            Ok(())
        }
        (Err(e), _) => Err(e),
        (Ok(false), _) => bail!("koje-shell is not running – it asks before {action}"),
    }
}

/// Run each command; the first failure stops.
/// koje's services and its Hyprland plugin as installed now (after a
/// package update). The plugin first: the shell asks for it when it starts.
pub fn restart() -> Result<()> {
    reload_plugin();
    run_all(&session::restart_commands())
}

/// A plugin Hyprland refuses (built for another version) is reported; the
/// services restart anyway.
fn reload_plugin() {
    if !Path::new(session::PLUGIN).is_file()
        || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty())
    {
        return;
    }
    let loaded = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
        .and_then(|rt| {
            rt.block_on(koje_hyprland::Hyprland::from_env().request("j/plugin list"))
                .ok()
        })
        .is_some_and(|reply| crate::doctor::plugin_listed(&reply));
    for (program, args) in session::plugin_reload_commands(loaded) {
        let out = std::process::Command::new(program).args(&args).output();
        // hyprctl exits 0 on refused commands; it answers "ok".
        match out {
            Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "ok" => {}
            Ok(o) => {
                eprintln!(
                    "warning: {program} {}: {}{}",
                    args.join(" "),
                    String::from_utf8_lossy(&o.stdout).trim(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                return;
            }
            Err(e) => {
                eprintln!("warning: running {program}: {e}");
                return;
            }
        }
    }
}

fn run_all(cmds: &[Cmd]) -> Result<()> {
    for (program, args) in cmds {
        let status = std::process::Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("running {program}"))?;
        if !status.success() {
            bail!("{program} {}: {status}", args.join(" "));
        }
    }
    Ok(())
}

/// The session's Hyprland config in the runtime directory: koje's snippet,
/// the screens from `monitors.xml` and the user's file if there is one
/// (ADR 0046, 0048). Its path.
fn write_session_conf(paths: &Paths, dir: &Path) -> Result<PathBuf> {
    // The screens from GNOME's monitors.xml (ADR 0046), before Hyprland
    // starts: the right mode from the first frame.
    let (lines, source) = crate::screens::lines(paths, &crate::screens::connected());
    if let crate::screens::Source::None(why) = &source {
        eprintln!("note: screens: Hyprland's defaults ({why})");
    }
    let monitors = dir.join("monitors.conf");
    koje_core::fsutil::write_atomic(
        &monitors,
        koje_core::monitors::hyprland_conf(&lines).as_bytes(),
    )?;
    let user = paths.session_hyprland_conf();
    // The plugin (ADR 0051) when the package has it; Hyprland refuses one
    // built for another version itself, with a notification.
    let plugin = Path::new(session::PLUGIN);
    let conf = session::session_conf(
        plugin.is_file().then_some(plugin),
        Path::new(session::SNIPPET),
        Some(&monitors),
        user.is_file().then_some(user.as_path()),
    );
    let path = dir.join("session.conf");
    koje_core::fsutil::write_atomic(&path, conf.as_bytes())?;
    Ok(path)
}

/// koje's runtime directory for the session's generated files.
fn runtime_dir() -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set – no login session?")?;
    let dir = runtime.join("koje");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// xdph's config with koje's screen sharing picker and the environment file
/// that points xdph at it (ADR 0050); `user_config_home` is the session's
/// own `XDG_CONFIG_HOME`.
fn write_xdph(paths: &Paths, dir: &Path, user_config_home: Option<&str>) -> Result<()> {
    let config_home = dir.join(session::XDPH_DIR);
    let hypr = config_home.join("hypr");
    std::fs::create_dir_all(&hypr).with_context(|| format!("creating {}", hypr.display()))?;
    let user_file = paths.session_xdph_conf();
    let user = match std::fs::read_to_string(&user_file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", user_file.display())),
    };
    let conf = session::xdph_conf(session::SHARE_PICKER, user.as_deref());
    koje_core::fsutil::write_atomic(&hypr.join("xdph.conf"), conf.as_bytes())?;
    koje_core::fsutil::write_atomic(
        &dir.join(session::XDPH_ENV),
        session::xdph_env(&config_home, user_config_home).as_bytes(),
    )?;
    Ok(())
}

/// Take xdph back to the user's own config.
fn remove_xdph(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(session::XDPH_ENV));
    let _ = std::fs::remove_dir_all(dir.join(session::XDPH_DIR));
}

/// The environment with the user's `environment.d` applied (ADR 0048);
/// empty – the inherited one stays – without systemd's generator.
fn environment_d() -> Vec<(String, String)> {
    match std::process::Command::new(session::ENVIRONMENT_D).output() {
        Ok(out) if out.status.success() => {
            session::parse_environment(&String::from_utf8_lossy(&out.stdout))
        }
        Ok(out) => {
            eprintln!(
                "warning: environment.d: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

fn start(paths: &Paths) -> Result<()> {
    let dir = runtime_dir()?;
    let conf = write_session_conf(paths, &dir)?;
    let environment_d = environment_d();
    let user_config_home = environment_d
        .iter()
        .find(|(k, _)| k == "XDG_CONFIG_HOME")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("XDG_CONFIG_HOME").ok());
    // Without it sharing still works, with xdph's own picker.
    if let Err(e) = write_xdph(paths, &dir, user_config_home.as_deref()) {
        eprintln!("warning: screen sharing picker: {e:#}");
    }
    let given = std::env::var("XDG_CURRENT_DESKTOP").ok();
    // Through Hyprland's watchdog when there is one; found in the session's
    // own PATH: `environment.d` may set another.
    let watchdog = crate::setup::find_on_path(session::WATCHDOG);
    let program = watchdog.clone().unwrap_or_else(|| {
        crate::setup::find_on_path(session::HYPRLAND)
            .unwrap_or_else(|| PathBuf::from(session::HYPRLAND))
    });
    let mut hyprland = tokio::process::Command::new(program);
    // `environment.d` first (e.g. GPU variables), the session's own over it.
    hyprland
        .args(session::hyprland_args(&conf, watchdog.is_some()))
        .envs(environment_d)
        .envs(session::session_environment(given.as_deref()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the session's runtime")?;
    let status = runtime.block_on(supervise(hyprland));
    // Whatever happened: no services and no environment of a gone session.
    remove_xdph(&dir);
    for (program, args) in session::end_commands() {
        let ok = std::process::Command::new(program)
            .args(&args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("warning: {program} {} failed", args.join(" "));
        }
    }
    let status = status?;
    if !status.success() {
        bail!("Hyprland exited: {status}");
    }
    Ok(())
}

/// Run Hyprland until it exits. A display manager ending the session sends
/// SIGTERM (or SIGHUP): Hyprland gets it too, so it ends its clients in order.
async fn supervise(mut hyprland: tokio::process::Command) -> Result<std::process::ExitStatus> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut child = hyprland
        .spawn()
        .with_context(|| format!("starting {}", session::HYPRLAND))?;
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    let mut int = signal(SignalKind::interrupt())?;
    loop {
        tokio::select! {
            status = child.wait() => return status.context("waiting for Hyprland"),
            _ = term.recv() => forward(&child),
            _ = hup.recv() => forward(&child),
            _ = int.recv() => forward(&child),
        }
    }
}

fn forward(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}
