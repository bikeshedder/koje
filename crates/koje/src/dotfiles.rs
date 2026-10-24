//! Dotfiles presets (ADR 0038): the presets a context lists are mounted
//! read-only at `~/.dotfiles/<name>` (live, with the other mounts) and linked
//! into its home when it starts, after `koje edit` and with `koje dotfiles`.
//! Decisions: `koje_core::dotfiles`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use koje_core::config::ContextConfig;
use koje_core::dotfiles::{self, Entry};
use koje_core::name::{ContextName, PresetName};
use koje_core::paths::Paths;
use koje_core::ports::ContainerRuntime as _;
use koje_podman::Podman;

/// The presets of `cfg` that exist, with their folders; a missing one is
/// reported and left out.
fn presets(paths: &Paths, cfg: &ContextConfig) -> Vec<(PresetName, PathBuf)> {
    cfg.dotfiles
        .iter()
        .filter_map(|p| {
            let dir = paths.dotfiles_dir().join(p.as_str());
            // A preset may be a link to a repository: mount what it points to.
            match std::fs::canonicalize(&dir) {
                Ok(real) if real.is_dir() => Some((p.clone(), real)),
                _ => {
                    println!(
                        "note: dotfiles preset {p} of {} does not exist – create {}",
                        cfg.name,
                        dir.display()
                    );
                    None
                }
            }
        })
        .collect()
}

/// Mount the presets of `cfg` at `~/.dotfiles/<name>` and unmount the others;
/// `live`: the mounts below `.dotfiles` now, as paths below the home.
pub fn sync_mounts(
    podman: &Podman,
    paths: &Paths,
    cfg: &ContextConfig,
    home_dir: &Path,
    live: &[PathBuf],
) -> Result<()> {
    let name = &cfg.name;
    let want: Vec<(PathBuf, PathBuf)> = presets(paths, cfg)
        .into_iter()
        .map(|(p, dir)| (dotfiles::mount_rel(&p), dir))
        .collect();
    let live: BTreeSet<&PathBuf> = live.iter().collect();
    for (rel, dir) in &want {
        if !live.contains(rel) {
            podman
                .live_mount_ro(home_dir, rel, dir)
                .with_context(|| format!("mounting {} into {name}", dir.display()))?;
        }
    }
    let wanted: BTreeSet<&PathBuf> = want.iter().map(|(rel, _)| rel).collect();
    for rel in live.difference(&wanted) {
        podman.live_unmount(home_dir, rel)?;
        println!("unmounted ~/{} from {name}", rel.display());
    }
    Ok(())
}

/// Link the presets into the running context's home (the stow-like plan of
/// `koje_core::dotfiles`), remove links no longer wanted, report what the
/// home has of its own where a link should go.
pub fn link(paths: &Paths, cfg: &ContextConfig) -> Result<()> {
    let name = &cfg.name;
    let presets: Vec<(PresetName, Vec<Entry>)> = presets(paths, cfg)
        .into_iter()
        .map(|(p, dir)| Ok((p, dotfiles::scan(&dir)?)))
        .collect::<Result<_>>()?;
    let links_file = paths.dotfiles_links(name);
    let previous = std::fs::read_to_string(&links_file)
        .map(|t| dotfiles::parse_links(&t))
        .unwrap_or_default();
    let probe = dotfiles::probe_paths(&presets, &previous);
    if probe.is_empty() {
        return Ok(());
    }
    let podman = Podman::new();
    let run = |script: &str, args: Vec<String>| -> Result<String> {
        let mut argv = vec!["sh", "-c", script, "sh"];
        argv.extend(args.iter().map(String::as_str));
        Ok(podman.query(name, &argv)?)
    };
    let found = dotfiles::parse_probe(&run(
        dotfiles::PROBE_SCRIPT,
        probe.iter().map(|p| p.display().to_string()).collect(),
    )?);
    let home = koje_podman::host::image_user()
        .context("finding the current user")?
        .home;
    let plan = dotfiles::plan(&home, &presets, &found, &previous);
    if !plan.steps.is_empty() {
        let out = run(dotfiles::APPLY_SCRIPT, dotfiles::apply_args(&plan.steps))?;
        for line in out.lines() {
            println!("note: dotfiles of {name}: {line}");
        }
        println!("dotfiles: {} change(s) in {name}", plan.steps.len());
    }
    for p in &plan.conflicts {
        println!(
            "note: kept ~/{} in {name} – it is the context's own; move it away to use the preset's",
            p.display()
        );
    }
    if let Some(dir) = links_file.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    koje_core::fsutil::write_atomic(&links_file, dotfiles::links_text(&plan.links).as_bytes())?;
    Ok(())
}

/// `koje dotfiles [-c <context>]`: link again now, e.g. after adding files to
/// a preset; lists the presets there are.
pub fn run(paths: &Paths, context: &str) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = koje_core::context::load(paths, &name)?;
    let podman = Podman::new();
    koje_core::lifecycle::require_running(podman.status(&name)?, name.as_str())
        .map_err(anyhow::Error::msg)?;
    let dir = paths.dotfiles_dir();
    let mut all: Vec<String> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(std::result::Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    all.sort();
    println!(
        "presets in {}: {}",
        dir.display(),
        if all.is_empty() {
            "none yet".to_owned()
        } else {
            all.join(", ")
        }
    );
    let used: Vec<&str> = cfg.dotfiles.iter().map(PresetName::as_str).collect();
    println!(
        "{name} uses: {}",
        if used.is_empty() {
            "none (add them to `dotfiles` with: koje edit {name})".to_owned()
        } else {
            used.join(", ")
        }
    );
    crate::mounts::sync(paths, &name, &cfg, false)?;
    link(paths, &cfg)
}
