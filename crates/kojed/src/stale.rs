//! Stale sound sockets (M4.11, from M3.11, ADR 0015): a context gets the host's
//! `PipeWire` and `PulseAudio` sockets as bind-mounted files, so when the sound
//! server restarts and puts new sockets in place, running contexts keep the
//! old ones and lose sound. kojed notices the replacement and tells the user
//! which contexts to restart.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use koje_core::name::ContextName;

/// How often kojed looks at the sockets.
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// A file's identity: device and inode.
pub type Identity = (u64, u64);

/// The sound sockets in the host's `XDG_RUNTIME_DIR`, as the spec mounts them.
pub fn sound_sockets(runtime: &Path) -> Vec<PathBuf> {
    vec![runtime.join("pipewire-0"), runtime.join("pulse/native")]
}

pub fn identity(path: &Path) -> Option<Identity> {
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// The last seen identity per socket.
#[derive(Debug, Default)]
pub struct Watch {
    seen: BTreeMap<PathBuf, Identity>,
}

impl Watch {
    /// Record the current identities; the sockets that were replaced since
    /// the last call (a socket that is missing for a while counts once it is
    /// back with another identity).
    pub fn update(
        &mut self,
        now: impl IntoIterator<Item = (PathBuf, Option<Identity>)>,
    ) -> Vec<PathBuf> {
        let mut replaced = Vec::new();
        for (path, id) in now {
            let Some(id) = id else { continue };
            if let Some(old) = self.seen.insert(path.clone(), id)
                && old != id
            {
                replaced.push(path);
            }
        }
        replaced
    }
}

/// The notification text, `None` when no running context is affected.
pub fn message(affected: &[ContextName]) -> Option<(String, String)> {
    if affected.is_empty() {
        return None;
    }
    let names: Vec<&str> = affected.iter().map(ContextName::as_str).collect();
    // One line: notification daemons often show only the first (host test).
    Some((
        format!("Sound restarted: {} lost sound", names.join(", ")),
        "Restart to get it back: koje down <context> --no-save, then koje up <context> -s empty"
            .to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_replaced_sockets() {
        let pw = PathBuf::from("/run/user/1000/pipewire-0");
        let pulse = PathBuf::from("/run/user/1000/pulse/native");
        let mut w = Watch::default();
        assert_eq!(
            w.update([(pw.clone(), Some((1, 10))), (pulse.clone(), None)]),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            w.update([(pw.clone(), Some((1, 10))), (pulse.clone(), Some((1, 20)))]),
            Vec::<PathBuf>::new(),
            "first sight is no replacement"
        );
        // PipeWire restarts: gone for a moment, then a new socket.
        assert_eq!(
            w.update([(pw.clone(), None), (pulse.clone(), Some((1, 20)))]),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            w.update([(pw.clone(), Some((1, 11))), (pulse.clone(), Some((1, 21)))]),
            [pw.clone(), pulse]
        );
        assert_eq!(w.update([(pw, Some((1, 11)))]), Vec::<PathBuf>::new());
    }

    #[test]
    fn real_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("pipewire-0");
        std::fs::write(&p, "").unwrap();
        let a = identity(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        std::fs::write(tmp.path().join("other"), "").unwrap(); // keeps the inode taken
        std::fs::write(&p, "").unwrap();
        assert_ne!(identity(&p), Some(a));
        assert_eq!(identity(&tmp.path().join("missing")), None);
    }

    #[test]
    fn messages() {
        assert_eq!(message(&[]), None);
        let (summary, body) = message(&[
            ContextName::new("a").unwrap(),
            ContextName::new("b").unwrap(),
        ])
        .unwrap();
        assert_eq!(summary, "Sound restarted: a, b lost sound");
        assert!(!body.contains('\n'), "one line: {body}");
        assert!(body.contains("koje down <context> --no-save"));
    }
}
