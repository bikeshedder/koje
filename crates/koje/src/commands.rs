//! Command handlers. Each unimplemented command names the milestone that
//! delivers it, so `koje <cmd>` doubles as a progress indicator.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use koje_core::color::Color;
use koje_core::context;
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_core::states::{self, Role, Summary};

use crate::cli::{Command, Ctx, ImageCmd, StateCmd};
use crate::prompt::confirm;

pub fn run(command: &Command) -> Result<()> {
    let paths = Paths::from_env()?;
    match command {
        Command::List { json } => list(&paths, *json),
        Command::New { ctx, color, prefix } => new(&paths, ctx, color.as_deref(), prefix.clone()),

        Command::Edit(ctx) => edit(&paths, ctx),
        Command::Rm { ctx, yes } => rm(&paths, ctx, *yes),

        Command::State(cmd) => state(&paths, cmd),
        Command::Image(ImageCmd::Build { no_cache }) => image_build(*no_cache),

        Command::Up { ctx, state, .. } => {
            crate::runtime::up(&paths, &ctx.context, state.as_deref())
        }
        Command::Down { ctx, no_save, .. } => crate::runtime::down(&paths, &ctx.context, *no_save),

        Command::Shell { ctx, dev } => crate::runtime::shell(&paths, &ctx.context, dev.as_deref()),

        Command::Open { .. } | Command::Sleep(_) | Command::Wake(_) => todo_in("M3"),

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

/// `koje image build`: build `koje/base` for the current user (ADR 0023).
fn image_build(no_cache: bool) -> Result<()> {
    let user = koje_podman::host::image_user().context("finding the current user")?;
    println!(
        "building {} for {} (uid {}, home {}) – this takes a few minutes",
        koje_podman::image::BASE_IMAGE,
        user.name,
        user.uid,
        user.home.display()
    );
    koje_podman::Podman::new().build_base(&user, no_cache)?;
    println!("built {}", koje_podman::image::BASE_IMAGE);
    Ok(())
}

/// `koje state …`
fn state(paths: &Paths, cmd: &StateCmd) -> Result<()> {
    let ctx = ContextName::new(match cmd {
        StateCmd::List(ctx) => &ctx.context,
        StateCmd::Cp { context, .. }
        | StateCmd::Mv { context, .. }
        | StateCmd::Rm { context, .. } => context,
    })?;
    let roles = context::load(paths, &ctx)?.states;
    match cmd {
        StateCmd::List(_) => state_list(paths, &ctx, &roles),
        StateCmd::Cp { from, to, .. } => {
            let (from, to) = (StateName::new(from)?, StateName::new(to)?);
            states::copy(paths, &ctx, &from, &to)?;
            println!("copied state {from} to {to}");
            Ok(())
        }
        StateCmd::Mv { from, to, .. } => {
            let (from, to) = (StateName::new(from)?, StateName::new(to)?);
            states::rename(paths, &ctx, &from, &to)?;
            println!("renamed state {from} to {to}");
            if let Some(role) = Role::of(&from, &roles) {
                println!(
                    "note: the `{}` role still names {from}; rename it in {} if wanted",
                    role.as_str(),
                    paths.context_config(&ctx).display()
                );
            }
            Ok(())
        }
        StateCmd::Rm { state, .. } => {
            let state = StateName::new(state)?;
            states::remove(paths, &ctx, &state)?;
            println!("removed state {state}");
            match Role::of(&state, &roles) {
                Some(Role::Default) => println!(
                    "note: {state} is the `default` role; `koje up {ctx}` now loads {} until the next save",
                    roles.fresh
                ),
                Some(Role::Fresh) => println!(
                    "note: {state} is the `fresh` role; a missing state now falls back to `empty`"
                ),
                None => {}
            }
            Ok(())
        }
    }
}

fn state_list(
    paths: &Paths,
    ctx: &ContextName,
    roles: &koje_core::config::StateRoles,
) -> Result<()> {
    let now = std::time::SystemTime::now();
    println!(
        "{:<24} {:>7} {:>7}  {:<12} ROLE",
        "STATE", "WINDOWS", "LAYOUTS", "MODIFIED"
    );
    for row in states::list(paths, ctx, roles)? {
        let (windows, layouts, problem) = match &row.summary {
            Some(Summary::Ok { windows, layouts }) => {
                (windows.to_string(), layouts.to_string(), None)
            }
            Some(Summary::Invalid(e)) => ("?".into(), "?".into(), Some(e.as_str())),
            None => ("-".into(), "-".into(), None),
        };
        let modified = row.modified.map_or_else(|| "-".to_owned(), |t| age(now, t));
        let role = match (row.role, &row.summary) {
            _ if row.name.is_empty_state() => "reserved".to_owned(),
            (Some(r), None) => format!("{} (not saved yet)", r.as_str()),
            (Some(r), Some(_)) => r.as_str().to_owned(),
            (None, _) => String::new(),
        };
        println!(
            "{:<24} {windows:>7} {layouts:>7}  {modified:<12} {role}",
            row.name
        );
        if let Some(problem) = problem {
            for line in problem.lines() {
                println!("    {line}");
            }
        }
    }
    Ok(())
}

/// Coarse relative time for listings: `just now`, `5 min ago`, `3 h ago`, `2 days ago`.
fn age(now: std::time::SystemTime, then: std::time::SystemTime) -> String {
    let secs = now.duration_since(then).map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => match secs / 86_400 {
            1 => "1 day ago".to_owned(),
            days => format!("{days} days ago"),
        },
    }
}

/// One row of `koje list --json`. Field names are a public interface for
/// scripts and bar modules; only add fields.
#[derive(Debug, serde::Serialize)]
struct ListRow {
    name: String,
    #[serde(flatten)]
    config: Option<ListConfig>,
    /// Number of stored states (without the virtual `empty`).
    states: usize,
    /// Why `context.toml` could not be loaded, if it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ListConfig {
    prefix: String,
    color: String,
    image: String,
    mounts: Vec<String>,
}

/// `koje list`: contexts found in the config directory. A context whose
/// `context.toml` is broken is listed with its error instead of failing the list.
fn list(paths: &Paths, json: bool) -> Result<()> {
    let mut rows = Vec::new();
    for name in context::names(paths)? {
        let states = states::stored_names(paths, &name)?.len();
        let (config, error) = match context::load(paths, &name) {
            Ok(cfg) => (
                Some(ListConfig {
                    prefix: cfg.prefix,
                    color: cfg.color.to_string(),
                    image: cfg.image,
                    mounts: cfg.mounts.iter().map(|m| m.path.to_string()).collect(),
                }),
                None,
            ),
            Err(e) => (None, Some(e.to_string())),
        };
        rows.push(ListRow {
            name: name.to_string(),
            config,
            states,
            error,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!(
            "no contexts yet – create one with `koje new <context>` ({})",
            paths.contexts_dir().display()
        );
        return Ok(());
    }
    let swatches = use_color();
    // Status column arrives with the container runtime in M3.
    println!("{:<24} {:<6} {:<10} STATES", "CONTEXT", "PREFIX", "COLOR");
    for row in rows {
        match (&row.config, &row.error) {
            (Some(cfg), _) => {
                let swatch = cfg
                    .color
                    .parse::<Color>()
                    .ok()
                    .filter(|_| swatches)
                    .map_or_else(String::new, |c| {
                        format!("\x1b[48;2;{};{};{}m  \x1b[0m ", c.r, c.g, c.b)
                    });
                let width = if swatches { 7 } else { 10 };
                println!(
                    "{:<24} {:<6} {swatch}{:<width$} {}",
                    row.name, cfg.prefix, cfg.color, row.states
                );
            }
            (None, error) => {
                println!("{:<24} {:<6} {:<10} {}", row.name, "?", "?", row.states);
                for line in error.as_deref().unwrap_or_default().lines() {
                    println!("    {line}");
                }
            }
        }
    }
    Ok(())
}

/// Color swatches only on a terminal and unless `NO_COLOR` is set (no-color.org).
fn use_color() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn age_is_coarse() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |s| age(now, now - Duration::from_secs(s));
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(300), "5 min ago");
        assert_eq!(ago(3 * 3600 + 5), "3 h ago");
        assert_eq!(ago(86_400 + 1), "1 day ago");
        assert_eq!(ago(9 * 86_400), "9 days ago");
        // Clock skew: a file from the future is "just now".
        assert_eq!(age(now, now + Duration::from_secs(60)), "just now");
    }
}
