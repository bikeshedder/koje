//! The contexts' Wayland sockets (M4.10).
//!
//! GNOME's Mutter has no security contexts (ADR 0059, L1; ADR 0022 is
//! superseded by 0061): a context's socket is a hard link to Mutter's own, in
//! `$XDG_RUNTIME_DIR/koje/<context>/wayland/wayland-0`. The directory is
//! mounted into the context, so a link made again is visible there at once.

use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};
use koje_core::name::ContextName;

/// The socket's file name in the context's directory.
pub const SOCKET_NAME: &str = "wayland-0";

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

/// Hard links to the compositor's socket `display`.
#[derive(Debug, Clone)]
pub struct Sockets {
    runtime: PathBuf,
    display: PathBuf,
}

impl Sockets {
    /// Contexts get hard links to the compositor's socket `display`.
    pub fn shared(runtime: PathBuf, display: PathBuf) -> Self {
        Self { runtime, display }
    }

    /// The socket of `ctx`, linked if needed.
    pub fn ensure(&self, ctx: &ContextName) -> Result<PathBuf> {
        link(&self.runtime, &self.display, ctx)
    }

    /// Remove the socket of `ctx` (`koje down`).
    pub fn release(&self, ctx: &ContextName) {
        let _ = std::fs::remove_file(dir(&self.runtime, ctx).join(SOCKET_NAME));
    }

    /// The compositor made its socket again: link every context that had one.
    pub fn reset(&self) {
        for ctx in with_sockets(&self.runtime) {
            if let Err(e) = link(&self.runtime, &self.display, &ctx) {
                tracing::warn!(context = %ctx, "{e:#}");
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

#[cfg(test)]
mod tests {
    use std::os::unix::net::{UnixListener, UnixStream};

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

    #[test]
    fn shared_sockets_are_links_to_the_compositors() {
        use std::os::unix::fs::MetadataExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        let display = run.join("wayland-0");
        let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
        let listener = UnixListener::bind(&display).unwrap();
        let sockets = Sockets::shared(run.to_owned(), display.clone());
        let a = ContextName::new("a").unwrap();

        let socket = sockets.ensure(&a).unwrap();
        assert_eq!(socket, dir(run, &a).join(SOCKET_NAME));
        assert_eq!(ino(&socket), ino(&display));
        UnixStream::connect(&socket).unwrap();

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
        assert!(none.ensure(&a).is_err());
    }
}
