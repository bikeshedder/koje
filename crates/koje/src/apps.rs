//! `koje apps` (M8.17): the programs of the active context, and starting
//! one. `koje favorites`: the dash's favorites of a context (ADR 0053).

use anyhow::{Result, bail};
use koje_core::apps::{LIST_SCRIPT, parse_listing};
use koje_core::context;
use koje_core::lifecycle::require_running;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::ContainerRuntime as _;
use koje_podman::Podman;

use crate::cli::ActiveCtx;

pub fn run(paths: &Paths, ctx: &ActiveCtx, start: Option<&str>) -> Result<()> {
    let name = ContextName::new(&crate::kojed::context_or_active(ctx)?)?;
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    let apps = parse_listing(&podman.query(&name, &["sh", "-c", LIST_SCRIPT])?);
    let Some(wanted) = start else {
        for app in &apps {
            println!("{}", app.name);
        }
        return Ok(());
    };
    // The name as listed, else the desktop file id.
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
    koje_runtime::runtime::run(paths, name.as_str(), &command, ctx.context.is_some(), None)
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
    let name = ContextName::new(&crate::kojed::context_or_active(ctx)?)?;
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
