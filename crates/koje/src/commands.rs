//! Command handlers.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use koje_core::color::Color;
use koje_core::context;
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_core::states::{self, Role, Summary};

use crate::cli::{Command, Ctx, ImageCmd, StateCmd};
use koje_runtime::prompt::confirm;

pub fn run(command: &Command) -> Result<()> {
    let paths = Paths::from_env()?;
    match command {
        Command::List => list(&paths),
        Command::Setup { dry_run, yes } => crate::setup::run(*dry_run, *yes),
        Command::Doctor => crate::doctor::run(),
        Command::New { ctx, color, prefix } => new(&paths, ctx, color.as_deref(), prefix.clone()),

        Command::Settings { context } => settings(&paths, context.as_deref()),
        Command::Edit { ctx, stdin } => {
            if *stdin {
                edit_from_stdin(&paths, ctx)
            } else {
                edit(&paths, ctx)
            }
        }
        Command::Config { stdin } => config(&paths, *stdin),
        Command::Recreate { ctx, yes } => crate::lifecycle::recreate(&paths, &ctx.context, *yes),
        Command::Rm { ctx, yes } => rm(&paths, ctx, *yes),

        Command::State(cmd) => state(&paths, cmd),
        Command::Image(ImageCmd::Build { no_cache }) => image_build(*no_cache),

        Command::Up {
            ctx,
            state,
            no_save,
            recover,
            no_recover,
            no_focus,
        } => crate::lifecycle::up(
            &paths,
            &ctx.context,
            state.as_deref(),
            crate::lifecycle::Up {
                no_save: *no_save,
                recover: *recover,
                no_recover: *no_recover,
                focus: !*no_focus,
            },
        ),
        Command::Down {
            ctx,
            state,
            no_save,
        } => crate::lifecycle::down(&paths, &ctx.context, state.as_deref(), *no_save),

        Command::Shell { ctx, dev, user } => {
            koje_runtime::runtime::shell(&paths, &ctx.context, dev.as_deref(), user.as_deref())
        }

        Command::Open { ctx, target } => koje_runtime::runtime::open(&paths, &ctx.context, target),
        Command::Run {
            ctx,
            no_focus,
            user,
            command,
        } => koje_runtime::runtime::run(&paths, &ctx.context, command, !*no_focus, user.as_deref()),

        Command::Apps { ctx, start } => crate::apps::run(&paths, ctx, start.as_deref()),
        Command::Favorites { ctx, set, reset } => {
            crate::apps::favorites(&paths, ctx, set.as_deref(), *reset)
        }
        Command::Dotfiles { context } => koje_runtime::dotfiles::run(&paths, context.as_deref()),
        Command::Terminal(ctx) => crate::terminal::run(&paths, ctx),

        Command::Sleep(ctx) => crate::lifecycle::sleep(&paths, &ctx.context),
        Command::Wake(ctx) => crate::lifecycle::wake(&paths, &ctx.context),

        Command::Focus(ctx) => focus(&paths, ctx),

        Command::Save { ctx, state } => {
            crate::lifecycle::save(&paths, &ctx.context, state.as_deref())
        }
        Command::Stats { context } => koje_runtime::memory::stats(&paths, context.as_deref()),
        Command::Limit { ctx, soft, hard } => {
            koje_runtime::memory::limit(&paths, &ctx.context, soft.as_deref(), hard.as_deref())
        }
    }
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
    crate::kojed::reload();
    Ok(())
}

/// `koje focus`: kojed shows the context's workspace.
fn focus(paths: &Paths, ctx: &Ctx) -> Result<()> {
    let name = context_name(ctx)?;
    context::load(paths, &name)?;
    crate::kojed::call::<_, ()>("Focus", &(name.as_str(),))
}

