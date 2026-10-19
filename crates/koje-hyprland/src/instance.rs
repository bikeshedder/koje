//! Finding the Hyprland instance: `$HYPRLAND_INSTANCE_SIGNATURE`, else the
//! newest one in `$XDG_RUNTIME_DIR/hypr/` (a nested Hyprland or a service
//! started before the session exported the variable, M2.2).

use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub const COMMAND_SOCKET: &str = ".socket.sock";
pub const EVENT_SOCKET: &str = ".socket2.sock";

/// The instance directory from the process environment.
pub fn from_env() -> Result<PathBuf> {
    let run = std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::NoRuntimeDir)?;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    discover(&Path::new(&run).join("hypr"), signature.as_deref())
}

/// The instance directory in `base` (`$XDG_RUNTIME_DIR/hypr`): the one named by
/// `signature` if it has a command socket, else the newest one that has.
pub fn discover(base: &Path, signature: Option<&str>) -> Result<PathBuf> {
    if let Some(sig) = signature.filter(|s| !s.is_empty() && !s.contains('/')) {
        let dir = base.join(sig);
        if dir.join(COMMAND_SOCKET).exists() {
            return Ok(dir);
        }
        tracing::debug!(signature = sig, "instance gone, looking for the newest one");
    }
    std::fs::read_dir(base)
        .map_err(|_| Error::NoInstance(base.to_owned()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().join(COMMAND_SOCKET).exists())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
        .ok_or_else(|| Error::NoInstance(base.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn instance(base: &Path, name: &str, age: u64) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(COMMAND_SOCKET), "").unwrap();
        let t = SystemTime::now() - Duration::from_secs(age);
        std::fs::File::open(&dir).unwrap().set_modified(t).unwrap();
        dir
    }

    #[test]
    fn signature_then_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let old = instance(tmp.path(), "old", 100);
        let new = instance(tmp.path(), "new", 10);
        std::fs::create_dir(tmp.path().join("no-socket")).unwrap();
        assert_eq!(discover(tmp.path(), Some("old")).unwrap(), old);
        assert_eq!(discover(tmp.path(), None).unwrap(), new);
        // A stale signature (Hyprland restarted) falls back to the newest.
        assert_eq!(discover(tmp.path(), Some("gone")).unwrap(), new);
        assert_eq!(discover(tmp.path(), Some("../x")).unwrap(), new);
    }

    #[test]
    fn nothing_running() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            discover(&tmp.path().join("hypr"), None),
            Err(Error::NoInstance(_))
        ));
        assert!(matches!(
            discover(tmp.path(), Some("x")),
            Err(Error::NoInstance(_))
        ));
    }
}
