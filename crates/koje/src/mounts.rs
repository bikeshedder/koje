//! Mounts of a running context follow `context.toml` (M5.11, ADR 0033):
//! configured folders below the home that the container was not created with
//! are mounted live at their own path (bound below the shared home in Podman's
//! namespace); live ones no longer configured are unmounted lazily; removed
//! real mounts and new ones outside the home need a recreate.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::config::ContextConfig;
use koje_core::live_mounts::{self, below_home};
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_podman::Podman;

/// Share the home volume's directory before the container starts; the
/// directory for the spec.
pub fn prepare(podman: &Podman, name: &ContextName) -> Result<PathBuf> {
    podman
        .prepare_home(name)
        .with_context(|| format!("preparing {name}'s home for live mounts"))
}

/// Bring the running context's mounts in line with `cfg`. `previous`: the
/// mounts before an edit (live ones among them may have to go); `None` when
/// nothing was edited (`koje up`: no note for old containers).
pub fn sync(
    paths: &Paths,
    name: &ContextName,
    cfg: &ContextConfig,
    previous: Option<&[PathBuf]>,
) -> Result<()> {
    let podman = Podman::new();
    let Some(real) = podman.container_mounts(name)? else {
        if previous.is_some() {
            println!(
                "note: {name}'s container is from before live mounts – mount changes apply \
                 after recreating it: koje down {name} && podman rm koje-{name} && koje up {name}"
            );
        }
        return Ok(());
    };
    let user = koje_podman::host::image_user().context("finding the current user")?;
    let home = PathBuf::from(&user.home);
    let desired = koje_podman::host::resolve_mounts(cfg, &paths.host_dirs()?)?;
    let home_dir = prepare(&podman, name)?;
    // Live now: of the configured and previous mounts below the home.
    let candidates: Vec<PathBuf> = desired
        .iter()
        .chain(previous.unwrap_or_default())
        .filter(|p| !real.contains(p))
        .filter_map(|p| below_home(p, &home))
        .collect();
    let live: Vec<PathBuf> = podman
        .live_mounted(&home_dir, &candidates)?
        .into_iter()
        .map(|rel| home.join(rel))
        .collect();
    let plan = live_mounts::plan(&desired, &real, &live, &home);
    for path in &plan.add {
        let below = below_home(path, &home).unwrap_or_default();
        podman
            .live_mount(&home_dir, &below, path)
            .with_context(|| format!("mounting {} into {name}", path.display()))?;
        println!("mounted {} into {name} (live)", path.display());
    }
    for path in &plan.remove {
        let below = below_home(path, &home).unwrap_or_default();
        podman.live_unmount(&home_dir, &below)?;
        println!("unmounted {} from {name}", path.display());
    }
    let recreate = format!("koje down {name} && podman rm koje-{name} && koje up {name}");
    for path in &plan.needs_recreate {
        if real.contains(path) {
            println!(
                "note: {} is no longer configured, but stays mounted in {name} until its \
                 container is recreated ({recreate})",
                path.display()
            );
        } else {
            println!(
                "note: {} is outside the home and can't be mounted live – it gets mounted \
                 when {name}'s container is recreated ({recreate})",
                path.display()
            );
        }
    }
    Ok(())
}
