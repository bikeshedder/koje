//! Host folders bound into a context (`mounts` in `context.toml`).
//!
//! Folders are mounted at the same path as on the host, so a path must be
//! absolute after expansion. Only a leading `~`, `$HOME` or `${HOME}` is
//! expanded; everything else that looks like shell syntax is rejected instead of
//! silently mounting a folder with a literal `$` or `~` in its name.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// One validated `mounts` entry. Keeps the text as written for round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MountPath {
    raw: String,
    base: Base,
    /// Normalized path below `base`, without `.` components or trailing `/`.
    rel: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    Home,
    Root,
}

impl MountPath {
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        let invalid = |reason| Error::InvalidMount {
            value: raw.clone(),
            reason,
        };
        let (base, rest) = if let Some(rest) = strip_home(&raw) {
            (Base::Home, rest)
        } else if raw.starts_with('~') {
            return Err(invalid(
                "only `~` for the own home is supported, not `~user`",
            ));
        } else if raw.starts_with('/') {
            (Base::Root, raw.as_str())
        } else if raw.is_empty() {
            return Err(invalid("must not be empty"));
        } else {
            return Err(invalid(
                "must be absolute or start with `~` or `$HOME` (relative paths are rejected)",
            ));
        };
        if rest.contains('$') {
            return Err(invalid("only a leading `$HOME` is expanded"));
        }
        let mut rel = PathBuf::new();
        for component in Path::new(rest).components() {
            match component {
                Component::Normal(part) => rel.push(part),
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => return Err(invalid("must not contain `..`")),
                Component::Prefix(_) => return Err(invalid("unsupported path prefix")),
            }
        }
        Ok(Self { raw, base, rel })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The absolute host path, with `~`/`$HOME` replaced by `home`.
    pub fn expand(&self, home: &Path) -> PathBuf {
        match self.base {
            Base::Home => home.join(&self.rel),
            Base::Root => Path::new("/").join(&self.rel),
        }
    }
}

