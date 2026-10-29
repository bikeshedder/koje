//! Security-context Wayland sockets, one per context (M4.10, ADR 0022).
//!
//! A worker thread owns the Wayland connection to the compositor (wayland-client
//! is blocking) and, per context, a listening socket registered through
//! `wp_security_context_v1`. The compositor serves a socket as long as kojed
//! holds the write end of its close pipe, so the sockets die with kojed and are
//! recreated when it starts again ([`Sockets::reset`] on `Connected`).
//!
//! Layout: `$XDG_RUNTIME_DIR/koje/<context>/wayland/wayland-0`. The directory is
//! mounted into the context, so a recreated socket is visible there at once.
//!
//! GNOME's Mutter has no security contexts (ADR 0059, L1): there the socket
//! in that directory is a hard link to Mutter's own ([`Sockets::shared`]).

use std::collections::BTreeMap;
use std::io::PipeWriter;
use std::os::fd::AsFd as _;
use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{Context as _, Result, anyhow};
use koje_core::name::ContextName;
use tokio::sync::oneshot;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1::WpSecurityContextManagerV1,
    wp_security_context_v1::WpSecurityContextV1,
};

/// The socket's file name in the context's directory.
pub const SOCKET_NAME: &str = "wayland-0";
/// Sandbox engine name the compositor sees (ADR 0022).
const ENGINE: &str = "io.koje";

/// `<runtime>/koje/<context>/wayland`
pub fn dir(runtime: &Path, ctx: &ContextName) -> PathBuf {
    runtime.join("koje").join(ctx.as_str()).join("wayland")
}

/// Contexts that had a socket (a socket file in their directory): after a
/// kojed or compositor restart they get a new one.
pub fn with_sockets(runtime: &Path) -> Vec<ContextName> {
    let Ok(entries) = std::fs::read_dir(runtime.join("koje")) else {
        return Vec::new();
    };
    let mut found: Vec<ContextName> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|e| ContextName::new(e.file_name().to_string_lossy()).ok())
        .filter(|ctx| {
            std::fs::symlink_metadata(dir(runtime, ctx).join(SOCKET_NAME))
                .is_ok_and(|m| m.file_type().is_socket())
        })
        .collect();
    found.sort();
    found
}

type Reply = oneshot::Sender<Result<PathBuf, String>>;

enum Msg {
    Ensure(ContextName, Reply),
    Release(ContextName),
    Reset,
    Probe(oneshot::Sender<Result<(), String>>),
}

/// Handle to the worker thread, or to the links to a shared socket.
#[derive(Debug, Clone)]
pub struct Sockets {
    inner: Inner,
}

#[derive(Debug, Clone)]
enum Inner {
    Security(mpsc::Sender<Msg>),
    /// Hard links to the compositor's socket `display` (GNOME).
    Shared {
        runtime: PathBuf,
        display: PathBuf,
    },
}

/// Why contexts share GNOME's socket (`koje doctor`).
const SHARED: &str =
    "GNOME's Mutter has no security contexts: contexts share its Wayland socket (ADR 0059, L1)";

impl Sockets {
    /// Start the worker. `display` finds the compositor's Wayland socket (it
    /// changes when the compositor restarts).
    pub fn start(
        runtime: PathBuf,
        display: impl Fn() -> Result<PathBuf> + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("wayland".into())
            .spawn(move || {
                Worker {
                    runtime,
                    display: Box::new(display),
                    wl: None,
                    contexts: BTreeMap::new(),
                }
                .run(&rx);
            })
            .context("starting the Wayland thread")?;
        Ok(Self {
            inner: Inner::Security(tx),
        })
    }

    /// Contexts get hard links to the compositor's socket `display`.
    pub fn shared(runtime: PathBuf, display: PathBuf) -> Self {
        Self {
            inner: Inner::Shared { runtime, display },
        }
    }

    fn tx(&self) -> Option<&mpsc::Sender<Msg>> {
        match &self.inner {
            Inner::Security(tx) => Some(tx),
            Inner::Shared { .. } => None,
        }
    }

    /// The socket of `ctx`, created if needed.
    pub async fn ensure(&self, ctx: &ContextName) -> Result<PathBuf> {
        let tx = match &self.inner {
            Inner::Security(tx) => tx,
            Inner::Shared { runtime, display } => return link(runtime, display, ctx),
        };
        let (reply, rx) = oneshot::channel();
        tx.send(Msg::Ensure(ctx.clone(), reply))
            .map_err(|_| anyhow!("the Wayland thread is gone"))?;
        rx.await
            .map_err(|_| anyhow!("the Wayland thread is gone"))?
            .map_err(|e| anyhow!(e))
    }

    /// Whether kojed reaches the compositor and it offers security contexts
    /// (`koje doctor`).
    pub async fn probe(&self) -> Result<()> {
        let Some(tx) = self.tx() else {
            return Err(anyhow!(SHARED));
        };
        let (reply, rx) = oneshot::channel();
        tx.send(Msg::Probe(reply))
            .map_err(|_| anyhow!("the Wayland thread is gone"))?;
        rx.await
            .map_err(|_| anyhow!("the Wayland thread is gone"))?
            .map_err(|e| anyhow!(e))
    }

