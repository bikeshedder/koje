//! `koje apps` (M8.17): the programs of the active context for koje's
//! application menu (`assets/bin/koje-app-menu`, Super+D) and, as JSON, for
//! the shell's search and app grid (M8.70); and starting one.

use anyhow::{Result, bail};
use koje_core::apps::{LIST_SCRIPT, parse_listing};
use koje_core::lifecycle::require_running;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::ContainerRuntime as _;
use koje_podman::Podman;

use crate::cli::ActiveCtx;

pub fn run(
    paths: &Paths,
    ctx: &ActiveCtx,
    start: Option<&str>,
    menu: bool,
    json: bool,
) -> Result<()> {
    let name = ContextName::new(&crate::layout::ctx(ctx)?)?;
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    let apps = parse_listing(&podman.query(&name, &["sh", "-c", LIST_SCRIPT])?);
    let Some(wanted) = start else {
        if json {
            let pid = podman.running_pid(&name)?;
            println!("{}", serde_json::to_string_pretty(&listing(&apps, pid))?);
            return Ok(());
        }
        if menu {
            println!("{name}  {}", crate::memory::summary(paths, name.as_str())?);
        }
        for app in &apps {
            println!("{}", app.name);
        }
        return Ok(());
    };
    // The name as listed, else the desktop file id (the shell's dash).
    let Some(app) = apps
        .iter()
        .find(|a| a.name == wanted)
        .or_else(|| apps.iter().find(|a| a.id == wanted))
    else {
        bail!("{name} has no program named {wanted:?} – see: koje apps -c {name}");
    };
    let mut command = app.command.clone();
    if app.terminal {
        command.insert(0, crate::terminal::TERMINAL.to_owned());
    }
    // The active context's workspace is in view already.
    crate::runtime::run(paths, name.as_str(), &command, ctx.context.is_some())
}

/// The programs for the shell: name, desktop file id, icon – a file of the
/// context as `/proc/<pid>/root/…` when it has one, else the theme name –,
/// search keywords and whether it runs in a terminal.
fn listing(apps: &[koje_core::apps::App], pid: Option<u32>) -> serde_json::Value {
    let icon = |name: &str| -> String {
        let found = pid.and_then(|pid| {
            koje_core::apps::icon_candidates(name)
                .into_iter()
                .map(|p| format!("/proc/{pid}/root{p}"))
                .find(|p| std::path::Path::new(p).is_file())
        });
        found.unwrap_or_else(|| name.to_owned())
    };
    apps.iter()
        .map(|a| {
            serde_json::json!({
                "name": a.name,
                "id": a.id,
                "icon": a.icon.as_deref().map(icon),
                "keywords": a.keywords,
                "terminal": a.terminal,
            })
        })
        .collect()
}
