//! The filtered host bus of a running context (ADR 0019, 0025): the proxy as a
//! transient systemd user unit on the host, the forwarder inside the context.
//! Rules and names come from `koje_core::hostbus`.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use koje_core::access::Access;
use koje_core::hostbus::{forwarded_names, proxy_rules};
use koje_core::name::ContextName;
use koje_core::spec::FORWARDER_IN_CONTEXT;
use koje_podman::Podman;

/// `koje-proxy-<context>`
pub fn unit_name(ctx: &ContextName) -> String {
    format!("koje-proxy-{ctx}")
}

/// `$XDG_RUNTIME_DIR/koje/<context>/host-bus`, mounted into the context.
pub fn dir(ctx: &ContextName) -> Result<PathBuf> {
    let run = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(run)
        .join("koje")
        .join(ctx.as_str())
        .join("host-bus"))
}

/// The static forwarder on the host: `$KOJE_FORWARD`, else the one `install.sh`
/// put into `~/.local/lib/koje/`.
pub fn forwarder_binary() -> Option<PathBuf> {
    let path = std::env::var_os("KOJE_FORWARD")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/lib/koje/koje-forward"))
        })?;
    path.is_file().then_some(path)
}

fn run(program: &str, args: &[&str]) -> Result<std::process::Output> {
    Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}"))
}

/// Start the proxy unit unless it runs; returns the directory with its socket.
pub fn start_proxy(ctx: &ContextName, access: &Access) -> Result<PathBuf> {
    let dir = dir(ctx)?;
    let socket = dir.join("bus");
    let unit = unit_name(ctx);
    if run("systemctl", &["--user", "is-active", "--quiet", &unit])?
        .status
        .success()
        && socket.exists()
    {
        return Ok(dir);
    }
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .context("no session bus (DBUS_SESSION_BUS_ADDRESS is not set)")?;
    if run("sh", &["-c", "command -v xdg-dbus-proxy"])?
        .stdout
        .is_empty()
    {
        bail!("xdg-dbus-proxy is not installed (Debian/Ubuntu: sudo apt install xdg-dbus-proxy)");
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let _ = std::fs::remove_file(&socket);
    let socket_arg = socket.display().to_string();
    let unit_arg = format!("--unit={unit}");
    let mut args = vec![
        "--user",
        &unit_arg,
        "--collect",
        "--quiet",
        "--property=Restart=on-failure",
        "--",
        "xdg-dbus-proxy",
        &bus,
        &socket_arg,
    ];
    let rules = proxy_rules(access);
    args.extend(rules.iter().map(String::as_str));
    let out = run("systemd-run", &args)?;
    if !out.status.success() {
        bail!(
            "starting {unit} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    wait_for(
        || socket.exists(),
        &format!("the proxy socket {}", socket.display()),
    )?;
    Ok(dir)
}

/// Stop the proxy unit; fine if it is not running.
pub fn stop_proxy(ctx: &ContextName) {
    let _ = run("systemctl", &["--user", "stop", &unit_name(ctx)]);
}

/// Start the forwarder in the context unless it already owns its names.
pub fn start_forwarder(
    podman: &Podman,
    ctx: &ContextName,
    access: &Access,
    uid: u32,
) -> Result<()> {
    let names = forwarded_names(access);
    if owns_all(podman, ctx, &names) {
        return Ok(());
    }
    let host = format!("unix:path=/run/user/{uid}/koje-host/bus");
    let mut command = vec![FORWARDER_IN_CONTEXT, "--host", &host];
    command.extend(names.iter().copied());
    podman.exec_detached(ctx, &command)?;
    wait_for(|| owns_all(podman, ctx, &names), "the forwarder")
}

/// Whether all `names` have an owner on the context bus.
fn owns_all(podman: &Podman, ctx: &ContextName, names: &[&str]) -> bool {
    let list = podman.exec_output(
        ctx,
        &[
            "dbus-send",
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.ListNames",
        ],
    );
    list.is_ok_and(|text| names.iter().all(|n| text.contains(&format!("\"{n}\""))))
}

fn wait_for(mut ready: impl FnMut() -> bool, what: &str) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if ready() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("{what} did not come up within 5 s")
}
