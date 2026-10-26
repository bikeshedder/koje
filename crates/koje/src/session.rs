//! `koje session` (ADR 0048): the koje session from the login screen.
//! `start` runs Hyprland with koje's config and cleans up after it;
//! `ready` (from Hyprland's `exec-once`) hands the environment to systemd and
//! starts koje's services; `restart` restarts them. Decisions:
//! `koje_core::session`.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use koje_core::paths::Paths;
use koje_core::session::{self, Cmd};

use crate::cli::SessionCmd;

pub fn run(paths: &Paths, cmd: &SessionCmd) -> Result<()> {
    match cmd {
        SessionCmd::Start => start(paths),
        SessionCmd::Ready => run_all(&session::ready_commands()),
        SessionCmd::Restart => run_all(&session::restart_commands()),
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

/// The user's own settings, so Hyprland finds the file it sources.
fn ensure_user_conf(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    koje_core::fsutil::write_atomic(path, session::USER_CONF_TEMPLATE.as_bytes())?;
    Ok(())
}

fn start(paths: &Paths) -> Result<()> {
    ensure_user_conf(&paths.session_hyprland_conf())?;
    let given = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let mut hyprland = tokio::process::Command::new(session::HYPRLAND);
    hyprland
        .args(session::hyprland_args(Path::new(session::SESSION_CONF)))
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