/// The part after a leading `~`, `$HOME` or `${HOME}`, if the path starts with one.
fn strip_home(raw: &str) -> Option<&str> {
    ["~", "${HOME}", "$HOME"].iter().find_map(|prefix| {
        raw.strip_prefix(prefix)
            .filter(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// One `mounts` entry: `"~/src"` or `{ path = "~/.config/nvim", unsafe = true }`.
///
/// `unsafe = true` lets the entry bypass the refused targets of ADR 0009.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: MountPath,
    pub allow_unsafe: bool,
}

impl From<MountPath> for Mount {
    fn from(path: MountPath) -> Self {
        Self {
            path,
            allow_unsafe: false,
        }
    }
}

/// Table form of [`Mount`] as written in `context.toml`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MountTable {
    path: MountPath,
    #[serde(default, rename = "unsafe")]
    allow_unsafe: bool,
}

impl Serialize for Mount {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.allow_unsafe {
            MountTable {
                path: self.path.clone(),
                allow_unsafe: true,
            }
            .serialize(serializer)
        } else {
            self.path.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Mount {
    // Hand-written instead of `#[serde(untagged)]` so the reason of a bad path
    // reaches the user instead of "did not match any variant".
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Mount;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a path or a table { path = \"…\", unsafe = true }")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Mount, E> {
                MountPath::new(v).map(Mount::from).map_err(E::custom)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Mount, A::Error> {
                let table =
                    MountTable::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(Mount {
                    path: table.path,
                    allow_unsafe: table.allow_unsafe,
                })
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Host folders the mount rules of ADR 0009 are checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDirs {
    /// The user's real home; `~` expands to it.
    pub home: PathBuf,
    /// Folders a mount may neither equal, contain nor lie inside: koje's own
    /// config/data/state dirs, the host's container storage, `$XDG_RUNTIME_DIR`.
    pub protected: Vec<PathBuf>,
}

/// Folders the image provides; a mount may neither equal nor lie inside them.
const SYSTEM_DIRS: &[&str] = &[
    "/usr", "/etc", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/proc", "/sys",
    "/dev", "/run", "/var", "/boot",
];

/// Why `path` is a refused mount target, if it is one.
fn refusal(path: &Path, host: &HostDirs) -> Option<String> {
    if host.home.starts_with(path) {
        return Some(format!(
            "would hide the context's home {}",
            host.home.display()
        ));
    }
    if let Some(dir) = host
        .protected
        .iter()
        .find(|dir| dir.starts_with(path) || path.starts_with(dir))
    {
        return Some(format!("overlaps the protected folder {}", dir.display()));
    }
    // Inside home is never a system folder, even when home lives in `/var/home`.
    if !path.starts_with(&host.home)
        && let Some(dir) = SYSTEM_DIRS.iter().find(|dir| path.starts_with(dir))
    {
        return Some(format!("would replace the system folder {dir}"));
    }
    None
}

/// Expand all mounts and reject refused targets (ADR 0009) and entries that
/// name the same folder.
pub fn expand_all(mounts: &[Mount], host: &HostDirs) -> Result<Vec<PathBuf>> {
    let mut seen: Vec<(PathBuf, &MountPath)> = Vec::with_capacity(mounts.len());
    for mount in mounts {
        let path = mount.path.expand(&host.home);
        if !mount.allow_unsafe
            && let Some(reason) = refusal(&path, host)
        {
            return Err(Error::RefusedMount {
                value: mount.path.raw.clone(),
                reason,
            });
        }
        if let Some((_, first)) = seen.iter().find(|(p, _)| *p == path) {
            return Err(Error::DuplicateMount {
                path,
                first: first.raw.clone(),
                second: mount.path.raw.clone(),
            });
        }
        seen.push((path, &mount.path));
    }
    Ok(seen.into_iter().map(|(p, _)| p).collect())
}

impl FromStr for MountPath {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for MountPath {
    type Error = Error;
    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

impl From<MountPath> for String {
    fn from(m: MountPath) -> String {
        m.raw
    }
}

impl fmt::Display for MountPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/mj";

    fn expand(raw: &str) -> PathBuf {
        MountPath::new(raw).unwrap().expand(Path::new(HOME))
    }

    fn reason(raw: &str) -> &'static str {
        match MountPath::new(raw) {
            Err(Error::InvalidMount { reason, .. }) => reason,
            other => panic!("expected InvalidMount for {raw:?}, got {other:?}"),
        }
    }

    #[test]
    fn expands_home_forms() {
        for raw in ["~/src/kp", "$HOME/src/kp", "${HOME}/src/kp", "~//src/./kp/"] {
            assert_eq!(expand(raw), PathBuf::from("/home/mj/src/kp"), "{raw}");
        }
        assert_eq!(expand("~"), PathBuf::from(HOME));
        assert_eq!(expand("$HOME"), PathBuf::from(HOME));
    }

    #[test]
    fn keeps_absolute_paths() {
        assert_eq!(expand("/srv/data/"), PathBuf::from("/srv/data"));
    }

    #[test]
    fn rejects_relative_paths() {
        for raw in ["src/kp", "./src", "../src", "Austausch"] {
            assert!(reason(raw).contains("relative"), "{raw}");
        }
        assert_eq!(reason(""), "must not be empty");
    }

    #[test]
    fn rejects_other_shell_syntax() {
        assert!(reason("~other/src").contains("~user"));
        assert!(reason("$HOMEDIR/src").contains("relative"));
        assert!(reason("$XDG_DATA_HOME/x").contains("relative"));
        assert!(reason("/srv/$USER").contains("$HOME"));
        assert!(reason("~/a/$HOME").contains("$HOME"));
    }

    #[test]
    fn rejects_parent_components() {
        assert_eq!(reason("~/src/../etc"), "must not contain `..`");
    }

    #[test]
    fn round_trips_as_written() {
        let m = MountPath::new("${HOME}/src/").unwrap();
        assert_eq!(m.as_str(), "${HOME}/src/");
        assert_eq!(String::from(m), "${HOME}/src/");
    }

    fn host() -> HostDirs {
        HostDirs {
            home: HOME.into(),
            protected: [
                "/home/mj/.config/koje",
                "/home/mj/.local/share/koje",
                "/home/mj/.local/state/koje",
                "/home/mj/.local/share/containers",
                "/run/user/1000",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        }
    }

    fn mounts(raw: &[&str]) -> Vec<Mount> {
        raw.iter()
            .map(|m| Mount::from(MountPath::new(*m).unwrap()))
            .collect()
    }

    fn refused(raw: &str) -> String {
        match expand_all(&mounts(&[raw]), &host()) {
            Err(Error::RefusedMount { reason, .. }) => reason,
            other => panic!("expected RefusedMount for {raw:?}, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicates_after_expansion() {
        let mounts = mounts(&["~/src/kp", "~/Austausch", "/home/mj/src/kp/"]);
        match expand_all(&mounts, &host()) {
            Err(Error::DuplicateMount {
                path,
                first,
                second,
            }) => {
                assert_eq!(path, PathBuf::from("/home/mj/src/kp"));
                assert_eq!(first, "~/src/kp");
                assert_eq!(second, "/home/mj/src/kp/");
            }
            other => panic!("expected DuplicateMount, got {other:?}"),
        }
    }

    #[test]
    fn nested_mounts_are_not_duplicates() {
        let paths = expand_all(&mounts(&["~/src", "~/src/kp", "/srv/data"]), &host()).unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/mj/src"),
                PathBuf::from("/home/mj/src/kp"),
                PathBuf::from("/srv/data"),
            ]
        );
    }

    #[test]
    fn refuses_home_and_its_ancestors() {
        for raw in ["~", "$HOME", "/home", "/"] {
            assert!(refused(raw).contains("home"), "{raw}");
        }
    }

    #[test]
    fn refuses_overlap_with_protected_folders() {
        for raw in [
            "~/.config",
            "~/.config/koje",
            "~/.config/koje/contexts/kp",
            "~/.local",
            "~/.local/state/koje",
            "~/.local/share/containers/storage",
            "/run/user/1000",
        ] {
            assert!(refused(raw).contains("protected"), "{raw}");
        }
        assert!(
            expand_all(
                &mounts(&["~/.config/nvim", "~/.local/share/fonts"]),
                &host()
            )
            .is_ok()
        );
    }

    #[test]
    fn refuses_system_folders() {
        for raw in [
            "/usr",
            "/etc/ssh",
            "/proc",
            "/var/lib",
            "/run/lock",
            "/lib64",
        ] {
            assert!(refused(raw).contains("system"), "{raw}");
        }
        assert!(
            expand_all(
                &mounts(&["/srv", "/opt/tools", "/mnt/usb", "/usrdata"]),
                &host()
            )
            .is_ok()
        );
    }

    #[test]
    fn home_below_var_is_not_a_system_folder() {
        // Fedora Atomic and others keep home in `/var/home`.
        let host = HostDirs {
            home: "/var/home/mj".into(),
            protected: vec![],
        };
        let ok = expand_all(&mounts(&["~/src"]), &host).unwrap();
        assert_eq!(ok, [PathBuf::from("/var/home/mj/src")]);
        assert!(matches!(
            expand_all(&mounts(&["/var"]), &host),
            Err(Error::RefusedMount { .. })
        ));
    }

    #[test]
    fn unsafe_bypasses_refusal_but_not_duplicates() {
        let mut list = mounts(&["~/.config", "/etc"]);
        for m in &mut list {
            m.allow_unsafe = true;
        }
        assert!(expand_all(&list, &host()).is_ok());
        list.push(list[0].clone());
        assert!(matches!(
            expand_all(&list, &host()),
            Err(Error::DuplicateMount { .. })
        ));
    }
}
