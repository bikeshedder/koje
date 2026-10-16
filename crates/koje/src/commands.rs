//! Command handlers. Each unimplemented command names the milestone that
//! delivers it, so `koje <cmd>` doubles as a progress indicator.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use koje_core::color::Color;
use koje_core::context;
use koje_core::name::ContextName;
use koje_core::paths::Paths;

use crate::cli::{Command, Ctx};
use crate::prompt::confirm;

pub fn run(command: &Command) -> Result<()> {
    let paths = Paths::from_env()?;
    match command {
        Command::List => list(&paths),
        Command::New { ctx, color, prefix } => new(&paths, ctx, color.as_deref(), prefix.clone()),

        Command::Edit(ctx) => edit(&paths, ctx),
        Command::Rm { ctx, yes } => rm(&paths, ctx, *yes),

        Command::State(_) => todo_in("M1"),

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

/// `koje edit`: edit a copy of `context.toml` and replace the original only
/// with a valid result.
fn edit(paths: &Paths, ctx: &Ctx) -> Result<()> {
    let name = context_name(ctx)?;
    context::load(paths, &name)?;
    let original_path = paths.context_config(&name);
    let original = std::fs::read_to_string(&original_path)
        .with_context(|| format!("reading {}", original_path.display()))?;
    // Same directory and a `.toml` suffix, so editors pick the right mode.
    let draft = paths
        .context_dir(&name)
        .join(format!(".context.edit-{}.toml", std::process::id()));
    std::fs::write(&draft, &original).with_context(|| format!("writing {}", draft.display()))?;
    let result = edit_loop(paths, &name, &draft, &original);
    let _ = std::fs::remove_file(&draft);
    result
}

fn edit_loop(paths: &Paths, name: &ContextName, draft: &Path, original: &str) -> Result<()> {
    loop {
        run_editor(draft)?;
        let text = std::fs::read_to_string(draft)
            .with_context(|| format!("reading {}", draft.display()))?;
        if text == original {
            println!("no changes");
            return Ok(());
        }
        match context::replace_config(paths, name, &text) {
            Ok(_) => {
                println!("saved {}", paths.context_config(name).display());
                return Ok(());
            }
            Err(err) => {
                eprintln!("koje: {err}");
                if !confirm("Reopen the editor?", true)? {
                    bail!(
                        "changes discarded; {} is unchanged",
                        paths.context_config(name).display()
                    );
                }
            }
        }
    }
}

/// Run `$VISUAL`, `$EDITOR` or `vi` on `file`. The variable may contain
/// arguments (`code --wait`), so it goes through the shell like git does.
fn run_editor(file: &Path) -> Result<()> {
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "vi".to_owned());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(file)
        .status()
        .with_context(|| format!("starting editor {editor:?}"))?;
    if !status.success() {
        bail!("editor {editor:?} failed ({status}); nothing was changed");
    }
    Ok(())
}

/// `koje rm`: delete a context's config, states and recovery file.
fn rm(paths: &Paths, ctx: &Ctx, yes: bool) -> Result<()> {
    let name = context_name(ctx)?;
    if !context::exists(paths, &name) {
        bail!("context {name} does not exist");
    }
    if !yes {
        println!("This deletes context {name} for good:");
        for dir in context::dirs(paths, &name).iter().filter(|d| d.exists()) {
            println!("  {}", dir.display());
        }
        if !confirm(&format!("Remove context {name}?"), false)? {
            bail!("aborted; nothing was removed");
        }
    }
    context::remove(paths, &name)?;
    println!("removed context {name}");
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
