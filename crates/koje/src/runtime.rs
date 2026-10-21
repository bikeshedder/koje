//! Commands that drive the context container: `koje up` and `koje down`
//! (M3.4). Decisions come from `koje_core::lifecycle`; this module only
//! gathers the inputs and executes the steps.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use koje_core::config::{ContextConfig, GlobalConfig};
use koje_core::context;
use koje_core::lifecycle::{
    DownPlan, PausePlan, UpPlan, pick_dev_container, plan_down, plan_sleep, plan_up, plan_wake,
    require_running,
};
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_core::ports::{ContainerRuntime as _, ContainerStatus};
use koje_core::spec::{Built, ContextInputs, build};
use koje_podman::Podman;
use koje_podman::image::BASE_IMAGE;

/// `koje up <context> -s empty`: create (first time) and start the context.
pub fn up(
    paths: &Paths,
    context: &str,
    state: Option<&str>,
    no_save: bool,
    recover: bool,
    no_recover: bool,
) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let requested = match state {
        Some(s) => StateName::new(s)?,
        None => cfg.states.default.clone(),
    };
    let global = GlobalConfig::load(&paths.global_config())?;
    let access = cfg.effective_access(&global);
    let podman = Podman::new();
    let status = podman.status(&name)?;
    let plan = plan_up(status);
    // Running: kojed has its live state (even without windows, after
    // `up -s empty`) and its recovery file is current – unless kojed found the
    // windows lost (Hyprland or the session crashed). Stopped with a recovery
    // file: it did not shut down cleanly.
    let running = plan == UpPlan::AlreadyRunning;
    let kojed_says = if running { windows_lost(&name) } else { None };
    let lost = kojed_says == Some(true);
    // Without kojed nothing can be restored or switched: "already running".
    let live_known = running && kojed_says == Some(false);
    let mut to_load = if running && !lost {
        state_to_load(paths, &name, &cfg, &requested)?
    } else {
        load_or_recover(paths, &name, &cfg, &requested, recover, no_recover)?
    };
    if plan == UpPlan::Paused {
        bail!("{name} is paused – resume it with: koje wake {name}");
    }
    let mut host = koje_podman::host::detect().context("detecting the host environment")?;
    // kojed's security-context socket (ADR 0022); it must exist before the
    // container starts, since its directory is mounted.
    if access.wayland {
        let socket: String = crate::kojed::call("WaylandSocket", &(name.as_str(),))
            .context("getting the context's Wayland socket from kojed")?;
        host.wayland_socket = Some(socket.into());
    }
    // Sound through kojed's directory of socket links (ADR 0029).
    if access.sound && (host.pipewire_socket.is_some() || host.pulse_socket.is_some()) {
        let dir: String = crate::kojed::call("SoundSockets", &(name.as_str(),))
            .context("getting the context's sound sockets from kojed")?;
        host.sound_dir = Some(dir.into());
    }
    // The proxy first: its directory must exist when the container starts.
    if koje_core::hostbus::needed(&access) {
        host.forwarder = crate::hostbus::forwarder_binary();
        match crate::hostbus::start_proxy(&name, &access) {
            Ok(dir) => host.host_bus_dir = Some(dir),
            Err(e) => println!("note: host interfaces unavailable: {e:#}"),
        }
    }
    match plan {
        UpPlan::CreateAndStart => {
            ensure_image(&podman, &cfg)?;
            let built = spec(paths, &cfg, &global, access, &host)?;
            print_notes(&built);
            podman
                .create(&built.spec)
                .with_context(|| format!("creating the container for {name}"))?;
            podman.start(&name)?;
            println!("started {name} (container created)");
        }
        UpPlan::Start => {
            podman.start(&name)?;
            println!("started {name}");
        }
        UpPlan::AlreadyRunning if live_known && state.is_some() => {
            switch_state(paths, &name, &cfg, &requested, to_load.take(), no_save)?;
        }
        UpPlan::AlreadyRunning if lost => println!("{name} is running, but its windows were lost"),
        UpPlan::AlreadyRunning => println!("{name} is already running"),
        UpPlan::Paused => unreachable!("handled above"),
    }
    if host.host_bus_dir.is_some()
        && host.forwarder.is_some()
        && let Err(e) = crate::hostbus::start_forwarder(&podman, &name, &access, host.uid)
    {
        println!("note: host interfaces unavailable – the forwarder did not start: {e:#}");
    }
    if (!running || lost)
        && let Some((label, state)) = to_load
        && !state.windows.is_empty()
    {
        let text = state.to_toml(Path::new("state"))?;
        let started: u32 = crate::kojed::call("Restore", &(name.as_str(), text.as_str()))
            .with_context(|| format!("restoring {label} of {name}"))?;
        println!(
            "restoring {label}: {} window(s), {started} program(s) started",
            state.windows.len()
        );
    }
    // Its workspace stays while empty, and its windows go there (M4.6): show it.
    if let Err(e) = crate::kojed::call::<_, ()>("Started", &(name.as_str(),)) {
        tracing::debug!("not telling kojed that {name} started: {e:#}");
    }
    if let Err(e) = crate::kojed::call::<_, ()>("Focus", &(name.as_str(),)) {
        tracing::debug!("not focusing {name}: {e:#}");
    }
    Ok(())
}