    /// Stop serving the socket of `ctx` (`koje down`).
    pub fn release(&self, ctx: &ContextName) {
        match &self.inner {
            Inner::Security(tx) => {
                let _ = tx.send(Msg::Release(ctx.clone()));
            }
            Inner::Shared { runtime, .. } => {
                let _ = std::fs::remove_file(dir(runtime, ctx).join(SOCKET_NAME));
            }
        }
    }

    /// The compositor (re)connected: new connection, new sockets for every
    /// context that had one.
    pub fn reset(&self) {
        match &self.inner {
            Inner::Security(tx) => {
                let _ = tx.send(Msg::Reset);
            }
            Inner::Shared { runtime, display } => {
                for ctx in with_sockets(runtime) {
                    if let Err(e) = link(runtime, display, &ctx) {
                        tracing::warn!(context = %ctx, "{e:#}");
                    }
                }
            }
        }
    }
}

/// The compositor's socket: `WAYLAND_DISPLAY`, relative to `runtime` unless
/// absolute (`wayland-0` without it).
pub fn display_from_env(runtime: &Path) -> PathBuf {
    runtime.join(std::env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| SOCKET_NAME.into()))
}

/// `ctx`'s socket as a hard link to `display`, made again when `display` was
/// replaced (as the sound links, ADR 0029).
fn link(runtime: &Path, display: &Path, ctx: &ContextName) -> Result<PathBuf> {
    use std::os::unix::fs::MetadataExt as _;
    let identity = |p: &Path| {
        std::fs::symlink_metadata(p)
            .ok()
            .map(|m| (m.dev(), m.ino()))
    };
    let dir = dir(runtime, ctx);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let socket = dir.join(SOCKET_NAME);
    let want =
        identity(display).ok_or_else(|| anyhow!("no Wayland socket {}", display.display()))?;
    if identity(&socket) != Some(want) {
        let _ = std::fs::remove_file(&socket);
        std::fs::hard_link(display, &socket)
            .with_context(|| format!("linking {} to {}", socket.display(), display.display()))?;
    }
    Ok(socket)
}

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextManagerV1,
        _: <WpSecurityContextManagerV1 as Proxy>::Event,
        &(): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextV1,
        _: <WpSecurityContextV1 as Proxy>::Event,
        &(): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

struct Wl {
    queue: EventQueue<State>,
    manager: WpSecurityContextManagerV1,
}

/// One served socket. Dropping it closes the pipe: the compositor stops
/// accepting on the socket.
struct Served {
    _listener: UnixListener,
    _close: PipeWriter,
    _security_context: WpSecurityContextV1,
}

struct Worker {
    runtime: PathBuf,
    display: Box<dyn Fn() -> Result<PathBuf> + Send>,
    wl: Option<Wl>,
    contexts: BTreeMap<ContextName, Served>,
}

