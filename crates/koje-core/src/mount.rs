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

/// Expand all mounts against `home` and reject entries that name the same folder.
pub fn expand_all(mounts: &[MountPath], home: &Path) -> Result<Vec<PathBuf>> {
    let mut seen: Vec<(PathBuf, &MountPath)> = Vec::with_capacity(mounts.len());
    for mount in mounts {
        let path = mount.expand(home);
        if let Some((_, first)) = seen.iter().find(|(p, _)| *p == path) {
            return Err(Error::DuplicateMount {
                path,
                first: first.raw.clone(),
                second: mount.raw.clone(),
            });
        }
        seen.push((path, mount));
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

    #[test]
    fn rejects_duplicates_after_expansion() {
        let mounts: Vec<MountPath> = ["~/src/kp", "~/Austausch", "/home/mj/src/kp/"]
            .into_iter()
            .map(|m| m.parse().unwrap())
            .collect();
        match expand_all(&mounts, Path::new(HOME)) {
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
        let mounts: Vec<MountPath> = ["~/src", "~/src/kp"]
            .into_iter()
            .map(|m| m.parse().unwrap())
            .collect();
        let paths = expand_all(&mounts, Path::new(HOME)).unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/mj/src"),
                PathBuf::from("/home/mj/src/kp")
            ]
        );
    }
}