/// The state `up` loads: requested → `fresh` → `empty` (ADR 0005); `None` for
/// `empty`. Says so when it falls back and what was repaired.
/// `koje up -s <state>` on a running context (M5.9): save the current windows
/// into `default` (unless `--no-save`), then kojed keeps, closes and starts.
fn switch_state(
    paths: &Paths,
    name: &ContextName,
    cfg: &ContextConfig,
    requested: &StateName,
    target: Option<(String, koje_core::state::State)>,
    no_save: bool,
) -> Result<()> {
    let default = &cfg.states.default;
    if !no_save {
        let live = live_state(name)?;
        koje_core::states::write(paths, name, default, &live)?;
        println!(
            "saved {} window(s) of {name} into state {default}",
            live.windows.len()
        );
    }
    if requested == default && !no_save {
        println!("{name} is in state {default} already");
        return Ok(());
    }
    let (label, target) = target.unwrap_or_else(|| {
        (
            "state empty".to_owned(),
            koje_core::state::State::new(koje_core::name::LayoutName::initial()),
        )
    });
    let text = target.to_toml(Path::new("state"))?;
    let (kept, closed, started): (u32, u32, u32) =
        crate::kojed::call("SwitchState", &(name.as_str(), text.as_str()))
            .with_context(|| format!("switching {name} to {label}"))?;
    println!(
        "{name}: switched to {label} – {kept} window(s) kept, {closed} closed, {started} program(s) started"
    );
    Ok(())
}

/// Whether kojed found the running context's windows lost; `None` without
/// kojed.
fn windows_lost(name: &ContextName) -> Option<bool> {
    crate::kojed::call::<_, bool>("WindowsLost", &(name.as_str(),)).ok()
}

/// The state to load, or the recovery file after a crash (ADR 0006): asked,
/// or decided by `--recover` / `--no-recover`.
fn load_or_recover(
    paths: &Paths,
    name: &ContextName,
    cfg: &ContextConfig,
    requested: &StateName,
    recover: bool,
    no_recover: bool,
) -> Result<Option<(String, koje_core::state::State)>> {
    use koje_core::recovery::{self, Action};
    let found = recovery::read(paths, name)?;
    let take = match recovery::decide(found.is_some(), recover, no_recover) {
        Action::None => false,
        Action::Restore => true,
        Action::Discard => {
            recovery::delete(paths, name)?;
            println!("discarded the recovery file of {name}");
            false
        }
        Action::Ask if !std::io::IsTerminal::is_terminal(&std::io::stdin()) => {
            // Nobody to ask (a launcher, autostart): keep it for later.
            println!(
                "note: {name} did not shut down cleanly – its recovery file is kept; \
                 restore it with: koje up {name} --recover (or after koje down {name} --no-save)"
            );
            false
        }
        Action::Ask => {
            let ago = found
                .as_ref()
                .and_then(|r| r.saved_at.elapsed().ok())
                .map_or_else(|| "earlier".to_owned(), recovery::ago);
            let yes = crate::prompt::confirm(
                &format!("{name} did not shut down cleanly – restore its windows from {ago}?"),
                true,
            )?;
            if !yes {
                recovery::delete(paths, name)?;
            }
            yes
        }
    };
    if take && let Some(r) = found {
        for issue in &r.loaded.repaired {
            println!("note: recovery file: repaired: {issue}");
        }
        return Ok(Some(("the recovery file".to_owned(), r.loaded.state)));
    }
    state_to_load(paths, name, cfg, requested)
}

