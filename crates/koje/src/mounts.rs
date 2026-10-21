//! Mounts of a running context follow `context.toml` (M5.11, ADR 0033):
//! configured mounts the container was not created with are mounted live
//! (below its live directory, with a symlink at their own path); live ones no
//! longer configured are unmounted lazily; removed real mounts need a recreate.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::config::ContextConfig;
use koje_core::live_mounts::{self, DIR_IN_CONTEXT};
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_podman::Podman;
use koje_podman::live::Link;

/// `$XDG_RUNTIME_DIR` on the host.
fn runtime_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")
}

/// Prepare the context's live directory before its container starts; the
/// directory for the spec.
pub fn prepare(podman: &Podman, name: &ContextName) -> Result<PathBuf> {
    let dir = live_mounts::host_dir(&runtime_dir()?, name);
    podman
        .prepare_live_dir(&dir)
        .with_context(|| format!("preparing the live mounts of {name}"))?;
    Ok(dir)
}

/// Bring the running context's mounts in line with `cfg`. `previous`: the
/// mounts before an edit, to find the paths of live entries to remove;
/// `None` when nothing was edited (`koje up`: no note for old containers).
pub fn sync(
    paths: &Paths,
    name: &ContextName,
    cfg: &ContextConfig,
    previous: Option<&[PathBuf]>,
) -> Result<()> {
    let podman = Podman::new();
    let Some(real) = podman.container_mounts(name)? else {
        if previous.is_none() {
            return Ok(());
        }
        println!(
            "note: {name}'s container is from before live mounts – mount changes apply \
             after recreating it: koje down {name} && podman rm koje-{name} && koje up {name}"
        );
        return Ok(());
    };
    let desired = koje_podman::host::resolve_mounts(cfg, &paths.host_dirs()?)?;
    let dir = live_mounts::host_dir(&runtime_dir()?, name);
    let live = podman.live_slots(&dir)?;
    let plan = live_mounts::plan(&desired, &real, &live);
    if plan.is_empty() {
        return Ok(());
    }
    let user = koje_podman::host::image_user().context("finding the current user")?;
    let home = PathBuf::from(&user.home);
    let inside = PathBuf::from(format!("/run/user/{}", user.uid)).join(DIR_IN_CONTEXT);
    for path in &plan.add {
        let slot = live_mounts::slot(path);
        podman
            .live_mount(&dir, &slot, path)
            .with_context(|| format!("mounting {} into {name}", path.display()))?;
        let target = inside.join(&slot);
        match podman.link_in_context(name, path, &target, !path.starts_with(&home))? {
            Link::Created => println!("mounted {} into {name} (live)", path.display()),
            Link::Occupied => println!(
                "mounted {} into {name} at {} – its own path is taken inside the context",
                path.display(),
                target.display()
            ),
        }
    }
    for slot in &plan.remove {
        podman.live_unmount(&dir, slot)?;
        match previous
            .unwrap_or_default()
            .iter()
            .find(|p| live_mounts::slot(p) == *slot)
        {
            Some(path) => {
                podman.unlink_in_context(name, path)?;
                println!("unmounted {} from {name}", path.display());
            }
            None => println!("unmounted {slot} from {name}"),
        }
    }
    for path in &plan.needs_recreate {
        println!(
            "note: {} stays mounted in {name} until its container is recreated \
             (koje down {name} && podman rm koje-{name} && koje up {name})",
            path.display()
        );
    }
    Ok(())
}
