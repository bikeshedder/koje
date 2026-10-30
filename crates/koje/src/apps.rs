//! `koje apps` (M8.17): the programs of the active context for koje's
//! application menu (`assets/bin/koje-app-menu`, Super+D) and, as JSON, for
//! the shell's search and app grid (M8.70); and starting one. `koje
//! favorites`: the dash's favorites of a context (ADR 0053).

use anyhow::{Result, bail};
use koje_core::apps::{LIST_SCRIPT, parse_listing};
use koje_core::context;
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
    crate::runtime::run(paths, name.as_str(), &command, ctx.context.is_some(), None)
}

/// The programs for the shell: name, desktop file id, icon – a file of the
/// context as `/proc/<pid>/root/…` when it has one, else the theme name –,
/// search keywords, whether it runs in a terminal, its windows' app id
/// (`StartupWMClass`) and its program's file name.
fn listing(apps: &[koje_core::apps::App], pid: Option<u32>) -> serde_json::Value {
    let icon = |name: &str| -> String {
        match pid {
            Some(pid) => koje_core::apps::host_icon(name, &format!("/proc/{pid}/root"), |p| {
                std::path::Path::new(p).is_file()
            }),
            None => name.to_owned(),
        }
    };
    apps.iter()
        .map(|a| {
            serde_json::json!({
                "name": a.name,
                "id": a.id,
                "icon": a.icon.as_deref().map(icon),
                "keywords": a.keywords,
                "terminal": a.terminal,
                "wm_class": a.wm_class,
                // The program, as the dash matches windows and favorites.
                "binary": a.command.first().and_then(|c| c.rsplit('/').next()),
            })
        })
        .collect()
}

/// List a context's favorites, or set them (`--set`) or remove them
/// (`--reset`). Its programs are not checked: a stopped context's are not
/// known, and the dash leaves out what it does not find.
pub fn favorites(
    paths: &Paths,
    ctx: &ActiveCtx,
    set: Option<&[String]>,
    reset: bool,
) -> Result<()> {
    let name = ContextName::new(&crate::layout::ctx(ctx)?)?;
    let cfg = if reset {
        context::set_favorites(paths, &name, None)?
    } else if let Some(ids) = set {
        context::set_favorites(paths, &name, Some(ids))?
    } else {
        context::load(paths, &name)?
    };
    match cfg.favorites {
        Some(ids) => {
            for id in ids {
                println!("{id}");
            }
        }
        None => println!("(GNOME's favorites, as far as {name} has them)"),
    }
    Ok(())
}
