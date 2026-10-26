//! `koje session` (ADR 0048): the koje session from the login screen.
//! `start` runs Hyprland with koje's config and cleans up after it;
//! `ready` (from Hyprland's `exec-once`) hands the environment to systemd and
//! starts koje's services; `restart` restarts them. Decisions:
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
        SessionCmd::Restart => run_all(&session::restart_commands()),
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
fn write_session_conf(paths: &Paths) -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set – no login session?")?;
    let dir = runtime.join("koje");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
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
    let conf = session::session_conf(
        Path::new(session::SNIPPET),
        Some(&monitors),
        user.is_file().then_some(user.as_path()),
    );
    let path = dir.join("session.conf");
    koje_core::fsutil::write_atomic(&path, conf.as_bytes())?;
    Ok(path)
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
    let conf = write_session_conf(paths)?;
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
        .envs(environment_d())
        .envs(session::session_environment(given.as_deref()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the session's runtime")?;
    let status = runtime.block_on(supervise(hyprland));
    // Whatever happened: no services and no environment of a gone session.
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