fn state_to_load(
    paths: &Paths,
    name: &ContextName,
    cfg: &ContextConfig,
    requested: &StateName,
) -> Result<Option<(String, koje_core::state::State)>> {
    use koje_core::state::{Resolved, resolve_load};
    match resolve_load(requested, &cfg.states, |s| {
        koje_core::states::exists(paths, name, s)
    }) {
        Resolved::Empty { fell_back } => {
            if fell_back && !requested.is_empty_state() {
                println!("no state {requested} yet – starting {name} without windows");
            }
            Ok(None)
        }
        Resolved::Load {
            name: state_name,
            fell_back,
        } => {
            if fell_back {
                println!("no state {requested} yet – loading {state_name}");
            }
            let Some(loaded) = koje_core::states::read(paths, name, &state_name)? else {
                return Ok(None);
            };
            for issue in &loaded.repaired {
                println!("note: state {state_name}: repaired: {issue}");
            }
            Ok(Some((format!("state {state_name}"), loaded.state)))
        }
    }
}

/// The live state of a running context from kojed (ADR 0030).
fn live_state(name: &ContextName) -> Result<koje_core::state::State> {
    let text: String = crate::kojed::call("State", &(name.as_str(),))
        .with_context(|| format!("getting the windows of {name} from kojed"))?;
    Ok(koje_core::state::State::from_toml(
        &text,
        Path::new("kojed's live state"),
    )?)
}

/// `koje save <context> [-s <state>]`: write the running windows and layouts
/// into a state (default: the `default` role).
pub fn save(paths: &Paths, context: &str, state: Option<&str>) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let target = match state {
        Some(s) => StateName::new(s)?,
        None => cfg.states.default.clone(),
    };
    koje_core::state::ensure_writable(&target)?;
    let podman = Podman::new();
    if !matches!(podman.status(&name)?, ContainerStatus::Running) {
        bail!("{name} is not running – nothing to save (start it with: koje up {name})");
    }
    let live = live_state(&name)?;
    koje_core::states::write(paths, &name, &target, &live)?;
    println!(
        "saved {} window(s) of {name} into state {target}",
        live.windows.len()
    );
    Ok(())
}

