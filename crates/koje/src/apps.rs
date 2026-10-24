//! `koje apps` (M8.17): the programs of the active context for koje's
//! application menu (`assets/bin/koje-app-menu`, Super+D), and starting one.

use anyhow::{Result, bail};
use koje_core::apps::{LIST_SCRIPT, parse_listing};
use koje_core::lifecycle::require_running;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::ContainerRuntime as _;
use koje_podman::Podman;

use crate::cli::ActiveCtx;

pub fn run(paths: &Paths, ctx: &ActiveCtx, start: Option<&str>, menu: bool) -> Result<()> {
    let name = ContextName::new(&crate::layout::ctx(ctx)?)?;
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    let apps = parse_listing(&podman.query(&name, &["sh", "-c", LIST_SCRIPT])?);
    let Some(wanted) = start else {
        if menu {
            println!("{name}  {}", crate::memory::summary(paths, name.as_str())?);
        }
        for app in &apps {
            println!("{}", app.name);
        }
        return Ok(());
    };
    let Some(app) = apps.iter().find(|a| a.name == wanted) else {
        bail!("{name} has no program named {wanted:?} – see: koje apps -c {name}");
    };
    // gio follows the desktop file (field codes, D-Bus activation); the
    // active context's workspace is in view already.
    let command = ["gio", "launch", app.path.as_str()].map(str::to_owned);
    crate::runtime::run(paths, name.as_str(), &command, ctx.context.is_some())
}
