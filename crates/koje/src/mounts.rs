//! Mounts follow `context.toml` (M5.11, ADR 0033): configured folders below the
//! home are mounted live into the shared home – before the container starts and
//! whenever the config changes, added and removed at any time; folders outside
//! the home are mounts of the container, changed only by a recreate.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::config::ContextConfig;
use koje_core::live_mounts::{self, below_home};
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::{ContainerRuntime as _, ContainerStatus};
use koje_podman::Podman;

/// Share the home volume's directory before the container starts; the
/// directory for the spec.
pub fn prepare(podman: &Podman, name: &ContextName) -> Result<PathBuf> {
    podman
        .prepare_home(name)
        .with_context(|| format!("preparing {name}'s home for live mounts"))
}

/// Bring the context's mounts in line with `cfg`: before its container starts
/// (`koje up`) and after `koje edit`. `edited` adds a note for containers from
/// before live mounts, which cannot follow.
pub fn sync(paths: &Paths, name: &ContextName, cfg: &ContextConfig, edited: bool) -> Result<()> {
    let podman = Podman::new();
    let recreate = format!("koje recreate {name}");
    let real = if podman.status(name)? == ContainerStatus::Missing {
        // Never started: `koje up` mounts them before the first start.
        if edited {
            return Ok(());
        }
        None
    } else if let Some(real) = podman.container_mounts(name)? {
        Some(real)
    } else {
        if edited {
            println!(
                "note: {name}'s container is from before live mounts – mount changes apply \
                 after recreating it: {recreate}"
            );
        }
        return Ok(());
    };
    let user = koje_podman::host::image_user().context("finding the current user")?;
    let home = PathBuf::from(&user.home);
    let desired = koje_podman::host::resolve_mounts(cfg, &paths.host_dirs()?)?;
    let home_dir = prepare(&podman, name)?;
    // Dotfiles presets (ADR 0038) are mounted below ~/.dotfiles, read-only,
    // by their own rules.
    let (dotfiles, others): (Vec<PathBuf>, Vec<PathBuf>) = podman
        .live_mounts(&home_dir)?
        .into_iter()
        .partition(|rel| rel.starts_with(koje_core::dotfiles::DIR));
    crate::dotfiles::sync_mounts(&podman, paths, cfg, &home_dir, &dotfiles)?;
    let live: Vec<PathBuf> = others.into_iter().map(|rel| home.join(rel)).collect();
    let plan = live_mounts::plan(&desired, real.as_deref(), &live, &home);
    for path in &plan.add {
        let below = below_home(path, &home).unwrap_or_default();
        podman
            .live_mount(&home_dir, &below, path)
            .with_context(|| format!("mounting {} into {name}", path.display()))?;
        if real.is_some() {
            println!("mounted {} into {name}", path.display());
        }
    }
    for path in &plan.remove {
        let below = below_home(path, &home).unwrap_or_default();
        podman.live_unmount(&home_dir, &below)?;
        println!("unmounted {} from {name}", path.display());
    }
    for path in &plan.stale_real {
        println!(
            "note: {} is no longer configured, but stays mounted in {name} until its \
             container is recreated ({recreate})",
            path.display()
        );
    }
    for path in &plan.missing_real {
        println!(
            "note: {} is outside the home and can't be mounted live – it gets mounted \
             when {name}'s container is recreated ({recreate})",
            path.display()
        );
    }
    Ok(())
}