/// `koje down <context> --no-save`: stop the context container.
pub fn down(paths: &Paths, context: &str, state: Option<&str>, no_save: bool) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let target = if no_save {
        None
    } else {
        let t = match state {
            Some(s) => StateName::new(s)?,
            None => cfg.states.default.clone(),
        };
        koje_core::state::ensure_writable(&t)?;
        Some(t)
    };
    let podman = Podman::new();
    let plan = plan_down(podman.status(&name)?);
    if matches!(plan, DownPlan::Stop | DownPlan::UnpauseAndStop) {
        if plan == DownPlan::UnpauseAndStop {
            podman.unpause(&name)?;
        }
        // The state first: closing the windows changes the live state (ADR 0030).
        if let Some(t) = &target {
            let live = live_state(&name).with_context(|| {
                format!("not stopped – to stop without saving: koje down {name} --no-save")
            })?;
            koje_core::states::write(paths, &name, t, &live)?;
            println!("saved {} window(s) into state {t}", live.windows.len());
        }
        close_programs(&name)?;
        podman.stop(&name)?;
        println!("stopped {name}");
        // A clean shutdown: no crash to recover from (ADR 0006).
        koje_core::recovery::delete(paths, &name)?;
    }
    match plan {
        DownPlan::Stop | DownPlan::UnpauseAndStop => {}
        DownPlan::AlreadyStopped => println!("{name} is not running"),
        DownPlan::NotCreated => println!("{name} has not been started yet"),
    }
    crate::hostbus::stop_proxy(&name);
    if let Err(e) = crate::kojed::call::<_, ()>("Stopped", &(name.as_str(),)) {
        tracing::debug!("not telling kojed that {name} stopped: {e:#}");
    }
    if let Err(e) = crate::kojed::call::<_, ()>("ReleaseWaylandSocket", &(name.as_str(),)) {
        tracing::debug!("not releasing the Wayland socket of {name}: {e:#}");
    }
    Ok(())
}

/// How long `down` waits for programs to end.
const CLOSE_TIMEOUT_SECS: u32 = 10;

/// End the context's programs in order before the container stops (M5.4):
/// kojed closes windows or signals programs and waits. Asks before stopping
/// when windows stay open (a program may be asking to save).
fn close_programs(name: &ContextName) -> Result<()> {
    let wait = std::time::Duration::from_secs(u64::from(CLOSE_TIMEOUT_SECS) + 10);
    let open: Vec<String> =
        match crate::kojed::call_within("CloseWindows", &(name.as_str(), CLOSE_TIMEOUT_SECS), wait)
        {
            Ok(open) => open,
            Err(e) => {
                println!("note: programs not closed in order ({e:#}) – stopping {name} directly");
                return Ok(());
            }
        };
    if open.is_empty() {
        return Ok(());
    }
    println!("still open after {CLOSE_TIMEOUT_SECS} s (maybe asking to save):");
    for w in &open {
        println!("  {w}");
    }
    if !crate::prompt::confirm(&format!("Stop {name} anyway?"), false)? {
        bail!("{name} is still running – close those windows, then: koje down {name} --no-save");
    }
    Ok(())
}

/// `koje shell <context> [--dev [name]]`: an interactive shell in the running
/// context, or in one of its dev containers.
pub fn shell(paths: &Paths, context: &str, dev: Option<&str>) -> Result<()> {
    use std::io::IsTerminal as _;
    let name = ContextName::new(context)?;
    context::load(paths, &name)?;
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    let tty = std::io::stdin().is_terminal();
    // Prefer a login bash, fall back to sh (dev containers may lack bash).
    let login = "command -v bash >/dev/null && exec bash -l || exec sh -l";
    match dev {
        None => podman.exec_attached(&name, &["sh", "-c", login], tty)?,
        Some(requested) => {
            let running = podman.dev_containers(&name)?;
            let requested = (!requested.is_empty()).then_some(requested);
            let dev = pick_dev_container(&running, requested).map_err(anyhow::Error::msg)?;
            let mut inner = vec!["podman", "exec", "-i"];
            if tty {
                inner.push("-t");
            }
            inner.extend([dev.as_str(), "sh", "-c", login]);
            podman.exec_attached(&name, &inner, tty)?
        }
    };
    // The shell's own exit status (last command in it) is not an error of koje.
    Ok(())
}

/// `koje open <context> <url|file>`: open in the running context with
/// `xdg-open` (links go to the context's browser). Files must lie in one of the
/// context's mounted folders – only there the context sees host files.
/// `koje run <context> <program> [args…]`: start a program in the running
/// context, detached; in the current directory if the context mounts it, else
/// in the home directory.
pub fn run(paths: &Paths, context: &str, command: &[String]) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    let cwd = std::env::current_dir().ok();
    let mounts = koje_podman::host::resolve_mounts(&cfg, &paths.host_dirs()?)?;
    let workdir = cwd
        .filter(|d| koje_core::open::visible_in(d, &mounts))
        .map(|d| d.display().to_string());
    // `exec` so the program replaces the shell that starts it.
    let line = format!("exec {}", koje_core::capture::shell_join(command));
    podman.launch(&name, &line, workdir.as_deref())?;
    println!("started {} in {name}", command.join(" "));
    Ok(())
}

