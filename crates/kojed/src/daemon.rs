//! kojed's window tracking (M4.4) and border colors (M4.5, ADR 0018): reacts to
//! compositor events, asks the container runtime which context a window's
//! process belongs to and sends tag rules and tags. Decisions live in
//! `koje_core::windows`; this module does the I/O around them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::capture::{Live, Observed};
use koje_core::color::Color;
use koje_core::integration::Integration;
use koje_core::live_layouts::{self, LayoutInfo, Shown};
use koje_core::name::ContextName;
use koje_core::name::{LayoutName, StateName, WindowId};
use koje_core::paths::Paths;
use koje_core::ports::WindowInfo;
use koje_core::ports::{
    Compositor, CompositorCommand, CompositorEvent, ContainerRuntime, ContainerStatus,
};
use koje_core::restore;
use koje_core::state::State;
use koje_core::windows::{self, WindowMap};

/// What clients hear about (D-Bus signals, ADR 0027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    WindowAdded {
        address: String,
        context: ContextName,
    },
    WindowRemoved {
        address: String,
        context: ContextName,
    },
}

/// A context as `ListContexts` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInfo {
    pub name: ContextName,
    pub color: Color,
    pub windows: usize,
}

#[derive(Debug)]
pub struct Daemon<C, R> {
    compositor: C,
    runtime: R,
    paths: Paths,
    colors: BTreeMap<ContextName, Color>,
    windows: WindowMap,
    focused: Option<String>,
    /// Live state per running context (ADR 0030).
    live: BTreeMap<ContextName, Live>,
    integrations: Vec<Integration>,
    /// `/proc` (a fake one in tests).
    proc_root: PathBuf,
    /// The user's home (also the context's): expands `~` in remembered paths.
    home: String,
    /// Processes sent SIGTERM by `close_windows`, per context: `down` waits
    /// until they are gone, not just their windows (Firefox writes its
    /// session after closing its windows – host test).
    quitting: BTreeMap<ContextName, Vec<u32>>,
    /// Running contexts: their workspaces stay while empty. `None` until the
    /// first sync asked the container runtime.
    running: Option<BTreeSet<ContextName>>,
    /// The focused workspace's name.
    workspace: Option<String>,
    /// File manager windows `(address, title)` and the folders found for them
    /// last time: asked again only when a title changes (M5.7 ticks).
    fm_cache: BTreeMap<ContextName, FolderCache>,
    /// The recovery file last written per context, to write only changes.
    recovery_written: BTreeMap<ContextName, String>,
    /// Running contexts whose windows were lost: a restarted kojed found
    /// their recovery file but none of its windows (Hyprland or the session
    /// crashed, the container survived). `koje up` offers to restore them.
    lost: BTreeMap<ContextName, State>,
    /// Memory notifications per context (ADR 0035).
    watches: BTreeMap<ContextName, koje_core::memory::Watch>,
}

/// A context crossed a memory limit (ADR 0035): notify, or at the hard limit
/// shut it down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAlert {
    pub ctx: ContextName,
    pub level: koje_core::memory::MemoryLevel,
    pub used: koje_core::size::ByteSize,
    pub limit: koje_core::size::ByteSize,
}

type FolderCache = (Vec<(String, String)>, BTreeMap<String, String>);

