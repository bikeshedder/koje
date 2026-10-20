//! Sound sockets that survive a sound server restart (M4.11, ADR 0029).
//!
//! A context does not get the host's `PipeWire` and `PulseAudio` sockets as
//! mounted files (a file mount keeps the old socket forever) but a mounted
//! directory, `$XDG_RUNTIME_DIR/koje/<context>/sound/`, holding hard links to
//! them. When the sound server replaces its sockets, kojed links the new ones;
//! the next connection from the context finds them.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use koje_core::name::ContextName;

/// How often kojed checks the links.
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Host socket (relative to `XDG_RUNTIME_DIR`) → link name in the directory.
/// The names match `koje_core::spec` (`PIPEWIRE_REMOTE`, `PULSE_SERVER`).
pub const SOCKETS: [(&str, &str); 2] = [
    ("pipewire-0", koje_core::spec::PIPEWIRE_LINK),
    ("pulse/native", koje_core::spec::PULSE_LINK),
];

/// `<runtime>/koje/<context>/sound`
pub fn dir(runtime: &Path, ctx: &ContextName) -> PathBuf {
    runtime.join("koje").join(ctx.as_str()).join("sound")
}

/// Every context's sound directory.
pub fn dirs(runtime: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(runtime.join("koje")) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path().join("sound"))
        .filter(|d| d.is_dir())
        .collect();
    found.sort();
    found
}

fn identity(path: &Path) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| (m.dev(), m.ino()))
}

/// Bring the links in `dir` up to date with the host sockets; the links that
/// were (re)made. A missing host socket leaves its link as it is.
pub fn sync(runtime: &Path, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut made = Vec::new();
    for (host, name) in SOCKETS {
        let host = runtime.join(host);
        let link = dir.join(name);
        let Some(want) = identity(&host) else {
            continue;
        };
        if identity(&link) == Some(want) {
            continue;
        }
        // Link beside it, then rename: the name never disappears.
        let tmp = dir.join(format!(".{name}.new"));
        let _ = std::fs::remove_file(&tmp);
        std::fs::hard_link(&host, &tmp)?;
        std::fs::rename(&tmp, &link)?;
        made.push(link);
    }
    Ok(made)
}

/// The context's sound directory with current links (`koje up`).
pub fn ensure(runtime: &Path, ctx: &ContextName) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    let dir = dir(runtime, ctx);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    sync(runtime, &dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::*;

    #[test]
    fn links_follow_a_restarted_server() {
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        std::fs::create_dir(run.join("pulse")).unwrap();
        let pw = UnixListener::bind(run.join("pipewire-0")).unwrap();
        let ctx = ContextName::new("a").unwrap();

        let dir = ensure(run, &ctx).unwrap();
        assert_eq!(dirs(run), std::slice::from_ref(&dir));
        // A hard-linked socket is the same socket: connecting through it works.
        let mut c = UnixStream::connect(dir.join("pipewire-0")).unwrap();
        c.write_all(b"x").unwrap();
        pw.accept().unwrap();
        assert!(
            !dir.join("pulse-native").exists(),
            "no PulseAudio on this host"
        );
        assert_eq!(
            sync(run, &dir).unwrap(),
            Vec::<PathBuf>::new(),
            "up to date"
        );

        // The server restarts: new sockets at the same host paths.
        drop(pw);
        std::fs::remove_file(run.join("pipewire-0")).unwrap();
        let pw = UnixListener::bind(run.join("pipewire-0")).unwrap();
        let _pulse = UnixListener::bind(run.join("pulse/native")).unwrap();
        assert_eq!(
            sync(run, &dir).unwrap(),
            [dir.join("pipewire-0"), dir.join("pulse-native")]
        );
        UnixStream::connect(dir.join("pipewire-0")).unwrap();
        pw.accept().unwrap();
        UnixStream::connect(dir.join("pulse-native")).unwrap();
    }
}