/// `koje edit`: edit a copy of `context.toml` and replace the original only
/// with a valid result.
fn edit(paths: &Paths, ctx: &Ctx) -> Result<()> {
    let name = context_name(ctx)?;
    let before = context::load(paths, &name)?;
    let original_path = paths.context_config(&name);
    let original = std::fs::read_to_string(&original_path)
        .with_context(|| format!("reading {}", original_path.display()))?;
    // Same directory and a `.toml` suffix, so editors pick the right mode.
    let draft = paths
        .context_dir(&name)
        .join(format!(".context.edit-{}.toml", std::process::id()));
    std::fs::write(&draft, &original).with_context(|| format!("writing {}", draft.display()))?;
    let result = edit_loop(&draft, &original, &paths.context_config(&name), |text| {
        context::replace_config(paths, &name, text).map(|_| ())
    });
    let _ = std::fs::remove_file(&draft);
    result?;
    apply_edit(paths, &name, &before);
    Ok(())
}

/// `koje settings`: the settings app (`koje-settings`, ADR 0060), detached.
fn settings(paths: &Paths, context: Option<&str>) -> Result<()> {
    let mut args = Vec::new();
    if let Some(c) = context {
        let name = ContextName::new(c)?;
        context::load(paths, &name)?;
        args.push(name.to_string());
    }
    std::process::Command::new("koje-settings")
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .context("starting koje-settings (the settings app)")?;
    Ok(())
}

/// `koje edit --stdin`: replace `context.toml` with what comes on stdin,
/// checked as `edit` checks the editor's result, then follow it as `edit`.
fn edit_from_stdin(paths: &Paths, ctx: &Ctx) -> Result<()> {
    use std::io::Read as _;
    let name = context_name(ctx)?;
    let before = context::load(paths, &name)?;
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("reading the new context.toml from stdin")?;
    context::replace_config(paths, &name, &text)?;
    println!("saved {}", paths.context_config(&name).display());
    crate::kojed::reload();
    apply_edit(paths, &name, &before);
    Ok(())
}

/// The context follows the edited config where it can (ADR 0033): mounts
/// below the home at once, running or not; image and access need a new
/// container (M5.11).
fn apply_edit(paths: &Paths, name: &ContextName, before: &koje_core::config::ContextConfig) {
    let Ok(after) = context::load(paths, name) else {
        return;
    };
    if let Ok(global) = koje_core::config::GlobalConfig::load(&paths.global_config()) {
        let fp = |c: &koje_core::config::ContextConfig| {
            koje_core::container::fingerprint(&c.image, &c.effective_access(&global))
        };
        let podman = koje_podman::Podman::new();
        let exists = !matches!(
            koje_core::ports::ContainerRuntime::status(&podman, name),
            Ok(koje_core::ports::ContainerStatus::Missing) | Err(_)
        );
        if exists && fp(before) != fp(&after) {
            println!(
                "note: the new image or access apply to a new container: koje recreate {name}"
            );
        }
    }
    if let Err(e) = koje_runtime::mounts::sync(paths, name, &after, true) {
        println!("note: mounts of {name} not changed: {e:#}");
    }
    // A running context links changed presets at once (ADR 0038).
    let running = matches!(
        koje_core::ports::ContainerRuntime::status(&koje_podman::Podman::new(), name),
        Ok(koje_core::ports::ContainerStatus::Running)
    );
    if running
        && (after.dotfiles != before.dotfiles || !after.dotfiles.is_empty())
        && let Err(e) = koje_runtime::dotfiles::link(paths, &after)
    {
        println!("note: dotfiles of {name} not linked: {e:#}");
    }
}

/// Edit `draft` until `save` takes it (or the user gives up); `target` is
/// the file `save` writes.
fn edit_loop(
    draft: &Path,
    original: &str,
    target: &Path,
    save: impl Fn(&str) -> koje_core::error::Result<()>,
) -> Result<()> {
    loop {
        run_editor(draft)?;
        let text = std::fs::read_to_string(draft)
            .with_context(|| format!("reading {}", draft.display()))?;
        if text == original {
            println!("no changes");
            return Ok(());
        }
        match save(&text) {
            Ok(()) => {
                println!("saved {}", target.display());
                crate::kojed::reload();
                return Ok(());
            }
            Err(err) => {
                eprintln!("koje: {err}");
                if !confirm("Reopen the editor?", true)? {
                    bail!("changes discarded; {} is unchanged", target.display());
                }
            }
        }
    }
}

