//! `koje stats` and `koje limit` (M6.5, ADR 0035): memory use per context and
//! its limits. Usage is the working set; limits change live.

use anyhow::{Context as _, Result};
use koje_core::config::{ContextConfig, GlobalConfig, Limits};
use koje_core::context::{self, LimitChange};
use koje_core::memory::{self, MemoryLevel, MemoryUsage};
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::{ContainerRuntime as _, ContainerStatus};
use koje_core::size::ByteSize;
use koje_podman::Podman;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Row {
    context: String,
    state: &'static str,
    /// Bytes; `null` when not running.
    working_set: Option<u64>,
    current: Option<u64>,
    anon: Option<u64>,
    soft: Option<u64>,
    hard: Option<u64>,
    /// `normal`, `near_soft`, `over_soft`, `over_hard`.
    level: Option<&'static str>,
}

fn level_name(l: MemoryLevel) -> &'static str {
    match l {
        MemoryLevel::Normal => "normal",
        MemoryLevel::NearSoft => "near_soft",
        MemoryLevel::OverSoft => "over_soft",
        MemoryLevel::OverHard => "over_hard",
    }
}

fn state_name(s: ContainerStatus) -> &'static str {
    match s {
        ContainerStatus::Missing => "not created",
        ContainerStatus::Stopped => "stopped",
        ContainerStatus::Running => "running",
        ContainerStatus::Paused => "asleep",
    }
}

fn row(podman: &Podman, cfg: &ContextConfig, global: &GlobalConfig) -> Result<Row> {
    let status = podman.status(&cfg.name)?;
    let limits = cfg.effective_limits(global)?;
    let usage: Option<MemoryUsage> =
        matches!(status, ContainerStatus::Running | ContainerStatus::Paused)
            .then(|| podman.memory_usage(&cfg.name).ok())
            .flatten();
    Ok(Row {
        context: cfg.name.to_string(),
        state: state_name(status),
        working_set: usage.map(|u| u.working_set().0),
        current: usage.map(|u| u.current.0),
        anon: usage.map(|u| u.anon.0),
        soft: limits.memory_soft.map(|b| b.0),
        hard: limits.memory_hard.map(|b| b.0),
        level: usage.map(|u| level_name(memory::classify(u.working_set(), &limits))),
    })
}

/// `koje stats [<context>] [--json]`.
pub fn stats(paths: &Paths, context: Option<&str>, json: bool) -> Result<()> {
    let global = GlobalConfig::load(&paths.global_config())?;
    let configs = match context {
        Some(c) => vec![context::load(paths, &ContextName::new(c)?)?],
        None => context::list(paths)?,
    };
    let podman = Podman::new();
    let rows: Vec<Row> = configs
        .iter()
        .map(|cfg| row(&podman, cfg, &global))
        .collect::<Result<_>>()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("no contexts yet – create one with `koje new <context>`");
        return Ok(());
    }
    let size = |b: Option<u64>| b.map_or_else(|| "–".to_owned(), |b| ByteSize(b).human());
    println!(
        "{:<16} {:<12} {:>10} {:>10} {:>9} {:>9}  ",
        "CONTEXT", "STATE", "USED", "CHARGED", "SOFT", "HARD"
    );
    for r in &rows {
        let mark = match r.level {
            Some("near_soft") => "  near soft limit",
            Some("over_soft") => "  over soft limit",
            Some("over_hard") => "  over hard limit",
            _ => "",
        };
        println!(
            "{:<16} {:<12} {:>10} {:>10} {:>9} {:>9}{mark}",
            r.context,
            r.state,
            size(r.working_set),
            size(r.current),
            size(r.soft),
            size(r.hard)
        );
    }
    println!(
        "\nUSED: what limits count (without the file cache the kernel drops first); CHARGED: everything"
    );
    Ok(())
}

fn change(value: Option<&str>) -> Result<LimitChange> {
    Ok(match value {
        None => LimitChange::Keep,
        Some("none") => LimitChange::Clear,
        Some(v) => LimitChange::Set(v.parse::<ByteSize>()?),
    })
}

/// The kernel limit of a running context follows the hard limit (ADR 0035):
/// after `koje up` started it, and after `koje limit`.
pub fn apply_kernel_limit(podman: &Podman, name: &ContextName, limits: &Limits) -> Result<()> {
    let want = memory::kernel_limit(limits);
    if podman.kernel_limit(name)? != want {
        podman
            .set_kernel_limit(name, want)
            .with_context(|| format!("setting {name}'s kernel memory limit"))?;
    }
    Ok(())
}

/// `koje limit <context> [--soft <size>|none] [--hard <size>|none]`.
pub fn limit(paths: &Paths, context: &str, soft: Option<&str>, hard: Option<&str>) -> Result<()> {
    let name = ContextName::new(context)?;
    let global = GlobalConfig::load(&paths.global_config())?;
    let show = |cfg: &ContextConfig| -> Result<()> {
        let l = cfg.effective_limits(&global)?;
        let s = |b: Option<ByteSize>| b.map_or_else(|| "none".to_owned(), |b| b.to_string());
        println!(
            "{name}: soft {}, hard {}",
            s(l.memory_soft),
            s(l.memory_hard)
        );
        Ok(())
    };
    if soft.is_none() && hard.is_none() {
        return show(&context::load(paths, &name)?);
    }
    // Checked with config.toml's defaults before anything is written.
    let path = paths.context_config(&name);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let new = context::with_limits(&text, change(soft)?, change(hard)?)?;
    let limits = context::validate(paths, &name, &new)?.effective_limits(&global)?;
    let cfg = context::replace_config(paths, &name, &new)?;
    let podman = Podman::new();
    if matches!(
        podman.status(&name)?,
        ContainerStatus::Running | ContainerStatus::Paused
    ) {
        apply_kernel_limit(&podman, &name, &limits)?;
    }
    crate::kojed::reload();
    show(&cfg)
}