pub fn open(paths: &Paths, context: &str, target: &str) -> Result<()> {
    use koje_core::open::{Target, classify, visible_in};
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let cwd = std::env::current_dir().context("finding the current directory")?;
    let arg = match classify(target, &cwd) {
        Target::Url(url) => url,
        Target::File(file) => {
            let file = std::fs::canonicalize(&file)
                .with_context(|| format!("{} does not exist", file.display()))?;
            let mounts = koje_podman::host::resolve_mounts(&cfg, &paths.host_dirs()?)?;
            if !visible_in(&file, &mounts) {
                bail!(
                    "{} is not visible in {name}: only its mounted folders are – \
                     add a folder to `mounts` with: koje edit {name}",
                    file.display()
                );
            }
            file.display().to_string()
        }
    };
    let podman = Podman::new();
    require_running(podman.status(&name)?, name.as_str()).map_err(anyhow::Error::msg)?;
    podman.exec_detached(&name, &["xdg-open", &arg])?;
    println!("opened {arg} in {name}");
    Ok(())
}

/// `koje sleep <context>`: freeze the context (CPU 0, memory kept). Moving its
/// windows to the stash first comes with M5; frozen windows are harmless (M2.8).
pub fn sleep(paths: &Paths, context: &str) -> Result<()> {
    let name = ContextName::new(context)?;
    context::load(paths, &name)?;
    let podman = Podman::new();
    match plan_sleep(podman.status(&name)?) {
        PausePlan::Do => {
            podman.pause(&name)?;
            println!("{name} is asleep – wake it with: koje wake {name}");
        }
        PausePlan::Already => println!("{name} is already asleep"),
        PausePlan::NotRunning => {
            bail!("{name} is not running – start it with: koje up {name} -s empty")
        }
    }
    Ok(())
}

/// `koje wake <context>`: resume a paused context.
pub fn wake(paths: &Paths, context: &str) -> Result<()> {
    let name = ContextName::new(context)?;
    context::load(paths, &name)?;
    let podman = Podman::new();
    match plan_wake(podman.status(&name)?) {
        PausePlan::Do => {
            podman.unpause(&name)?;
            println!("{name} is awake");
        }
        PausePlan::Already => println!("{name} is not asleep"),
        PausePlan::NotRunning => {
            bail!("{name} is not running – start it with: koje up {name} -s empty")
        }
    }
    Ok(())
}

/// Build `koje/base` on first use (ADR 0023).
fn ensure_image(podman: &Podman, cfg: &ContextConfig) -> Result<()> {
    if cfg.image != BASE_IMAGE || podman.image_exists(BASE_IMAGE)? {
        return Ok(());
    }
    println!("{BASE_IMAGE} is missing – building it first (a few minutes)");
    let user = koje_podman::host::image_user().context("finding the current user")?;
    podman.build_base(&user, false)?;
    Ok(())
}

/// The container spec from the context's config, the global defaults and the host.
fn spec(
    paths: &Paths,
    cfg: &ContextConfig,
    global: &GlobalConfig,
    access: koje_core::access::Access,
    host: &koje_core::spec::HostEnv,
) -> Result<Built> {
    let mounts = koje_podman::host::resolve_mounts(cfg, &paths.host_dirs()?)?;
    let limits = cfg.effective_limits(global)?;
    Ok(build(
        &ContextInputs {
            name: &cfg.name,
            image: &cfg.image,
            access,
            limits: &limits,
            mounts: &mounts,
        },
        host,
    ))
}

fn print_notes(built: &Built) {
    for note in &built.notes {
        println!("note: {note}");
    }
}
