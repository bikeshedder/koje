//! Command handlers. Each unimplemented command names the milestone that
//! delivers it, so `koje <cmd>` doubles as a progress indicator.

use anyhow::{Context as _, Result, bail};
use koje_core::config::ContextConfig;
use koje_core::paths::Paths;

use crate::cli::Command;

pub fn run(command: &Command) -> Result<()> {
    match command {
        Command::List => list(),

        Command::New { .. } | Command::Edit(_) | Command::Rm { .. } | Command::State(_) => {
            todo_in("M1")
        }

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

/// `koje list`: contexts found in the config directory.
fn list() -> Result<()> {
    let paths = Paths::from_env()?;
    let dir = paths.contexts_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no contexts yet ({} does not exist)", dir.display());
            return Ok(());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };

    let mut configs = Vec::new();
    for entry in entries {
        let path = entry?.path().join("context.toml");
        if path.is_file() {
            configs.push(ContextConfig::load(&path)?);
        }
    }
    configs.sort_by(|a, b| a.name.cmp(&b.name));

    for cfg in configs {
        // Status column arrives with the container runtime in M3.
        println!("{:<24} {:<6} {}", cfg.name, cfg.prefix, cfg.color);
    }
    Ok(())
}
