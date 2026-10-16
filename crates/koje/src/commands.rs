//! Command handlers. Each unimplemented command names the milestone that
//! delivers it, so `koje <cmd>` doubles as a progress indicator.

use anyhow::{Result, bail};
use koje_core::color::Color;
use koje_core::context;
use koje_core::name::ContextName;
use koje_core::paths::Paths;

use crate::cli::{Command, Ctx};

pub fn run(command: &Command) -> Result<()> {
    let paths = Paths::from_env()?;
    match command {
        Command::List => list(&paths),
        Command::New { ctx, color, prefix } => new(&paths, ctx, color.as_deref(), prefix.clone()),

        Command::Edit(_) | Command::Rm { .. } | Command::State(_) => todo_in("M1"),

        Command::Up { .. }
        | Command::Down { .. }
        | Command::Shell { .. }
        | Command::Open { .. }
        | Command::Sleep(_)
        | Command::Wake(_) => todo_in("M3"),

        Command::Focus(_) => todo_in("M4"),

        Command::Save { .. }
        | Command::Stash
        | Command::Unstash { .. }
        | Command::Layout { .. } => todo_in("M5"),

        Command::Stats { .. } | Command::Limit { .. } => todo_in("M6"),

        Command::Snap { .. } | Command::Overlay { .. } | Command::Pin | Command::Unpin => {
            todo_in("M7")
        }
    }
}

fn todo_in(milestone: &str) -> Result<()> {
    bail!("not implemented yet – planned for milestone {milestone} (see docs/plan/)")
}

fn context_name(ctx: &Ctx) -> Result<ContextName> {
    Ok(ContextName::new(&ctx.context)?)
}

/// `koje new`: write a fresh `context.toml`.
fn new(paths: &Paths, ctx: &Ctx, color: Option<&str>, prefix: Option<String>) -> Result<()> {
    let name = context_name(ctx)?;
    let color = color.map(str::parse::<Color>).transpose()?;
    let cfg = context::create(paths, &name, prefix, color)?;
    println!(
        "created context {} ({}, {}) in {}",
        cfg.name,
        cfg.prefix,
        cfg.color,
        paths.context_config(&name).display()
    );
    Ok(())
}

/// `koje list`: contexts found in the config directory.
fn list(paths: &Paths) -> Result<()> {
    let configs = context::list(paths)?;
    if configs.is_empty() {
        println!(
            "no contexts yet – create one with `koje new <context>` ({})",
            paths.contexts_dir().display()
        );
        return Ok(());
    }
    for cfg in configs {
        // Status column arrives with the container runtime in M3.
        println!("{:<24} {:<6} {}", cfg.name, cfg.prefix, cfg.color);
    }
    Ok(())
}
