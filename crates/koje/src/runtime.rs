//! Commands that drive the context container: `koje up` and `koje down`
//! (M3.4). Decisions come from `koje_core::lifecycle`; this module only
//! gathers the inputs and executes the steps.

use anyhow::{Context as _, Result, bail};
use koje_core::config::{ContextConfig, GlobalConfig};
use koje_core::context;
use koje_core::lifecycle::{DownPlan, UpPlan, plan_down, plan_up};
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_core::ports::ContainerRuntime as _;
use koje_core::spec::{Built, ContextInputs, build};
use koje_podman::Podman;
use koje_podman::image::BASE_IMAGE;

/// `koje up <context> -s empty`: create (first time) and start the context.
pub fn up(paths: &Paths, context: &str, state: Option<&str>) -> Result<()> {
    let name = ContextName::new(context)?;
    let cfg = context::load(paths, &name)?;
    let wants_empty = state
        .map(StateName::new)
        .transpose()?
        .is_some_and(|s| s.is_empty_state());
    if !wants_empty {
        bail!(
            "restoring windows from a state comes with milestone M5 – \
             start {name} without windows: koje up {name} -s empty"
        );
    }
    let podman = Podman::new();
    match plan_up(podman.status(&name)?) {
        UpPlan::CreateAndStart => {
            ensure_image(&podman, &cfg)?;
            let built = spec(paths, &cfg)?;
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
        UpPlan::AlreadyRunning => println!("{name} is already running"),
        UpPlan::Paused => bail!("{name} is paused – resume it with: koje wake {name}"),
    }
    Ok(())
}

/// `koje down <context> --no-save`: stop the context container.
pub fn down(paths: &Paths, context: &str, no_save: bool) -> Result<()> {
    let name = ContextName::new(context)?;
    context::load(paths, &name)?;
    if !no_save {
        bail!(
            "saving the windows into a state comes with milestone M5 – \
             stop {name} without saving: koje down {name} --no-save"
        );
    }
    let podman = Podman::new();
    match plan_down(podman.status(&name)?) {
        DownPlan::Stop => {
            podman.stop(&name)?;
            println!("stopped {name}");
        }
        DownPlan::UnpauseAndStop => {
            podman.unpause(&name)?;
            podman.stop(&name)?;
            println!("stopped {name}");
        }
        DownPlan::AlreadyStopped => println!("{name} is not running"),
        DownPlan::NotCreated => println!("{name} has not been started yet"),
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
fn spec(paths: &Paths, cfg: &ContextConfig) -> Result<Built> {
    let global = GlobalConfig::load(&paths.global_config())?;
    let host = koje_podman::host::detect().context("detecting the host environment")?;
    let mounts = koje_podman::host::resolve_mounts(cfg, &paths.host_dirs()?)?;
    let limits = cfg.effective_limits(&global)?;
    Ok(build(
        &ContextInputs {
            name: &cfg.name,
            image: &cfg.image,
            access: cfg.effective_access(&global),
            limits: &limits,
            mounts: &mounts,
        },
        &host,
    ))
}

fn print_notes(built: &Built) {
    for note in &built.notes {
        println!("note: {note}");
    }
}
