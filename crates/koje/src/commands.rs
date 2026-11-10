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
        Command::New {
            ctx,
            color,
            prefix,
            image,
        } => new(
            &paths,
            ctx,
            color.as_deref(),
            prefix.clone(),
            image.as_deref(),
        ),

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
        Command::Image(ImageCmd::Build {
            images,
            no_cache,
            pull,
        }) => image_build(&paths, images, *no_cache, *pull),
        Command::Image(ImageCmd::New { name, from, label }) => {
            image_new(&paths, name, from, label.as_deref())
        }
        Command::Image(ImageCmd::List) => image_list(&paths),
        Command::Image(ImageCmd::Rename { name, new_name }) => image_rename(&paths, name, new_name),
        Command::Image(ImageCmd::Customize { image }) => image_customize(&paths, image),
        Command::Image(ImageCmd::Reset { image, yes }) => image_reset(&paths, image, *yes),
        Command::Image(ImageCmd::Rm { name, yes }) => image_rm(&paths, name, *yes),
        Command::Image(ImageCmd::Edit(ctx)) => image_edit(&paths, ctx),

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

/// An image chosen on the terminal from koje's and the folders (ADR 0067).
fn choose_image(paths: &Paths, question: &str) -> Result<String> {
    let choices: Vec<(String, String)> = koje_core::images::list(paths)?
        .into_iter()
        .filter(|e| e.error.is_none())
        .map(|e| (e.reference(), e.label))
        .collect();
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        let names: Vec<&str> = choices.iter().map(|(v, _)| v.as_str()).collect();
        bail!("choose an image with --image: {}", names.join(", "));
    }
    koje_runtime::prompt::choose(question, &choices)?
        .context("no image chosen; nothing was created")
}

fn context_name(ctx: &Ctx) -> Result<ContextName> {
    Ok(ContextName::new(&ctx.context)?)
}

/// `koje new`: write a fresh `context.toml`.
fn new(
    paths: &Paths,
    ctx: &Ctx,
    color: Option<&str>,
    prefix: Option<String>,
    image: Option<&str>,
) -> Result<()> {
    let name = context_name(ctx)?;
    let color = color.map(str::parse::<Color>).transpose()?;
    let image = match image {
        Some(i) => i.to_owned(),
        None => choose_image(paths, &format!("Which image does {name} run?"))?,
    };
    // As it is: a wrong version or name is refused here, not at the first start.
    koje_runtime::images::source(paths, &image)?;
    let cfg = context::create(paths, &name, prefix, color, &image)?;
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
    koje_runtime::runtime::purge(paths, &name, status)?;
    // kojed forgets it: rules, color, workspace.
    crate::kojed::reload();
    Ok(())
}

/// `koje image build`: build images and the user layer (ADR 0040); without
/// names, the images contexts use (or koje's default one).
pub fn image_build(paths: &Paths, images: &[String], no_cache: bool, pull: bool) -> Result<()> {
    let mut images = images.to_vec();
    if images.is_empty() {
        images = koje_runtime::images::in_use(paths)?;
    }
    if images.is_empty() {
        bail!("no context uses an image yet – name one: koje image build localhost/koje/fedora:45");
    }
    let opts = koje_runtime::images::Options {
        force: true,
        no_cache,
        pull,
    };
    for image in &images {
        let ready = koje_runtime::images::ensure(paths, image, opts)
            .with_context(|| format!("building image {image}"))?;
        println!("built {image} → {}", ready.tag);
    }
    println!("contexts get the new build when they are recreated: koje recreate <context>");
    Ok(())
}

/// `koje image edit`: the context's Containerfile, opened with `xdg-open`.
fn image_edit(paths: &Paths, ctx: &Ctx) -> Result<()> {
    let name = context_name(ctx)?;
    let own = koje_runtime::images::own_file(paths, &name)?;
    if own.created {
        println!("created {}", own.file.display());
    }
    if own.image_set {
        let image = koje_core::context::load(paths, &name)?.image;
        println!("{name} now uses image {image:?}");
        crate::kojed::reload();
    }
    let status = std::process::Command::new("xdg-open")
        .arg(&own.file)
        .status()
        .context("starting xdg-open")?;
    if !status.success() {
        bail!(
            "xdg-open could not open {} ({status}) – open it in your editor",
            own.file.display()
        );
    }
    println!("{name} gets the changes when recreated: koje recreate {name}");
    Ok(())
}

