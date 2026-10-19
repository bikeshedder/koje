//! kojed's window tracking (M4.4) and border colors (M4.5, ADR 0018): reacts to
//! compositor events, asks the container runtime which context a window's
//! process belongs to and sends tag rules and tags. Decisions live in
//! `koje_core::windows`; this module does the I/O around them.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use koje_core::color::Color;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_core::ports::{Compositor, CompositorCommand, CompositorEvent, ContainerRuntime};
use koje_core::windows::{self, WindowMap};

#[derive(Debug)]
pub struct Daemon<C, R> {
    compositor: C,
    runtime: R,
    paths: Paths,
    colors: BTreeMap<ContextName, Color>,
    windows: WindowMap,
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
        }
    }

    #[cfg(test)]
    pub fn windows(&self) -> &WindowMap {
        &self.windows
    }

    pub async fn handle(&mut self, event: CompositorEvent) -> Result<()> {
        match event {
            CompositorEvent::Connected => self.sync().await,
            CompositorEvent::ConfigReloaded => {
                // Runtime rules are gone after a reload (ADR 0018); tags are
                // set again too, which is idempotent.
                let mut commands = self.rules();
                commands.extend(self.windows.tags());
                self.apply(&commands).await
            }
            CompositorEvent::WindowOpened { address, .. } => self.opened(&address).await,
            CompositorEvent::WindowClosed { address } => {
                self.windows.remove(&address);
                Ok(())
            }
            CompositorEvent::WindowFocused { .. } | CompositorEvent::WindowTitle { .. } => Ok(()),
        }
    }

    /// Start and every (re)connection: contexts, rules, all windows.
    async fn sync(&mut self) -> Result<()> {
        self.load_colors();
        let present = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let new = self
            .windows
            .retain(present.iter().map(|w| w.address.as_str()));
        let mut commands = Vec::new();
        for w in present.iter().filter(|w| new.contains(&w.address)) {
            let ctx = self.resolve(w.pid).await;
            self.windows.insert(&w.address, w.pid, ctx);
        }
        // Contexts created since the colors were loaded.
        if self
            .windows
            .contexts()
            .iter()
            .any(|c| !self.colors.contains_key(*c))
        {
            self.load_colors();
        }
        commands.extend(self.rules());
        commands.extend(self.windows.tags());
        tracing::info!(
            windows = present.len(),
            contexts = ?self.windows.counts(),
            "synced with the compositor"
        );
        self.apply(&commands).await
    }

    async fn opened(&mut self, address: &str) -> Result<()> {
        if self.windows.contains(address) {
            return Ok(());
        }
        // Events carry no pid; the window list has it.
        let all = self
            .compositor
            .windows()
            .await
            .map_err(anyhow::Error::new)?;
        let Some(w) = all.into_iter().find(|w| w.address == address) else {
            return Ok(()); // closed again already
        };
        let ctx = self.resolve(w.pid).await;
        let mut commands = Vec::new();
        if let Some(c) = &ctx
            && !self.colors.contains_key(c)
        {
            // A context created after kojed started.
            self.load_colors();
            commands.extend(self.colors.get(c).map(|color| windows::rule(c, *color)));
        }
        tracing::debug!(address, pid = w.pid, context = ?ctx.as_ref().map(ContextName::as_str), "window opened");
        commands.extend(self.windows.insert(address, w.pid, ctx));
        self.apply(&commands).await
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
    fn load_colors(&mut self) {
        match koje_core::context::list(&self.paths) {
            Ok(list) => self.colors = list.into_iter().map(|c| (c.name, c.color)).collect(),
            Err(e) => tracing::warn!("reading the contexts: {e}"),
        }
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

    /// pid → context; counts lookups.
    #[derive(Debug, Clone, Default)]
    struct FakeRuntime {
        pids: Arc<Mutex<HashMap<u32, ContextName>>>,
        lookups: Arc<Mutex<u32>>,
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
        _tmp: tempfile::TempDir,
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
            _tmp: tmp,
            paths,
            runtime,
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

        d.handle(CompositorEvent::Connected).await.unwrap();
        assert_eq!(
            comp.take(),
            [windows::rule(&ctx("work"), red()), tag("0xa", "koje-work")]
        );

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
        assert_eq!(comp.take(), [tag("0xb", "koje-work")]);
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

        d.handle(CompositorEvent::WindowClosed {
            address: "0xb".into(),
        })
        .await
        .unwrap();
        assert!(!d.windows().contains("0xb"));
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
            [windows::rule(&ctx("new"), blue), tag("0xn", "koje-new")]
        );
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
        d.handle(CompositorEvent::Connected).await.unwrap();
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