impl<C, R> Daemon<C, R>
where
    C: Compositor,
    R: ContainerRuntime + Clone + Send + 'static,
{
    pub fn new(compositor: C, runtime: R, paths: Paths) -> Self {
        Self {
            compositor,
            runtime,
            paths,
            colors: BTreeMap::new(),
            windows: WindowMap::new(),
            focused: None,
            live: BTreeMap::new(),
            integrations: koje_core::integration::built_in(),
            proc_root: PathBuf::from("/proc"),
            home: std::env::var("HOME").unwrap_or_default(),
            quitting: BTreeMap::new(),
            running: None,
            workspace: None,
            fm_cache: BTreeMap::new(),
            recovery_written: BTreeMap::new(),
            lost: BTreeMap::new(),
            watches: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub fn with_proc_root(mut self, root: PathBuf) -> Self {
        self.proc_root = root;
        self
    }

    fn load_integrations(&mut self) {
        let (all, errors) = koje_core::integration::load(&self.paths.integrations_dir());
        for e in errors {
            tracing::warn!("integration skipped: {e}");
        }
        self.integrations = all;
    }

    /// A context window appeared: into its live state (ADR 0030).
    /// Returns the command placing a restored window from its layout (M5.3).
    fn track(&mut self, ctx: &ContextName, w: &WindowInfo) -> Option<CompositorCommand> {
        let live = self.live.entry(ctx.clone()).or_default();
        // Restored windows wait: what the window's process shows (a terminal's
        // directory) tells several windows of a program apart (M5.10).
        let observed = (!live.pending.is_empty()).then(|| Observed {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            process: crate::procfs::observe(&self.proc_root, w.pid),
            folder: None,
        });
        let added = live.add_observed(
            &w.address,
            &w.app_id,
            observed.as_ref(),
            &self.integrations,
            w.placement.clone(),
        );
        let Some(added) = added else {
            tracing::warn!(address = %w.address, app_id = %w.app_id, "no window id");
            return None;
        };
        tracing::debug!(context = %ctx, id = %added.id, restored = added.place.is_some(), "window tracked");
        if added.stash {
            // Restored, but not in the active layout (M5.5).
            return Some(CompositorCommand::MoveToWorkspace {
                address: w.address.clone(),
                workspace: live_layouts::stash_workspace(ctx),
            });
        }
        added.place.map(|placement| CompositorCommand::Place {
            address: w.address.clone(),
            placement,
        })
    }

    /// `""` → the active context (CLI commands without `-c`).
    pub fn context_or_active(&mut self, given: &str) -> Result<ContextName> {
        if given.is_empty() {
            return self
                .active_context()
                .cloned()
                .context("no active context – pass one with -c <context>");
        }
        let ctx = ContextName::new(given)?;
        self.require(&ctx)?;
        Ok(ctx)
    }

    /// Where the windows of `ctx` are now.
    async fn shown(&self, ctx: &ContextName) -> Result<Vec<Shown>> {
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        Ok(present
            .into_iter()
            .filter(|w| self.windows.context_of(&w.address) == Some(ctx))
            .map(|w| Shown {
                address: w.address,
                workspace: w.workspace,
                placement: w.placement,
            })
            .collect())
    }

    fn live_mut(&mut self, ctx: &ContextName) -> Result<&mut Live> {
        self.live
            .get_mut(ctx)
            .with_context(|| format!("{ctx} has no windows – start it with: koje up {ctx}"))
    }

    /// `koje layout <name>` (M5.5).
    pub async fn switch_layout(&mut self, ctx: &ContextName, name: &LayoutName) -> Result<()> {
        let shown = self.shown(ctx).await?;
        let commands = self.live_mut(ctx)?.switch_layout(ctx, name, &shown)?;
        self.apply(&commands).await
    }

    /// `koje layout next|prev`.
    pub async fn cycle_layout(&mut self, ctx: &ContextName, step: i32) -> Result<LayoutName> {
        let shown = self.shown(ctx).await?;
        let (name, commands) = self.live_mut(ctx)?.cycle_layout(ctx, step, &shown)?;
        self.apply(&commands).await?;
        Ok(name)
    }

    /// `koje sleep` (M7.4): all windows of `ctx` into its stash first.
    pub async fn hide(&mut self, ctx: &ContextName) -> Result<()> {
        let shown = self.shown(ctx).await?;
        let commands = self.live_mut(ctx)?.hide_all(ctx, &shown)?;
        self.apply(&commands).await
    }

    /// `koje wake` (M7.4): the active layout back.
    pub async fn show(&mut self, ctx: &ContextName) -> Result<()> {
        let shown = self.shown(ctx).await?;
        let commands = self.live_mut(ctx)?.show_active(ctx, &shown);
        self.apply(&commands).await
    }

    /// `koje layout new <name> [--copy]`.
    pub async fn new_layout(
        &mut self,
        ctx: &ContextName,
        name: LayoutName,
        copy: bool,
    ) -> Result<()> {
        let shown = self.shown(ctx).await?;
        let commands = self.live_mut(ctx)?.new_layout(ctx, name, copy, &shown)?;
        self.apply(&commands).await
    }

    /// `koje layout save`.
    pub async fn save_layout(&mut self, ctx: &ContextName) -> Result<()> {
        let shown = self.shown(ctx).await?;
        self.live_mut(ctx)?.save_layout(ctx, &shown)?;
        Ok(())
    }

    /// The live state of `ctx` for pure layout edits (`autosave`, `mv`, `rm`).
    pub fn live_state_mut(&mut self, ctx: &ContextName) -> Result<&mut State> {
        Ok(&mut self.live_mut(ctx)?.state)
    }

    pub fn layouts(&mut self, ctx: &ContextName) -> Result<Vec<LayoutInfo>> {
        Ok(self.live_mut(ctx)?.layouts())
    }

    /// `koje stash`: the focused window (M5.6).
    pub async fn stash_focused(&mut self) -> Result<(ContextName, String)> {
        let address = self.focused.clone().context("no window has focus")?;
        let ctx = self
            .windows
            .context_of(&address)
            .cloned()
            .context("the focused window belongs to no context")?;
        let commands = self.live_mut(&ctx)?.stash(&ctx, &address)?;
        let id = self
            .live_mut(&ctx)?
            .windows
            .get(&address)
            .map(|w| w.id.to_string())
            .unwrap_or_default();
        self.apply(&commands).await?;
        Ok((ctx, id))
    }

    /// `koje unstash [<window>] [--to <layout>]`; without a window the
    /// focused one (e.g. picked in the shown stash).
    pub async fn unstash(
        &mut self,
        ctx: &ContextName,
        window: Option<WindowId>,
        to: Option<LayoutName>,
    ) -> Result<WindowId> {
        let id = if let Some(id) = window {
            id
        } else {
            let address = self.focused.clone().context("no window has focus")?;
            self.live_mut(ctx)?
                .windows
                .get(&address)
                .map(|w| w.id.clone())
                .with_context(|| format!("the focused window is not one of {ctx}"))?
        };
        let shown = self.shown(ctx).await?;
        let commands = self.live_mut(ctx)?.unstash(ctx, &id, to.as_ref(), &shown)?;
        self.apply(&commands).await?;
        Ok(id)
    }

    /// `koje stash --show`: show or hide the context's stash.
    pub async fn toggle_stash(&mut self, ctx: &ContextName) -> Result<()> {
        let name = live_layouts::stash_workspace(ctx)
            .trim_start_matches("special:")
            .to_owned();
        self.apply(&[CompositorCommand::ToggleSpecial { name }])
            .await
    }

    /// Start the windows of `state` in the freshly started `ctx` (`koje up`,
    /// M5.3). Windows appearing for them take their ids and places.
    pub async fn restore(
        &mut self,
        ctx: &ContextName,
        state: State,
        name: Option<StateName>,
    ) -> Result<restore::Plan> {
        self.require(ctx)?;
        if self.live.get(ctx).is_some_and(|l| !l.windows.is_empty()) {
            anyhow::bail!(
                "{ctx} already has windows – switching states in a running context comes with M5.9"
            );
        }
        self.lost.remove(ctx);
        let mut live = Live::restoring(state);
        live.current.clone_from(&name);
        self.remember_current(ctx, name.as_ref());
        let plan = restore::plan(&live.pending, &self.integrations, &self.home);
        self.live.insert(ctx.clone(), live);
        self.launch_all(ctx, &plan).await;
        Ok(plan)
    }

    async fn launch_all(&self, ctx: &ContextName, plan: &restore::Plan) {
        for l in &plan.launches {
            let runtime = self.runtime.clone();
            let (c, command, workdir) = (ctx.clone(), l.command.clone(), l.workdir.clone());
            let started = tokio::task::spawn_blocking(move || {
                runtime.launch(&c, &command, workdir.as_deref())
            })
            .await;
            let error = match started {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.to_string()),
                Err(e) => Some(e.to_string()),
            };
            match error {
                None => tracing::info!(context = %ctx, id = %l.id, command = %l.command, "started"),
                Some(e) => {
                    tracing::warn!(context = %ctx, id = %l.id, "starting {}: {e}", l.command);
                }
            }
        }
        for (id, why) in &plan.skipped {
            tracing::info!(context = %ctx, %id, "not started: {why}");
        }
    }

    /// `koje up -s <state>` on a running context (M5.9): keep the windows the
    /// target has, close the others, start its missing ones, arrange by its
    /// active layout. Returns `(kept, closed, started programs)`.
    pub async fn switch_state(
        &mut self,
        ctx: &ContextName,
        target: State,
        name: Option<StateName>,
    ) -> Result<(usize, usize, usize)> {
        self.require(ctx)?;
        self.lost.remove(ctx);
        self.remember_current(ctx, name.as_ref());
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let mine: Vec<&WindowInfo> = present
            .iter()
            .filter(|w| self.windows.context_of(&w.address) == Some(ctx))
            .collect();
        let shown: Vec<Shown> = mine
            .iter()
            .map(|w| Shown {
                address: w.address.clone(),
                workspace: w.workspace.clone(),
                placement: w.placement.clone(),
            })
            .collect();
        let live = self.live_mut(ctx)?;
        live.current = name;
        let change = live.switch_state(ctx, target, &shown);
        // SIGTERM only for programs whose every window goes; a program keeping
        // a window has the others closed one by one.
        let closing: Vec<(String, String, u32)> = mine
            .iter()
            .filter(|w| change.close.contains(&w.address))
            .map(|w| (w.address.clone(), w.app_id.clone(), w.pid))
            .collect();
        let staying: BTreeSet<u32> = mine
            .iter()
            .filter(|w| !change.close.contains(&w.address))
            .map(|w| w.pid)
            .collect();
        let quit = koje_core::capture::plan_quit(&closing, &self.integrations);
        let mut commands: Vec<CompositorCommand> = quit
            .close
            .iter()
            .map(|a| CompositorCommand::CloseWindow { address: a.clone() })
            .collect();
        let mut signal = Vec::new();
        for pid in quit.signal {
            if staying.contains(&pid) {
                commands.extend(
                    closing
                        .iter()
                        .filter(|(_, _, p)| *p == pid)
                        .map(|(a, ..)| CompositorCommand::CloseWindow { address: a.clone() }),
                );
            } else {
                signal.push(pid);
            }
        }
        commands.extend(change.commands);
        self.apply(&commands).await?;
        for pid in signal {
            let runtime = self.runtime.clone();
            if let Ok(Err(e)) = tokio::task::spawn_blocking(move || runtime.terminate(pid)).await {
                tracing::warn!(context = %ctx, pid, "SIGTERM: {e}");
            }
        }
        let plan = restore::plan(&change.start, &self.integrations, &self.home);
        self.launch_all(ctx, &plan).await;
        Ok((change.kept, change.close.len(), plan.launches.len()))
    }

    /// `koje up` started `ctx` (for the bar: running contexts).
    pub async fn started(&mut self, ctx: &ContextName) -> Result<()> {
        self.require(ctx)?;
        // Ask the runtime first: the other running contexts count too.
        self.running().await;
        self.running.get_or_insert_default().insert(ctx.clone());
        Ok(())
    }

    /// Every few seconds (M5.7, M5.8): take the live state of each running
    /// context – the active layout follows moved and resized windows (no
    /// compositor events for that, M2.2) – and write it to the recovery file
    /// when it changed (ADR 0006).
    pub async fn tick(&mut self) {
        // Also contexts without windows: the file must not keep windows that
        // were closed on purpose (`up -s empty`). Not lost ones: their file
        // holds what may be restored.
        let contexts: Vec<ContextName> = self
            .live
            .keys()
            .filter(|c| !self.lost.contains_key(*c))
            .cloned()
            .collect();
        for ctx in contexts {
            let state = match self.state(&ctx).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(context = %ctx, "tick: {e:#}");
                    continue;
                }
            };
            let path = self.paths.recovery_file(&ctx);
            let Ok(text) = state.to_toml(&path) else {
                continue;
            };
            if self.recovery_written.get(&ctx) == Some(&text) {
                continue;
            }
            match koje_core::recovery::write(&self.paths, &ctx, &state) {
                Ok(()) => {
                    tracing::debug!(context = %ctx, "recovery file written");
                    self.recovery_written.insert(ctx, text);
                }
                Err(e) => tracing::warn!(context = %ctx, "recovery file: {e}"),
            }
        }
    }

    /// Every tick (M6.2, M6.3): the working set of each running context with
    /// limits against them, read from the config files each time (always
    /// current, no reload needed).
    pub async fn check_memory(&mut self) -> Vec<MemoryAlert> {
        use koje_core::memory::MemoryLevel;
        let Ok(global) = koje_core::config::GlobalConfig::load(&self.paths.global_config()) else {
            return Vec::new();
        };
        let mut alerts = Vec::new();
        for ctx in self.running().await {
            let Ok(limits) = koje_core::context::load(&self.paths, &ctx)
                .and_then(|c| c.effective_limits(&global))
            else {
                continue;
            };
            if limits.memory_soft.is_none() && limits.memory_hard.is_none() {
                self.watches.remove(&ctx);
                continue;
            }
            let runtime = self.runtime.clone();
            let c = ctx.clone();
            let Ok(Ok(usage)) = tokio::task::spawn_blocking(move || runtime.memory_usage(&c)).await
            else {
                continue;
            };
            let used = usage.working_set();
            let watch = self.watches.entry(ctx.clone()).or_default();
            if let Some(level) = watch.step(used, &limits) {
                let limit = if level == MemoryLevel::OverHard {
                    limits.memory_hard
                } else {
                    limits.memory_soft
                };
                alerts.push(MemoryAlert {
                    ctx,
                    level,
                    used,
                    limit: limit.unwrap_or_default(),
                });
            }
        }
        alerts
    }

    /// Before a hard-limit shutdown: the windows into the state the context is
    /// in, else the `default` role (ADR 0032, 0035).
    pub async fn save_before_shutdown(&mut self, ctx: &ContextName) -> Result<StateName> {
        let state = self.state(ctx).await?;
        let target = match self.current_state(ctx) {
            Some(s) => s.clone(),
            None => koje_core::context::load(&self.paths, ctx)?.states.default,
        };
        koje_core::states::write(&self.paths, ctx, &target, &state)?;
        Ok(target)
    }

    /// Stop the context's container (after its programs were asked to end).
    pub async fn stop_container(&mut self, ctx: &ContextName) -> Result<()> {
        let runtime = self.runtime.clone();
        let c = ctx.clone();
        tokio::task::spawn_blocking(move || runtime.stop(&c))
            .await?
            .map_err(anyhow::Error::new)?;
        self.stopped(ctx).await;
        koje_core::recovery::delete(&self.paths, ctx)?;
        self.watches.remove(ctx);
        Ok(())
    }

    /// After a kojed restart: running contexts get their live state back from
    /// their recovery files, so the windows still open keep ids and layouts
    /// (ADR 0030). Returns those contexts (their unmatched windows expire
    /// after the sync).
    async fn recover_live_states(&mut self) -> Vec<ContextName> {
        let mut recovered = Vec::new();
        for ctx in self.running().await {
            if self.live.contains_key(&ctx) {
                continue;
            }
            match koje_core::recovery::read(&self.paths, &ctx) {
                Ok(Some(r)) => {
                    tracing::info!(context = %ctx, "live state from the recovery file");
                    self.lost.insert(ctx.clone(), r.loaded.state.clone());
                    let mut live = Live::restoring(r.loaded.state);
                    live.current = koje_core::recovery::read_current(&self.paths, &ctx);
                    self.live.insert(ctx.clone(), live);
                    recovered.push(ctx);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(context = %ctx, "recovery file: {e}"),
            }
        }
        recovered
    }

    /// `koje down` stopped `ctx`: its live state is over.
    pub async fn stopped(&mut self, ctx: &ContextName) {
        self.recovery_written.remove(ctx);
        self.lost.remove(ctx);
        self.fm_cache.remove(ctx);
        self.running().await;
        if let Some(r) = self.running.as_mut() {
            r.remove(ctx);
        }
        self.live.remove(ctx);
        self.quitting.remove(ctx);
    }

    /// The running contexts; the first time asked from the container runtime.
    pub async fn running(&mut self) -> BTreeSet<ContextName> {
        if self.running.is_none() {
            let mut found = BTreeSet::new();
            for ctx in self.colors.keys() {
                let runtime = self.runtime.clone();
                let c = ctx.clone();
                if let Ok(Ok(ContainerStatus::Running | ContainerStatus::Paused)) =
                    tokio::task::spawn_blocking(move || runtime.status(&c)).await
                {
                    found.insert(ctx.clone());
                }
            }
            self.running = Some(found);
        }
        self.running.clone().unwrap_or_default()
    }

    /// End the programs of `ctx` in order (`koje down`, M5.4): close their
    /// windows, or SIGTERM programs whose integration says `quit = "signal"`.
    /// The number of windows asked to go.
    pub async fn close_windows(&mut self, ctx: &ContextName) -> Result<usize> {
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let mine: Vec<(String, String, u32)> = present
            .into_iter()
            .filter(|w| self.windows.context_of(&w.address) == Some(ctx))
            .map(|w| (w.address, w.app_id, w.pid))
            .collect();
        let plan = koje_core::capture::plan_quit(&mine, &self.integrations);
        let close: Vec<CompositorCommand> = plan
            .close
            .iter()
            .map(|address| CompositorCommand::CloseWindow {
                address: address.clone(),
            })
            .collect();
        self.apply(&close).await?;
        self.quitting
            .insert(ctx.clone(), plan.signal.iter().copied().collect());
        for pid in plan.signal {
            let runtime = self.runtime.clone();
            match tokio::task::spawn_blocking(move || runtime.terminate(pid)).await {
                Ok(Ok(())) => tracing::info!(context = %ctx, pid, "asked to quit"),
                Ok(Err(e)) => tracing::warn!(context = %ctx, pid, "SIGTERM: {e}"),
                Err(e) => tracing::warn!(context = %ctx, pid, "SIGTERM: {e}"),
            }
        }
        Ok(mine.len())
    }

    /// What of `ctx` has not ended yet: open windows (`id (app id)`) and
    /// signalled processes still running (`process <pid>`).
    pub fn open_windows(&self, ctx: &ContextName) -> Vec<String> {
        let mut open: Vec<String> = self
            .live
            .get(ctx)
            .map(|l| {
                l.windows
                    .values()
                    .map(|w| format!("{} ({})", w.id, w.app_id))
                    .collect()
            })
            .unwrap_or_default();
        for pid in self.quitting.get(ctx).into_iter().flatten() {
            if self.proc_root.join(pid.to_string()).exists() {
                open.push(format!("process {pid} (still quitting)"));
            }
        }
        open
    }

    fn remember_current(&self, ctx: &ContextName, name: Option<&StateName>) {
        if let Err(e) = koje_core::recovery::write_current(&self.paths, ctx, name) {
            tracing::warn!(context = %ctx, "current state: {e}");
        }
    }

    /// The state the running `ctx` is in (ADR 0032); `None` for `empty` or
    /// unknown.
    pub fn current_state(&self, ctx: &ContextName) -> Option<&StateName> {
        self.live.get(ctx).and_then(|l| l.current.as_ref())
    }

    /// Whether the windows of the running `ctx` were lost (see `lost`).
    pub fn is_lost(&self, ctx: &ContextName) -> bool {
        self.lost.contains_key(ctx)
    }

    /// Restored windows of `ctx` that did not appear leave its state.
    pub fn expire_pending(&mut self, ctx: &ContextName) {
        if let Some(live) = self.live.get_mut(ctx) {
            let gone = live.expire_pending();
            if !gone.is_empty() {
                tracing::info!(context = %ctx, ?gone, "restored windows did not appear");
            }
        }
    }

    fn untrack(&mut self, ctx: &ContextName, address: &str) {
        if let Some(live) = self.live.get_mut(ctx) {
            live.remove(address);
        }
    }

    /// The live state of `ctx` (`koje save`): specs from what the windows'
    /// processes show now, placements of the active layout refreshed.
    pub async fn state(&mut self, ctx: &ContextName) -> Result<State> {
        self.require(ctx)?;
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let by_address: BTreeMap<&str, &WindowInfo> =
            present.iter().map(|w| (w.address.as_str(), w)).collect();
        let folders = self.file_manager_folders(ctx, &by_address).await;
        let proc_root = self.proc_root.clone();
        let integrations = &self.integrations;
        let live = self.live.entry(ctx.clone()).or_default();
        Ok(live.snapshot(integrations, |address| {
            let w = by_address.get(address)?;
            let observed = Observed {
                folder: folders.get(address).cloned(),
                app_id: w.app_id.clone(),
                title: w.title.clone(),
                process: crate::procfs::observe(&proc_root, w.pid),
            };
            // Placements only of shown windows (not from the stash, M7.4).
            let shown = w.workspace == windows::workspace(ctx);
            Some((observed, shown.then(|| w.placement.clone())))
        }))
    }

    /// The folders of the context's file manager windows (M5.12): asked once
    /// from the file manager on the context's bus. Empty if there is none or
    /// it does not answer.
    async fn file_manager_folders(
        &mut self,
        ctx: &ContextName,
        by_address: &BTreeMap<&str, &WindowInfo>,
    ) -> BTreeMap<String, String> {
        use koje_core::integration::{Detect, find};
        let windows: Vec<(String, String)> = self
            .live
            .get(ctx)
            .into_iter()
            .flat_map(|l| l.windows.iter())
            .filter(|(_, w)| {
                find(&self.integrations, &w.app_id)
                    .is_some_and(|i| i.detect.values().any(|d| *d == Detect::FileManager))
            })
            .filter_map(|(address, _)| {
                by_address
                    .get(address.as_str())
                    .map(|w| (address.clone(), w.title.clone()))
            })
            .collect();
        if windows.is_empty() {
            return BTreeMap::new();
        }
        if let Some((seen, folders)) = self.fm_cache.get(ctx)
            && *seen == windows
        {
            return folders.clone();
        }
        let folders = self.query_folders(ctx, &windows).await;
        self.fm_cache
            .insert(ctx.clone(), (windows, folders.clone()));
        folders
    }

    async fn query_folders(
        &self,
        ctx: &ContextName,
        windows: &[(String, String)],
    ) -> BTreeMap<String, String> {
        let runtime = self.runtime.clone();
        let c = ctx.clone();
        let reply = tokio::task::spawn_blocking(move || {
            runtime.query(
                &c,
                &[
                    "dbus-send",
                    "--session",
                    "--print-reply",
                    "--dest=org.freedesktop.FileManager1",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.DBus.Properties.Get",
                    "string:org.freedesktop.FileManager1",
                    "string:OpenWindowsWithLocations",
                ],
            )
        })
        .await;
        match reply {
            Ok(Ok(text)) => koje_core::capture::assign_folders(
                windows,
                &koje_core::capture::parse_file_manager_windows(&text),
            ),
            Ok(Err(e)) => {
                tracing::info!(context = %ctx, "file manager folders: {e}");
                BTreeMap::new()
            }
            Err(e) => {
                tracing::info!(context = %ctx, "file manager folders: {e}");
                BTreeMap::new()
            }
        }
    }

    /// Known contexts with their window counts.
    pub fn contexts(&self) -> Vec<ContextInfo> {
        let counts = self.windows.counts();
        self.colors
            .iter()
            .map(|(name, color)| ContextInfo {
                name: name.clone(),
                color: *color,
                windows: counts.get(name).copied().unwrap_or(0),
            })
            .collect()
    }

    /// The context of the focused window, if it belongs to one.
    /// The context of the focused workspace (also when it has no windows),
    /// else of the focused window.
    pub fn active_context(&self) -> Option<&ContextName> {
        self.workspace
            .as_deref()
            .and_then(|w| self.colors.keys().find(|c| windows::workspace(c) == w))
            .or_else(|| {
                self.focused
                    .as_deref()
                    .and_then(|a| self.windows.context_of(a))
            })
    }

    /// Fail unless `ctx` is a context (re-reading the contexts once); the
    /// commands for any color changes that brought.
    fn require(&mut self, ctx: &ContextName) -> Result<Vec<CompositorCommand>> {
        let mut commands = Vec::new();
        if !self.colors.contains_key(ctx) {
            commands = self.load_colors();
        }
        if !self.colors.contains_key(ctx) {
            anyhow::bail!("no context named {ctx}");
        }
        Ok(commands)
    }

    /// Whether `ctx` is a context.
    pub fn knows(&mut self, ctx: &ContextName) -> bool {
        self.require(ctx).is_ok()
    }

    /// Show a context's workspace (`koje focus`).
    pub async fn focus(&mut self, ctx: &ContextName) -> Result<()> {
        let mut commands = self.require(ctx)?;
        commands.push(windows::focus(ctx));
        self.apply(&commands).await
    }

    /// Read the contexts again (after `koje new`/`koje edit`) and send their
    /// rules. Without a compositor that is fine: rules follow on `Connected`.
    pub async fn reload(&mut self) -> Result<()> {
        self.load_integrations();
        let list = koje_core::context::list(&self.paths).context("reading the contexts")?;
        let recolor = self.set_colors(list.into_iter().map(|c| (c.name, c.color)).collect());
        let mut commands = self.rules();
        commands.extend(recolor);
        if let Err(e) = self.apply(&commands).await {
            tracing::info!("rules not sent: {e:#}");
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn windows(&self) -> &WindowMap {
        &self.windows
    }

    pub async fn handle(&mut self, event: CompositorEvent) -> Result<Vec<Change>> {
        match event {
            CompositorEvent::Connected => self.sync().await,
            CompositorEvent::ConfigReloaded => {
                // Runtime rules are gone after a reload (ADR 0018); tags are
                // set again too, which is idempotent.
                let mut commands = self.rules();
                commands.extend(self.windows.tags());
                self.apply(&commands).await.map(|()| Vec::new())
            }
            CompositorEvent::WindowOpened { address, .. } => self.opened(&address).await,
            CompositorEvent::WindowClosed { address } => {
                let context = self.windows.remove(&address);
                if let Some(ctx) = &context {
                    self.untrack(ctx, &address);
                }
                Ok(context
                    .map(|context| Change::WindowRemoved { address, context })
                    .into_iter()
                    .collect())
            }
            CompositorEvent::WindowFocused { address } => {
                self.focused = address;
                Ok(Vec::new())
            }
            CompositorEvent::WorkspaceFocused { name } => {
                self.workspace = Some(name);
                Ok(Vec::new())
            }
            CompositorEvent::WindowTitle { .. } => Ok(Vec::new()),
        }
    }

    /// Start and every (re)connection: contexts, rules, all windows.
    async fn sync(&mut self) -> Result<Vec<Change>> {
        let mut recolor = self.load_colors();
        match self.compositor.active_workspace().await {
            Ok(name) => self.workspace = Some(name),
            Err(e) => tracing::debug!("active workspace: {e}"),
        }
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let diff = self
            .windows
            .retain(present.iter().map(|w| w.address.as_str()));
        for (address, ctx) in &diff.removed {
            self.untrack(ctx, address);
        }
        let mut changes: Vec<Change> = diff
            .removed
            .into_iter()
            .map(|(address, context)| Change::WindowRemoved { address, context })
            .collect();
        let recovered = self.recover_live_states().await;
        let mut moves = Vec::new();
        for w in present.iter().filter(|w| diff.added.contains(&w.address)) {
            let ctx = self.resolve(w.pid).await;
            if let Some(context) = &ctx {
                let place = self.track(context, w);
                moves.extend(windows::move_to_context(&w.address, &w.workspace, context));
                moves.extend(place);
                changes.push(Change::WindowAdded {
                    address: w.address.clone(),
                    context: context.clone(),
                });
            }
            self.windows.insert(&w.address, w.pid, ctx);
        }
        // Windows of a recovered state that are not there any more. If none
        // is left, the context's windows were lost (kept for `koje up`).
        for ctx in &recovered {
            self.expire_pending(ctx);
            let empty = self.live.get(ctx).is_none_or(|l| l.windows.is_empty());
            let had_windows = self.lost.get(ctx).is_some_and(|s| !s.windows.is_empty());
            if empty && had_windows {
                tracing::info!(context = %ctx, "its windows are gone: koje up offers to restore them");
            } else {
                self.lost.remove(ctx);
            }
        }
        // Contexts created since the colors were loaded.
        if self
            .windows
            .contexts()
            .iter()
            .any(|c| !self.colors.contains_key(*c))
        {
            recolor.extend(self.load_colors());
        }
        let mut commands = self.rules();
        commands.extend(self.windows.tags());
        commands.extend(recolor);
        commands.extend(moves);
        tracing::info!(
            windows = present.len(),
            contexts = %self
                .windows
                .counts()
                .iter()
                .map(|(c, n)| format!("{c}={n}"))
                .collect::<Vec<_>>()
                .join(" "),
            "synced with the compositor"
        );
        self.apply(&commands).await?;
        Ok(changes)
    }

    async fn opened(&mut self, address: &str) -> Result<Vec<Change>> {
        if self.windows.contains(address) {
            return Ok(Vec::new());
        }
        // Events carry no pid; the window list has it.
        let all = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let Some(w) = all.into_iter().find(|w| w.address == address) else {
            return Ok(Vec::new()); // closed again already
        };
        let ctx = self.resolve(w.pid).await;
        let mut commands = Vec::new();
        if let Some(c) = &ctx
            && !self.colors.contains_key(c)
        {
            // A context created after kojed started.
            commands.extend(self.load_colors());
            commands.extend(self.colors.get(c).map(|color| windows::rule(c, *color)));
        }
        tracing::debug!(address, pid = w.pid, context = ?ctx.as_ref().map(ContextName::as_str), "window opened");
        let place = ctx.as_ref().and_then(|c| self.track(c, &w));
        let change = ctx.clone().map(|context| {
            commands.push(windows::tag_window(address, &context));
            commands.extend(windows::move_to_context(address, &w.workspace, &context));
            commands.extend(place);
            Change::WindowAdded {
                address: address.to_owned(),
                context,
            }
        });
        self.windows.insert(address, w.pid, ctx);
        self.apply(&commands).await?;
        Ok(change.into_iter().collect())
    }

    /// The context of a process: cached per pid while it has windows, else
    /// asked from the container runtime (blocking: podman runs a process).
    async fn resolve(&self, pid: u32) -> Option<ContextName> {
        if let Some(known) = self.windows.known_pid(pid) {
            return known;
        }
        if pid == 0 {
            return None;
        }
        let runtime = self.runtime.clone();
        match tokio::task::spawn_blocking(move || runtime.context_of_pid(pid)).await {
            Ok(Ok(ctx)) => ctx,
            Ok(Err(e)) => {
                tracing::warn!(pid, "cannot tell the context of a window: {e}");
                None
            }
            Err(e) => {
                tracing::warn!(pid, "context lookup failed: {e}");
                None
            }
        }
    }

    /// Context colors from the config files; on error the old ones stay.
    /// Returns the commands for windows whose context changed color.
    fn load_colors(&mut self) -> Vec<CompositorCommand> {
        match koje_core::context::list(&self.paths) {
            Ok(list) => self.set_colors(list.into_iter().map(|c| (c.name, c.color)).collect()),
            Err(e) => {
                tracing::warn!("reading the contexts: {e}");
                Vec::new()
            }
        }
    }

    /// New context colors. A rule only colors windows opened after it, so the
    /// open windows of a context whose color changed get their colors set
    /// directly (host test of M4.5).
    fn set_colors(&mut self, colors: BTreeMap<ContextName, Color>) -> Vec<CompositorCommand> {
        let old = std::mem::replace(&mut self.colors, colors);
        self.colors
            .iter()
            .filter(|(ctx, color)| old.get(*ctx).is_some_and(|o| o != *color))
            .flat_map(|(ctx, color)| self.windows.recolor(ctx, *color))
            .collect()
    }

    fn rules(&self) -> Vec<CompositorCommand> {
        self.colors
            .iter()
            .map(|(ctx, color)| windows::rule(ctx, *color))
            .collect()
    }

    async fn apply(&self, commands: &[CompositorCommand]) -> Result<()> {
        self.compositor
            .apply(commands)
            .await
            .context("sending commands to the compositor")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use koje_core::layout::{Placement, Rect};
    use koje_core::memory::MemoryUsage;
    use koje_core::ports::WindowInfo;
    use koje_core::spec::ContainerSpec;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeCompositor {
        windows: Mutex<Vec<WindowInfo>>,
        applied: Mutex<Vec<CompositorCommand>>,
    }

    impl FakeCompositor {
        fn open(&self, address: &str, pid: u32) {
            self.windows.lock().unwrap().push(WindowInfo {
                address: address.into(),
                pid,
                app_id: "foot".into(),
                title: String::new(),
                workspace: "1".into(),
                placement: Placement {
                    monitor: "DP-1".into(),
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 1,
                        h: 1,
                    },
                },
            });
        }

        fn close(&self, address: &str) {
            self.windows
                .lock()
                .unwrap()
                .retain(|w| w.address != address);
        }

        fn take(&self) -> Vec<CompositorCommand> {
            std::mem::take(&mut *self.applied.lock().unwrap())
        }
    }

    impl Compositor for &FakeCompositor {
        type Error = std::io::Error;

        fn windows(&self) -> impl Future<Output = Result<Vec<WindowInfo>, Self::Error>> + Send {
            std::future::ready(Ok(self.windows.lock().unwrap().clone()))
        }

        fn active_workspace(&self) -> impl Future<Output = Result<String, Self::Error>> + Send {
            std::future::ready(Ok("1".to_owned()))
        }

        fn apply(
            &self,
            commands: &[CompositorCommand],
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            self.applied.lock().unwrap().extend_from_slice(commands);
            std::future::ready(Ok(()))
        }
    }

    type Launched = (String, Option<String>);

    /// pid → context; counts lookups.
    #[derive(Debug, Clone, Default)]
    struct FakeRuntime {
        pids: Arc<Mutex<HashMap<u32, ContextName>>>,
        lookups: Arc<Mutex<u32>>,
        /// `(command, workdir)` of every launch.
        launched: Arc<Mutex<Vec<Launched>>>,
        terminated: Arc<Mutex<Vec<u32>>>,
        query_reply: Arc<Mutex<String>>,
        memory: Arc<Mutex<HashMap<ContextName, MemoryUsage>>>,
    }

    impl ContainerRuntime for FakeRuntime {
        type Error = std::io::Error;

        fn status(&self, _: &ContextName) -> std::io::Result<ContainerStatus> {
            Ok(ContainerStatus::Running)
        }
        fn create(&self, _: &ContainerSpec) -> std::io::Result<()> {
            Ok(())
        }
        fn remove(&self, _: &ContextName) -> std::io::Result<()> {
            Ok(())
        }
        fn start(&self, _: &ContextName) -> std::io::Result<()> {
            Ok(())
        }
        fn stop(&self, _: &ContextName) -> std::io::Result<()> {
            Ok(())
        }
        fn pause(&self, _: &ContextName) -> std::io::Result<()> {
            Ok(())
        }
        fn unpause(&self, _: &ContextName) -> std::io::Result<()> {
            Ok(())
        }
        fn memory_usage(&self, ctx: &ContextName) -> std::io::Result<MemoryUsage> {
            Ok(self
                .memory
                .lock()
                .unwrap()
                .get(ctx)
                .copied()
                .unwrap_or_default())
        }
        fn launch(
            &self,
            _: &ContextName,
            command: &str,
            workdir: Option<&str>,
        ) -> std::io::Result<()> {
            self.launched
                .lock()
                .unwrap()
                .push((command.to_owned(), workdir.map(str::to_owned)));
            Ok(())
        }
        fn query(&self, _: &ContextName, argv: &[&str]) -> std::io::Result<String> {
            assert_eq!(argv[0], "dbus-send");
            Ok(self.query_reply.lock().unwrap().clone())
        }
        fn terminate(&self, pid: u32) -> std::io::Result<()> {
            self.terminated.lock().unwrap().push(pid);
            Ok(())
        }
        fn context_of_pid(&self, pid: u32) -> std::io::Result<Option<ContextName>> {
            *self.lookups.lock().unwrap() += 1;
            Ok(self.pids.lock().unwrap().get(&pid).cloned())
        }
    }

    fn ctx(s: &str) -> ContextName {
        ContextName::new(s).unwrap()
    }

    fn red() -> Color {
        Color::rgb(0xff, 0, 0)
    }

    struct Setup {
        tmp: tempfile::TempDir,
        paths: Paths,
        runtime: FakeRuntime,
    }

    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = Paths::with_roots(root.join("c"), root.join("d"), root.join("s"));
        koje_core::context::create(&paths, &ctx("work"), None, Some(red())).unwrap();
        let runtime = FakeRuntime::default();
        runtime.pids.lock().unwrap().insert(10, ctx("work"));
        Setup {
            tmp,
            paths,
            runtime,
        }
    }

    fn mv(address: &str, ws: &str) -> CompositorCommand {
        CompositorCommand::MoveToWorkspace {
            address: address.into(),
            workspace: ws.into(),
        }
    }

    fn tag(address: &str, t: &str) -> CompositorCommand {
        CompositorCommand::TagWindow {
            address: address.into(),
            tag: t.into(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn colors_existing_and_new_windows() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        comp.open("0xh", 99); // a host window
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());

        let changes = d.handle(CompositorEvent::Connected).await.unwrap();
        assert_eq!(
            comp.take(),
            [
                windows::rule(&ctx("work"), red()),
                tag("0xa", "koje-work"),
                mv("0xa", "work")
            ]
        );
        assert_eq!(
            changes,
            [Change::WindowAdded {
                address: "0xa".into(),
                context: ctx("work")
            }]
        );
        d.handle(CompositorEvent::WindowFocused {
            address: Some("0xa".into()),
        })
        .await
        .unwrap();
        assert_eq!(d.active_context(), Some(&ctx("work")));
        d.handle(CompositorEvent::WindowFocused {
            address: Some("0xh".into()),
        })
        .await
        .unwrap();
        assert_eq!(d.active_context(), None, "a host window");

        // A second window of the same process: no new lookup.
        comp.open("0xb", 10);
        let lookups = *s.runtime.lookups.lock().unwrap();
        d.handle(CompositorEvent::WindowOpened {
            address: "0xb".into(),
            workspace: "1".into(),
            class: "foot".into(),
            title: String::new(),
        })
        .await
        .unwrap();
        assert_eq!(comp.take(), [tag("0xb", "koje-work"), mv("0xb", "work")]);
        assert_eq!(*s.runtime.lookups.lock().unwrap(), lookups);

        // A reload drops the rules: rule and tags again.
        d.handle(CompositorEvent::ConfigReloaded).await.unwrap();
        assert_eq!(
            comp.take(),
            [
                windows::rule(&ctx("work"), red()),
                tag("0xa", "koje-work"),
                tag("0xb", "koje-work")
            ]
        );

        let changes = d
            .handle(CompositorEvent::WindowClosed {
                address: "0xb".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            changes,
            [Change::WindowRemoved {
                address: "0xb".into(),
                context: ctx("work")
            }]
        );
        assert!(!d.windows().contains("0xb"));
        assert_eq!(
            d.contexts(),
            [ContextInfo {
                name: ctx("work"),
                color: red(),
                windows: 1
            }]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn context_created_after_start() {
        let s = setup();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();

        let blue = Color::rgb(0, 0, 0xff);
        koje_core::context::create(&s.paths, &ctx("new"), None, Some(blue)).unwrap();
        s.runtime.pids.lock().unwrap().insert(20, ctx("new"));
        comp.open("0xn", 20);
        d.handle(CompositorEvent::WindowOpened {
            address: "0xn".into(),
            workspace: "1".into(),
            class: "foot".into(),
            title: String::new(),
        })
        .await
        .unwrap();
        assert_eq!(
            comp.take(),
            [
                windows::rule(&ctx("new"), blue),
                tag("0xn", "koje-new"),
                mv("0xn", "new")
            ]
        );
    }

    /// Host test of M4.5: after `koje edit` only new windows had the new color.
    #[tokio::test(flavor = "current_thread")]
    async fn color_change_reaches_open_windows() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();

        let blue = Color::rgb(0, 0, 0xff);
        let path = s.paths.context_config(&ctx("work"));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("#FF0000", "#0000FF");
        koje_core::context::replace_config(&s.paths, &ctx("work"), &text).unwrap();
        d.reload().await.unwrap();
        assert_eq!(
            comp.take(),
            [
                windows::rule(&ctx("work"), blue),
                CompositorCommand::BorderColors {
                    address: "0xa".into(),
                    active: blue,
                    inactive: blue.dimmed()
                }
            ]
        );
        // Unchanged colors: only the rules again.
        d.reload().await.unwrap();
        assert_eq!(comp.take(), [windows::rule(&ctx("work"), blue)]);
    }

    /// A fake `/proc`: pid 10 is foot with one shell in `/home/mj/src/kp`.
    fn fake_proc(root: &std::path::Path) {
        for (pid, cmdline, cwd) in [
            (10, &b"foot\0"[..], "/home/mj"),
            (11, &b"-bash\0"[..], "/home/mj/src/kp"),
        ] {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(d.join("task").join(pid.to_string())).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
            std::os::unix::fs::symlink(cwd, d.join("cwd")).unwrap();
        }
        std::fs::write(root.join("10/task/10/children"), "11").unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn live_state_of_a_context() {
        let s = setup();
        let proc_root = s.tmp.path().join("proc");
        fake_proc(&proc_root);
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        comp.open("0xh", 99);
        let mut d =
            Daemon::new(&comp, s.runtime.clone(), s.paths.clone()).with_proc_root(proc_root);
        d.handle(CompositorEvent::Connected).await.unwrap();

        let state = d.state(&ctx("work")).await.unwrap();
        assert_eq!(state.windows.len(), 1, "host windows are not part of it");
        let w = &state.windows[0];
        assert_eq!(
            (w.id.as_str(), w.integration.as_deref()),
            ("terminal", Some("terminal"))
        );
        assert_eq!(w.remembered["cwd"].as_str(), Some("/home/mj/src/kp"));
        assert_eq!(w.command.as_deref(), Some("foot"));
        let main = koje_core::name::LayoutName::initial();
        assert!(state.layout[&main].windows.contains_key(&w.id));
        // The state file format round-trips.
        let text = state.to_toml(std::path::Path::new("x")).unwrap();
        assert_eq!(
            State::from_toml(&text, std::path::Path::new("x")).unwrap(),
            state
        );

        d.handle(CompositorEvent::WindowClosed {
            address: "0xa".into(),
        })
        .await
        .unwrap();
        comp.close("0xa");
        let state = d.state(&ctx("work")).await.unwrap();
        assert!(state.windows.is_empty() && state.layout[&main].windows.is_empty());
        assert!(d.state(&ctx("nope")).await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restore_starts_programs_and_places_their_windows() {
        let s = setup();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.home = "/home/mj".into();
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();

        let saved = State::parse(
            r#"
active_layout = "main"
[[windows]]
id = "term"
integration = "terminal"
command = "foot"
app_id = "foot"
cwd = "~/src/kp"
[[windows]]
id = "web"
integration = "browser"
command = "firefox"
app_id = "firefox"
[[windows]]
id = "web-2"
integration = "browser"
command = "firefox"
app_id = "firefox"
[layout.main.windows]
term = { monitor = "DP-1", x = 10, y = 20, w = 300, h = 200 }
"#,
            std::path::Path::new("default.toml"),
        )
        .unwrap()
        .state;
        let plan = d.restore(&ctx("work"), saved, None).await.unwrap();
        assert_eq!(
            plan.launches.len(),
            2,
            "one firefox for both browser windows"
        );
        assert_eq!(
            *s.runtime.launched.lock().unwrap(),
            [
                ("foot".to_owned(), Some("/home/mj/src/kp".to_owned())),
                ("firefox".to_owned(), None)
            ]
        );

        // The terminal's window appears: it takes `term` and its place.
        comp.open("0xt", 10);
        d.handle(CompositorEvent::WindowOpened {
            address: "0xt".into(),
            workspace: "1".into(),
            class: "foot".into(),
            title: String::new(),
        })
        .await
        .unwrap();
        let place = comp
            .take()
            .into_iter()
            .find(|c| matches!(c, CompositorCommand::Place { .. }));
        assert_eq!(
            place,
            Some(CompositorCommand::Place {
                address: "0xt".into(),
                placement: koje_core::layout::Placement {
                    monitor: "DP-1".into(),
                    rect: koje_core::layout::Rect {
                        x: 10,
                        y: 20,
                        w: 300,
                        h: 200
                    },
                },
            })
        );
        assert!(
            d.restore(
                &ctx("work"),
                State::new(koje_core::name::LayoutName::initial()),
                None
            )
            .await
            .is_err(),
            "has windows now"
        );
        d.expire_pending(&ctx("work"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn close_windows_in_order() {
        let s = setup();
        s.runtime.pids.lock().unwrap().insert(20, ctx("work"));
        let comp = FakeCompositor::default();
        comp.open("0xa", 10); // foot
        comp.open("0xh", 99); // a host window: untouched
        comp.open("0xf", 20);
        comp.open("0xg", 20);
        for w in comp.windows.lock().unwrap().iter_mut() {
            if w.pid == 20 {
                w.app_id = "firefox".into();
            }
        }
        let proc_root = s.tmp.path().join("proc");
        std::fs::create_dir_all(&proc_root).unwrap();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone())
            .with_proc_root(proc_root.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();
        assert_eq!(d.open_windows(&ctx("work")).len(), 3);

        assert_eq!(d.close_windows(&ctx("work")).await.unwrap(), 3);
        assert_eq!(
            comp.take(),
            [CompositorCommand::CloseWindow {
                address: "0xa".into()
            }]
        );
        assert_eq!(
            *s.runtime.terminated.lock().unwrap(),
            [20],
            "Firefox: one SIGTERM"
        );
        for a in ["0xa", "0xf", "0xg"] {
            d.handle(CompositorEvent::WindowClosed { address: a.into() })
                .await
                .unwrap();
        }
        // Its windows are gone, the process still writes its session.
        std::fs::create_dir_all(proc_root.join("20")).unwrap();
        assert_eq!(
            d.open_windows(&ctx("work")),
            ["process 20 (still quitting)"]
        );
        std::fs::remove_dir(proc_root.join("20")).unwrap();
        assert_eq!(d.open_windows(&ctx("work")), Vec::<String>::new());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn file_manager_windows_remember_their_folders() {
        let s = setup();
        s.runtime.pids.lock().unwrap().insert(30, ctx("work"));
        *s.runtime.query_reply.lock().unwrap() = r#"   variant       array [
         dict entry(
            string "/org/gnome/Nautilus/window/2"
            array [
               string "file:///home/mj/Downloads"
               string "file:///home/mj/Old%20Data"
            ]
         )
         dict entry(
            string "/org/gnome/Nautilus/window/1"
            array [
               string "file:///home/mj"
            ]
         )
      ]"#
        .to_owned();
        let comp = FakeCompositor::default();
        comp.open("0xn", 30);
        comp.open("0xm", 30);
        for (w, title) in comp
            .windows
            .lock()
            .unwrap()
            .iter_mut()
            .zip(["Home", "Old Data"])
        {
            w.app_id = "org.gnome.Nautilus".into();
            w.title = title.into();
        }
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let state = d.state(&ctx("work")).await.unwrap();
        let paths: Vec<(&str, Option<&str>)> = state
            .windows
            .iter()
            .map(|w| {
                (
                    w.id.as_str(),
                    w.remembered.get("path").and_then(|v| v.as_str()),
                )
            })
            .collect();
        assert_eq!(
            paths,
            [
                ("files", Some("/home/mj")),
                ("files-2", Some("/home/mj/Old Data"))
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_empty_context_is_active_on_its_workspace() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xh", 99); // a host window
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        assert_eq!(d.active_context(), None, "workspace 1");
        d.handle(CompositorEvent::WorkspaceFocused {
            name: "work".into(),
        })
        .await
        .unwrap();
        assert_eq!(
            d.active_context(),
            Some(&ctx("work")),
            "no windows, still active"
        );
        d.handle(CompositorEvent::WorkspaceFocused { name: "2".into() })
            .await
            .unwrap();
        assert_eq!(d.active_context(), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn running_contexts() {
        let s = setup();
        koje_core::context::create(&s.paths, &ctx("home"), None, None).unwrap();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        // At start: what the runtime reports (the fake: all running).
        assert_eq!(
            d.running().await,
            BTreeSet::from([ctx("home"), ctx("work")])
        );
        d.stopped(&ctx("home")).await;
        assert_eq!(d.running().await, BTreeSet::from([ctx("work")]));
        d.started(&ctx("home")).await.unwrap();
        assert_eq!(d.running().await.len(), 2);
        assert!(d.started(&ctx("nope")).await.is_err());

        // kojed restarted: `Started` before anything asked the runtime still
        // knows the contexts that were running.
        let mut fresh = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        fresh.reload().await.unwrap();
        fresh.started(&ctx("home")).await.unwrap();
        assert_eq!(
            fresh.running().await,
            BTreeSet::from([ctx("home"), ctx("work")])
        );
        assert!(
            comp.take()
                .iter()
                .all(|c| matches!(c, CompositorCommand::ContextRule { .. })),
            "only the sync's rules – no workspace commands"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn layouts_and_the_stash() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        comp.open("0xb", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();
        let work = ctx("work");
        let stash_ws = "special:stash-work";
        let to_stash = |a: &str| CompositorCommand::MoveToWorkspace {
            address: a.into(),
            workspace: stash_ws.into(),
        };

        d.new_layout(&work, LayoutName::new("review").unwrap(), false)
            .await
            .unwrap();
        assert_eq!(comp.take(), [to_stash("0xa"), to_stash("0xb")]);
        for w in comp.windows.lock().unwrap().iter_mut() {
            w.workspace = stash_ws.into();
        }
        assert_eq!(
            d.unstash(&work, Some(WindowId::new("terminal").unwrap()), None)
                .await
                .unwrap()
                .as_str(),
            "terminal"
        );
        assert_eq!(
            comp.take(),
            [CompositorCommand::MoveToWorkspace {
                address: "0xa".into(),
                workspace: "work".into()
            }]
        );
        let names: Vec<(String, bool, usize)> = d
            .layouts(&work)
            .unwrap()
            .into_iter()
            .map(|l| (l.name.to_string(), l.active, l.windows))
            .collect();
        assert_eq!(
            names,
            [("main".into(), false, 2), ("review".into(), true, 1)]
        );

        // `koje stash` takes the focused window.
        d.handle(CompositorEvent::WindowFocused {
            address: Some("0xa".into()),
        })
        .await
        .unwrap();
        assert_eq!(
            d.stash_focused().await.unwrap(),
            (work.clone(), "terminal".to_owned())
        );
        assert_eq!(comp.take(), [to_stash("0xa")]);
        assert_eq!(d.context_or_active("").unwrap(), work);

        assert_eq!(d.cycle_layout(&work, 1).await.unwrap().as_str(), "main");
        d.toggle_stash(&work).await.unwrap();
        assert!(comp.take().contains(&CompositorCommand::ToggleSpecial {
            name: "stash-work".into()
        }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restored_windows_outside_the_active_layout_go_to_the_stash() {
        let s = setup();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let saved = State::parse(
            r#"
active_layout = "main"
[[windows]]
id = "term"
command = "foot"
app_id = "foot"
[layout.main]
[layout.review.windows]
term = { monitor = "DP-1", x = 1, y = 2, w = 3, h = 4 }
"#,
            std::path::Path::new("x"),
        )
        .unwrap()
        .state;
        d.restore(&ctx("work"), saved, None).await.unwrap();
        comp.take();
        comp.open("0xt", 10);
        d.handle(CompositorEvent::WindowOpened {
            address: "0xt".into(),
            workspace: "1".into(),
            class: "foot".into(),
            title: String::new(),
        })
        .await
        .unwrap();
        assert!(comp.take().contains(&CompositorCommand::MoveToWorkspace {
            address: "0xt".into(),
            workspace: "special:stash-work".into(),
        }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tick_autosaves_and_writes_the_recovery_file() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let file = s.paths.recovery_file(&ctx("work"));
        d.tick().await;
        let first = std::fs::read_to_string(&file).unwrap();
        assert!(first.contains("id = \"terminal\""), "{first}");
        // Unchanged: not written again.
        std::fs::remove_file(&file).unwrap();
        d.tick().await;
        assert!(!file.exists());
        // Moved (no compositor event for that): the active layout follows –
        // for a window on the context's workspace (kojed moved it there).
        comp.windows.lock().unwrap()[0].workspace = "work".into();
        comp.windows.lock().unwrap()[0].placement.rect.x = 777;
        d.tick().await;
        assert!(std::fs::read_to_string(&file).unwrap().contains("x = 777"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_restarted_kojed_takes_the_live_state_from_the_recovery_file() {
        let s = setup();
        koje_core::recovery::write(
            &s.paths,
            &ctx("work"),
            &State::parse(
                r#"
active_layout = "review"
[[windows]]
id = "shell"
integration = "terminal"
command = "foot"
app_id = "foot"
[[windows]]
id = "gone"
command = "evince"
app_id = "org.gnome.Evince"
[layout.main.windows]
shell = { monitor = "DP-1", x = 0, y = 0, w = 1, h = 1 }
[layout.review.windows]
shell = { monitor = "DP-1", x = 5, y = 5, w = 1, h = 1 }
"#,
                std::path::Path::new("x"),
            )
            .unwrap()
            .state,
        )
        .unwrap();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let state = d.state(&ctx("work")).await.unwrap();
        assert_eq!(state.active_layout.as_str(), "review", "layouts kept");
        let ids: Vec<&str> = state.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["shell"], "renamed id kept; the closed window left");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn switch_state_closes_without_killing_kept_windows() {
        let s = setup();
        s.runtime.pids.lock().unwrap().insert(20, ctx("work"));
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        comp.open("0xf", 20);
        comp.open("0xg", 20);
        for w in comp
            .windows
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|w| w.pid == 20)
        {
            w.app_id = "firefox".into();
        }
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();
        let target = State::parse(
            r#"
active_layout = "main"
[[windows]]
id = "browser"
integration = "browser"
command = "firefox"
app_id = "firefox"
[[windows]]
id = "viewer"
command = "evince talk.pdf"
app_id = "org.gnome.Evince"
[layout.main.windows]
browser = { monitor = "DP-1", x = 1, y = 2, w = 3, h = 4 }
"#,
            std::path::Path::new("talk.toml"),
        )
        .unwrap()
        .state;
        let (kept, closed, started) = d.switch_state(&ctx("work"), target, None).await.unwrap();
        assert_eq!((kept, closed, started), (1, 2, 1));
        let commands = comp.take();
        for a in ["0xa", "0xg"] {
            assert!(
                commands.contains(&CompositorCommand::CloseWindow { address: a.into() }),
                "{a} closed: {commands:?}"
            );
        }
        assert_eq!(
            *s.runtime.terminated.lock().unwrap(),
            Vec::<u32>::new(),
            "Firefox keeps a window: no SIGTERM"
        );
        assert_eq!(s.runtime.launched.lock().unwrap()[0].0, "evince talk.pdf");
    }

    /// Host test of M5.9: after `up -s empty` the recovery file still had
    /// the old windows, and `up -s default` asked to recover them.
    #[tokio::test(flavor = "current_thread")]
    async fn an_emptied_context_has_an_empty_recovery_file() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        d.tick().await;
        let file = s.paths.recovery_file(&ctx("work"));
        assert!(std::fs::read_to_string(&file).unwrap().contains("terminal"));
        d.switch_state(
            &ctx("work"),
            State::new(koje_core::name::LayoutName::initial()),
            None,
        )
        .await
        .unwrap();
        d.handle(CompositorEvent::WindowClosed {
            address: "0xa".into(),
        })
        .await
        .unwrap();
        comp.close("0xa");
        d.tick().await;
        assert!(!std::fs::read_to_string(&file).unwrap().contains("terminal"));
        assert!(!d.is_lost(&ctx("work")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn windows_lost_while_the_context_ran() {
        let s = setup();
        let saved = State::parse(
            "active_layout = \"main\"\n[[windows]]\nid = \"term\"\ncommand = \"foot\"\napp_id = \"foot\"\n",
            std::path::Path::new("x"),
        )
        .unwrap()
        .state;
        koje_core::recovery::write(&s.paths, &ctx("work"), &saved).unwrap();
        // kojed restarts after a Hyprland crash: the container runs, no windows.
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        assert!(d.is_lost(&ctx("work")));
        d.tick().await;
        let file = s.paths.recovery_file(&ctx("work"));
        assert!(
            std::fs::read_to_string(&file).unwrap().contains("term"),
            "kept for koje up"
        );
        d.restore(&ctx("work"), saved, None).await.unwrap();
        assert!(!d.is_lost(&ctx("work")));
    }

    /// Host test of M5.9: switching from `empty` to `default` saved the
    /// empty state into `default` (ADR 0032: save into the current state).
    #[tokio::test(flavor = "current_thread")]
    async fn the_current_state_is_remembered() {
        let s = setup();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let work = ctx("work");
        let talk = StateName::new("talk").unwrap();
        let empty_state = || State::new(koje_core::name::LayoutName::initial());
        d.restore(&work, empty_state(), Some(talk.clone()))
            .await
            .unwrap();
        assert_eq!(d.current_state(&work), Some(&talk));
        assert_eq!(
            koje_core::recovery::read_current(&s.paths, &work),
            Some(talk.clone())
        );
        d.switch_state(&work, empty_state(), None).await.unwrap();
        assert_eq!(d.current_state(&work), None, "empty");
        assert_eq!(koje_core::recovery::read_current(&s.paths, &work), None);

        // A restarted kojed reads it back with the recovery file.
        d.switch_state(&work, empty_state(), Some(talk.clone()))
            .await
            .unwrap();
        koje_core::recovery::write(&s.paths, &work, &empty_state()).unwrap();
        let mut again = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        again.handle(CompositorEvent::Connected).await.unwrap();
        assert_eq!(again.current_state(&work), Some(&talk));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restored_terminals_find_their_ids_by_directory() {
        let s = setup();
        let proc_root = s.tmp.path().join("proc");
        fake_proc(&proc_root); // pid 10: foot, its shell in /home/mj/src/kp
        let comp = FakeCompositor::default();
        let mut d =
            Daemon::new(&comp, s.runtime.clone(), s.paths.clone()).with_proc_root(proc_root);
        d.handle(CompositorEvent::Connected).await.unwrap();
        let saved = State::parse(
            r#"
active_layout = "main"
[[windows]]
id = "elsewhere"
integration = "terminal"
command = "foot"
app_id = "foot"
cwd = "/tmp"
[[windows]]
id = "project"
integration = "terminal"
command = "foot"
app_id = "foot"
cwd = "/home/mj/src/kp"
"#,
            std::path::Path::new("x"),
        )
        .unwrap()
        .state;
        d.restore(&ctx("work"), saved, None).await.unwrap();
        comp.open("0xp", 10);
        d.handle(CompositorEvent::WindowOpened {
            address: "0xp".into(),
            workspace: "work".into(),
            class: "foot".into(),
            title: String::new(),
        })
        .await
        .unwrap();
        let state = d.state(&ctx("work")).await.unwrap();
        let ids: Vec<&str> = state.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["elsewhere", "project"], "both still in the state");
        assert_eq!(
            d.open_windows(&ctx("work")),
            ["project (foot)"],
            "the first window took `project` by its directory, not `elsewhere` by order"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn memory_limits_notify_and_shut_down() {
        use koje_core::memory::{MemoryLevel, MemoryUsage};
        use koje_core::size::ByteSize;
        let s = setup();
        let work = ctx("work");
        let file = s.paths.context_config(&work);
        let text = std::fs::read_to_string(&file).unwrap()
            + "\n[limits]\nmemory_soft = \"1G\"\nmemory_hard = \"2G\"\n";
        koje_core::context::replace_config(&s.paths, &work, &text).unwrap();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        let set = |current: u64, inactive: u64| {
            s.runtime.memory.lock().unwrap().insert(
                work.clone(),
                MemoryUsage {
                    current: ByteSize(current),
                    anon: ByteSize(0),
                    inactive_file: ByteSize(inactive),
                },
            );
        };
        // 1.5 GiB charged, but 1 GiB of it inactive cache: 0.5 GiB counts.
        set(3 << 29, 1 << 30);
        assert_eq!(d.check_memory().await, []);
        set(980 << 20, 0);
        let alerts = d.check_memory().await;
        assert_eq!(alerts.len(), 1);
        assert_eq!(
            (alerts[0].level, alerts[0].limit),
            (MemoryLevel::NearSoft, ByteSize(1 << 30))
        );
        assert_eq!(d.check_memory().await, [], "once per crossing");
        set(2 << 30, 0);
        assert_eq!(d.check_memory().await[0].level, MemoryLevel::OverHard);

        // The shutdown: windows saved (into `default`: no current state), stopped.
        assert_eq!(
            d.save_before_shutdown(&work).await.unwrap().as_str(),
            "default"
        );
        assert!(
            s.paths
                .state_file(&work, &StateName::new("default").unwrap())
                .is_file()
        );
        d.stop_container(&work).await.unwrap();
        assert!(!d.running().await.contains(&work));
    }

    /// M7.4: sleep hides every window, autosave leaves the stash's positions
    /// out, wake brings the active layout back.
    #[tokio::test(flavor = "current_thread")]
    async fn sleep_and_wake_hide_and_show() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        comp.windows.lock().unwrap()[0].workspace = "work".into();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.take();
        let work = ctx("work");
        d.hide(&work).await.unwrap();
        assert_eq!(
            comp.take(),
            [CompositorCommand::MoveToWorkspace {
                address: "0xa".into(),
                workspace: "special:stash-work".into()
            }]
        );
        {
            let mut w = comp.windows.lock().unwrap();
            w[0].workspace = "special:stash-work".into();
            w[0].placement.rect.x = 5000;
        }
        let state = d.state(&work).await.unwrap();
        let main = koje_core::name::LayoutName::initial();
        assert_eq!(
            state.layout[&main].windows[&WindowId::new("terminal").unwrap()]
                .rect
                .x,
            0,
            "the stash's position is not autosaved"
        );
        d.show(&work).await.unwrap();
        assert!(comp.take().contains(&CompositorCommand::MoveToWorkspace {
            address: "0xa".into(),
            workspace: "work".into()
        }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn focus() {
        let s = setup();
        let comp = FakeCompositor::default();
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.focus(&ctx("work")).await.unwrap();
        assert_eq!(
            comp.take(),
            [CompositorCommand::FocusWorkspace {
                workspace: "work".into()
            }]
        );
        let err = d.focus(&ctx("nope")).await.unwrap_err();
        assert_eq!(err.to_string(), "no context named nope");
        assert_eq!(comp.take(), []);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconnect_forgets_closed_windows() {
        let s = setup();
        let comp = FakeCompositor::default();
        comp.open("0xa", 10);
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
        d.handle(CompositorEvent::Connected).await.unwrap();
        comp.close("0xa");
        comp.open("0xc", 10);
        let changes = d.handle(CompositorEvent::Connected).await.unwrap();
        assert_eq!(
            changes,
            [
                Change::WindowRemoved {
                    address: "0xa".into(),
                    context: ctx("work")
                },
                Change::WindowAdded {
                    address: "0xc".into(),
                    context: ctx("work")
                }
            ]
        );
        assert!(!d.windows().contains("0xa"));
        assert_eq!(d.windows().context_of("0xc"), Some(&ctx("work")));
        // A window that closed before kojed looked: nothing to do.
        comp.take();
        d.handle(CompositorEvent::WindowOpened {
            address: "0xgone".into(),
            workspace: "1".into(),
            class: String::new(),
            title: String::new(),
        })
        .await
        .unwrap();
        assert_eq!(comp.take(), []);
    }
}
