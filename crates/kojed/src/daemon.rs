//! kojed's window tracking (M4.4) and border colors (M4.5, ADR 0018): reacts to
//! compositor events, asks the container runtime which context a window's
//! process belongs to and sends tag rules and tags. Decisions live in
//! `koje_core::windows`; this module does the I/O around them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use koje_core::capture::{Live, Observed};
use koje_core::color::Color;
use koje_core::integration::Integration;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::WindowInfo;
use koje_core::ports::{Compositor, CompositorCommand, CompositorEvent, ContainerRuntime};
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
}

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
        let added = self.live.entry(ctx.clone()).or_default().add(
            &w.address,
            &w.app_id,
            &self.integrations,
            w.placement.clone(),
        );
        let Some(added) = added else {
            tracing::warn!(address = %w.address, app_id = %w.app_id, "no window id");
            return None;
        };
        tracing::debug!(context = %ctx, id = %added.id, restored = added.place.is_some(), "window tracked");
        added.place.map(|placement| CompositorCommand::Place {
            address: w.address.clone(),
            placement,
        })
    }

    /// Start the windows of `state` in the freshly started `ctx` (`koje up`,
    /// M5.3). Windows appearing for them take their ids and places.
    pub async fn restore(&mut self, ctx: &ContextName, state: State) -> Result<restore::Plan> {
        self.require(ctx)?;
        if self.live.get(ctx).is_some_and(|l| !l.windows.is_empty()) {
            anyhow::bail!(
                "{ctx} already has windows – switching states in a running context comes with M5.9"
            );
        }
        let live = Live::restoring(state);
        let plan = restore::plan(&live.pending, &self.integrations, &self.home);
        self.live.insert(ctx.clone(), live);
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
        Ok(plan)
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

    /// The context's windows that are still open: `id (app id)`.
    pub fn open_windows(&self, ctx: &ContextName) -> Vec<String> {
        self.live
            .get(ctx)
            .map(|l| {
                l.windows
                    .values()
                    .map(|w| format!("{} ({})", w.id, w.app_id))
                    .collect()
            })
            .unwrap_or_default()
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
        let proc_root = self.proc_root.clone();
        let integrations = &self.integrations;
        let live = self.live.entry(ctx.clone()).or_default();
        Ok(live.snapshot(integrations, |address| {
            let w = by_address.get(address)?;
            let observed = Observed {
                app_id: w.app_id.clone(),
                title: w.title.clone(),
                process: crate::procfs::observe(&proc_root, w.pid),
            };
            Some((observed, w.placement.clone()))
        }))
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
    pub fn active_context(&self) -> Option<&ContextName> {
        self.focused
            .as_deref()
            .and_then(|a| self.windows.context_of(a))
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
            CompositorEvent::WindowTitle { .. } => Ok(Vec::new()),
        }
    }

    /// Start and every (re)connection: contexts, rules, all windows.
    async fn sync(&mut self) -> Result<Vec<Change>> {
        let mut recolor = self.load_colors();
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
    use koje_core::ports::{ContainerStatus, WindowInfo};
    use koje_core::size::ByteSize;
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
        fn memory_usage(&self, _: &ContextName) -> std::io::Result<ByteSize> {
            Ok(ByteSize(0))
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
        let plan = d.restore(&ctx("work"), saved).await.unwrap();
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
                State::new(koje_core::name::LayoutName::initial())
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
        let mut d = Daemon::new(&comp, s.runtime.clone(), s.paths.clone());
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
        assert_eq!(d.open_windows(&ctx("work")), Vec::<String>::new());
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