/// `koje image new`: an image folder with a Containerfile building on `from`.
fn image_new(paths: &Paths, name: &str, from: &str, label: Option<&str>) -> Result<()> {
    let name = koje_core::name::ImageName::new(name)?;
    let from_line = koje_runtime::images::from_line(paths, from)?;
    if koje_core::image::parents(&format!("FROM {from_line}"))
        .iter()
        .any(|p| p.koje_name() == Some(&name))
    {
        bail!("image {name} cannot build on itself");
    }
    let text = koje_runtime::images::new_containerfile(&from_line);
    let dir = koje_core::images::create(paths, &name, &text, label)?;
    println!("created {}", dir.join("Containerfile").display());
    println!(
        "use it in a context with image = \"{}\" (koje edit <context>)",
        koje_core::image::reference(name.as_str(), koje_core::image::FIRST_VERSION)
    );
    crate::kojed::reload();
    Ok(())
}

/// `koje image list`.
fn image_list(paths: &Paths) -> Result<()> {
    let images = koje_core::images::list(paths)?;
    let width = images
        .iter()
        .map(|e| e.name.as_str().len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!("{:<width$}  {:<24}  USED BY", "NAME", "LABEL");
    for e in images {
        let used: Vec<String> = e.used_by.iter().map(ToString::to_string).collect();
        let kind = match e.kind {
            koje_core::images::Kind::BuiltIn => " (koje's)",
            koje_core::images::Kind::Folder => "",
            _ if koje_runtime::images::koje_has_update(&e) => " (customized; koje has a newer one)",
            koje_core::images::Kind::Customized => " (customized)",
        };
        println!(
            "{:<width$}  {:<24}  {}{}",
            e.name,
            format!("{}{kind}", e.label),
            if used.is_empty() {
                "-".to_owned()
            } else {
                used.join(", ")
            },
            e.error.map(|err| format!("  ! {err}")).unwrap_or_default()
        );
    }
    Ok(())
}

/// `koje image rename`.
fn image_rename(paths: &Paths, name: &str, new_name: &str) -> Result<()> {
    let (old, new) = (
        koje_core::name::ImageName::new(name)?,
        koje_core::name::ImageName::new(new_name)?,
    );
    let moved = koje_core::images::rename(paths, &old, &new)?;
    println!("renamed image {old} to {new}");
    for ctx in moved {
        println!(
            "  {ctx} now uses it as {}",
            context::load(paths, &ctx)?.image
        );
    }
    crate::kojed::reload();
    Ok(())
}

/// `koje image customize`.
fn image_customize(paths: &Paths, image: &str) -> Result<()> {
    let name = koje_core::name::ImageName::new(image)?;
    let dir = koje_runtime::images::customize(paths, &name)?;
    println!("customized {name}: {}", dir.display());
    println!(
        "it takes the place of koje's {name} for every context; contexts get it when recreated"
    );
    crate::kojed::reload();
    Ok(())
}

/// `koje image reset`.
fn image_reset(paths: &Paths, image: &str, yes: bool) -> Result<()> {
    let name = koje_core::name::ImageName::new(image)?;
    if !yes
        && !confirm(
            &format!(
                "Reset {name} to koje's version? {} and your changes in it are deleted.",
                paths.images_dir().join(name.as_str()).display()
            ),
            false,
        )?
    {
        bail!("aborted; nothing was changed");
    }
    koje_core::images::reset(paths, &name)?;
    println!("{name} is koje's own image again; contexts get it when recreated");
    crate::kojed::reload();
    Ok(())
}

/// `koje image rm`.
fn image_rm(paths: &Paths, name: &str, yes: bool) -> Result<()> {
    let name = koje_core::name::ImageName::new(name)?;
    if !yes
        && !confirm(
            &format!(
                "Remove image {name}? Its folder {} is deleted.",
                paths.images_dir().join(name.as_str()).display()
            ),
            false,
        )?
    {
        bail!("aborted; nothing was removed");
    }
    koje_core::images::remove(paths, &name)?;
    println!("removed image {name}");
    crate::kojed::reload();
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