impl Worker {
    fn run(mut self, rx: &mpsc::Receiver<Msg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Ensure(ctx, reply) => {
                    let result = self.ensure(&ctx).map_err(|e| format!("{e:#}"));
                    let _ = reply.send(result);
                }
                Msg::Release(ctx) => {
                    self.contexts.remove(&ctx);
                    let _ = std::fs::remove_file(dir(&self.runtime, &ctx).join(SOCKET_NAME));
                    tracing::info!(context = %ctx, "Wayland socket released");
                }
                Msg::Reset => self.reset(),
                Msg::Probe(reply) => {
                    // A stale connection fails the roundtrip: connect anew.
                    if self.roundtrip().is_err() {
                        self.wl = None;
                    }
                    let result = self.connect().map(drop).map_err(|e| format!("{e:#}"));
                    let _ = reply.send(result);
                }
            }
        }
    }

    fn reset(&mut self) {
        self.contexts.clear();
        self.wl = None;
        for ctx in with_sockets(&self.runtime) {
            if let Err(e) = self.ensure(&ctx) {
                tracing::warn!(context = %ctx, "recreating the Wayland socket: {e:#}");
            }
        }
    }

    fn ensure(&mut self, ctx: &ContextName) -> Result<PathBuf> {
        let dir = dir(&self.runtime, ctx);
        let socket = dir.join(SOCKET_NAME);
        // Still served if the connection answers (a dead compositor fails here).
        if self.contexts.contains_key(ctx) && socket.exists() && self.roundtrip().is_ok() {
            return Ok(socket);
        }
        self.contexts.remove(ctx);
        let served = self.create(ctx, &dir, &socket).or_else(|e| {
            // Maybe the compositor restarted: one more try on a new connection.
            tracing::debug!("{e:#}; reconnecting");
            self.wl = None;
            self.create(ctx, &dir, &socket)
        })?;
        self.contexts.insert(ctx.clone(), served);
        tracing::info!(context = %ctx, socket = %socket.display(), "Wayland socket ready");
        Ok(socket)
    }

    fn roundtrip(&mut self) -> Result<()> {
        let wl = self.wl.as_mut().context("not connected")?;
        wl.queue.roundtrip(&mut State)?;
        Ok(())
    }

    fn connect(&mut self) -> Result<&mut Wl> {
        if self.wl.is_none() {
            let path = (self.display)()?;
            let stream = UnixStream::connect(&path)
                .with_context(|| format!("connecting to the compositor at {}", path.display()))?;
            let conn = Connection::from_socket(stream)?;
            let (globals, queue) = registry_queue_init::<State>(&conn)?;
            let manager = globals.bind(&queue.handle(), 1..=1, ()).map_err(|e| {
                anyhow!("the compositor does not offer wp_security_context_manager_v1: {e}")
            })?;
            self.wl = Some(Wl { queue, manager });
        }
        self.wl.as_mut().context("not connected")
    }

    fn create(&mut self, ctx: &ContextName, dir: &Path, socket: &Path) -> Result<Served> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let _ = std::fs::remove_file(socket);
        let listener =
            UnixListener::bind(socket).with_context(|| format!("binding {}", socket.display()))?;
        let (close_read, close_write) = std::io::pipe()?;
        let wl = self.connect()?;
        let qh = wl.queue.handle();
        let sc = wl
            .manager
            .create_listener(listener.as_fd(), close_read.as_fd(), &qh, ());
        sc.set_sandbox_engine(ENGINE.into());
        sc.set_app_id(format!("{ENGINE}.{ctx}"));
        sc.set_instance_id(ctx.to_string());
        sc.commit();
        wl.queue.roundtrip(&mut State)?;
        Ok(Served {
            _listener: listener,
            _close: close_write,
            _security_context: sc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_contexts_with_sockets() {
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        let a = ContextName::new("a").unwrap();
        let b = ContextName::new("b").unwrap();
        for ctx in [&a, &b] {
            std::fs::create_dir_all(dir(run, ctx)).unwrap();
        }
        let _socket = UnixListener::bind(dir(run, &a).join(SOCKET_NAME)).unwrap();
        // b: a released context (directory without socket); junk is skipped.
        std::fs::write(run.join("koje/Not A Context"), "").unwrap();
        assert_eq!(with_sockets(run), [a]);
        assert_eq!(with_sockets(&run.join("nothing")), []);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shared_sockets_are_links_to_the_compositors() {
        use std::os::unix::fs::MetadataExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        let display = run.join("wayland-0");
        let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
        let listener = UnixListener::bind(&display).unwrap();
        let sockets = Sockets::shared(run.to_owned(), display.clone());
        let a = ContextName::new("a").unwrap();

        let socket = sockets.ensure(&a).await.unwrap();
        assert_eq!(socket, dir(run, &a).join(SOCKET_NAME));
        assert_eq!(ino(&socket), ino(&display));
        UnixStream::connect(&socket).unwrap();
        assert!(
            sockets
                .probe()
                .await
                .unwrap_err()
                .to_string()
                .contains("ADR 0059")
        );

        // The compositor made its socket again: `reset` links the new one.
        drop(listener);
        std::fs::remove_file(&display).unwrap();
        let _listener = UnixListener::bind(&display).unwrap();
        assert_ne!(ino(&socket), ino(&display));
        sockets.reset();
        assert_eq!(ino(&socket), ino(&display));

        sockets.release(&a);
        assert!(!socket.exists());
        assert!(display.exists());
        // No compositor socket: an error, no link.
        let none = Sockets::shared(run.to_owned(), run.join("wayland-9"));
        assert!(none.ensure(&a).await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_compositor_is_an_error_not_a_hang() {
        let tmp = tempfile::tempdir().unwrap();
        let sockets =
            Sockets::start(tmp.path().to_owned(), || Err(anyhow!("no compositor"))).unwrap();
        let err = sockets
            .ensure(&ContextName::new("a").unwrap())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no compositor"), "{err}");
        assert!(sockets.probe().await.is_err());
    }

    /// M4.8: a security-context socket from the running compositor accepts a
    /// Wayland client, and dies with the worker's close pipe.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "needs a running Hyprland with security-context-v1"]
    async fn live_socket_serves_clients() {
        let tmp = tempfile::tempdir().unwrap();
        let sockets = Sockets::start(tmp.path().to_owned(), || {
            Ok(koje_hyprland::instance::wayland_from_env()?)
        })
        .unwrap();
        let ctx = ContextName::new("koje-test").unwrap();
        let socket = sockets.ensure(&ctx).await.unwrap();
        assert_eq!(socket, dir(tmp.path(), &ctx).join(SOCKET_NAME));
        // A client on the socket: the registry answers.
        let conn = Connection::from_socket(UnixStream::connect(&socket).unwrap()).unwrap();
        let (globals, _queue) = registry_queue_init::<State>(&conn).unwrap();
        assert!(
            globals
                .contents()
                .with_list(|l| l.iter().any(|g| g.interface == "wl_compositor")),
            "the compositor's globals are offered"
        );
        // Sandboxed clients do not get the security-context manager themselves.
        assert!(
            !globals.contents().with_list(|l| l
                .iter()
                .any(|g| g.interface == "wp_security_context_manager_v1")),
            "a sandboxed client must not create security contexts"
        );
        sockets.release(&ctx);
    }
}