/// `koje config [--stdin]`: edit `config.toml` like `edit` edits a
/// context's file. kojed and the shell follow the file themselves.
fn config(paths: &Paths, stdin: bool) -> Result<()> {
    use koje_core::config::GlobalConfig;
    let path = paths.global_config();
    let save = |text: &str| GlobalConfig::replace(&path, text).map(|_| ());
    if stdin {
        use std::io::Read as _;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("reading the new config.toml from stdin")?;
        save(&text)?;
        println!("saved {}", path.display());
        crate::kojed::reload();
        return Ok(());
    }
    let original = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let draft = dir.join(format!(".config.edit-{}.toml", std::process::id()));
    std::fs::write(&draft, &original).with_context(|| format!("writing {}", draft.display()))?;
    let result = edit_loop(&draft, &original, &path, save);
    let _ = std::fs::remove_file(&draft);
    result
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
    use koje_core::container::{container_name, home_volume};
    use koje_core::lifecycle::{DownPlan, plan_down};
    use koje_core::ports::{ContainerRuntime as _, ContainerStatus};

    let name = context_name(ctx)?;
    if !context::exists(paths, &name) {
        bail!("context {name} does not exist");
    }
    // Podman may be missing or broken; koje's own files can be removed anyway.
    let podman = koje_podman::Podman::new();
    let volume = home_volume(&name);
    let (status, has_volume) = match podman.status(&name) {
        Ok(status) => (Some(status), podman.volume_exists(&volume).unwrap_or(false)),
        Err(e) => {
            eprintln!("koje: could not check for a container ({e}); removing koje's files only");
            (None, false)
        }
    };
    let has_container = status.is_some_and(|s| s != ContainerStatus::Missing);
    if !yes {
        println!("This deletes context {name} for good:");
        if let Some(status) = status.filter(|_| has_container) {
            let running = matches!(status, ContainerStatus::Running | ContainerStatus::Paused);
            let note = if running {
                " (running – stopped first)"
            } else {
                ""
            };
            println!("  container {}{note}", container_name(&name));
        }
        if has_volume {
            println!("  home volume {volume} – everything {name} stored in its home");
        }
        for dir in context::dirs(paths, &name).iter().filter(|d| d.exists()) {
            println!("  {}", dir.display());
        }
        if !confirm(&format!("Remove context {name}?"), false)? {
            bail!("aborted; nothing was removed");
        }
    }
    // Container and volume first, config last: on failure `koje rm` can be repeated.
    if let Some(status) = status.filter(|_| has_container) {
        match plan_down(status) {
            DownPlan::Stop => podman.stop(&name)?,
            DownPlan::UnpauseAndStop => {
                podman.unpause(&name)?;
                podman.stop(&name)?;
            }
            DownPlan::AlreadyStopped | DownPlan::NotCreated => {}
        }
        podman.remove(&name)?;
    }
    if has_volume {
        // Shared for live mounts (ADR 0033): unmount it in Podman's namespace first.
        podman.release_home(&name)?;
        podman.remove_volume(&volume)?;
    }
    koje_runtime::hostbus::stop_proxy(&name);
    context::remove(paths, &name)?;
    println!("removed context {name}");
    Ok(())
}

/// `koje image build`: build `koje/base` for the current user (ADR 0023).
pub fn image_build(no_cache: bool) -> Result<()> {
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

/// One row of `koje list`.
#[derive(Debug)]
struct ListRow {
    name: String,
    config: Option<ListConfig>,
    /// Number of stored states (without the virtual `empty`).
    states: usize,
    /// Why `context.toml` could not be loaded, if it could not.
    error: Option<String>,
}

#[derive(Debug)]
struct ListConfig {
    prefix: String,
    color: String,
}

/// `koje list`: contexts found in the config directory. A context whose
/// `context.toml` is broken is listed with its error instead of failing the list.
fn list(paths: &Paths) -> Result<()> {
    let mut rows = Vec::new();
    for name in context::names(paths)? {
        let states = states::stored_names(paths, &name)?.len();
        let (config, error) = match context::load(paths, &name) {
            Ok(cfg) => (
                Some(ListConfig {
                    prefix: cfg.prefix,
                    color: cfg.color.to_string(),
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
