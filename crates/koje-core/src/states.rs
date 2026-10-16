//! A context's state files: read, write, list, copy, rename, remove.
//!
//! Rules (ADR 0005, `docs/decisions/0013-state-file-operations.md`):
//! - `empty` is virtual: it can be read and copied from, never written,
//!   renamed or removed.
//! - `cp`/`mv` never overwrite; deleting happens only via [`remove`].
//! - Role states (`default`, `fresh`) may be renamed or removed; the load
//!   fallback covers a missing one. [`Role`] tells the caller to mention it.

use std::path::Path;
use std::time::SystemTime;

use crate::config::StateRoles;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::name::{ContextName, LayoutName, StateName};
use crate::paths::Paths;
use crate::state::{Loaded, State, ensure_writable};

/// Read a stored state of a context; `None` if it does not exist.
/// `empty` is virtual and never read from disk.
pub fn read(paths: &Paths, ctx: &ContextName, name: &StateName) -> Result<Option<Loaded>> {
    if name.is_empty_state() {
        return Ok(Some(Loaded {
            state: State::new(LayoutName::initial()),
            repaired: Vec::new(),
        }));
    }
    State::load(&paths.state_file(ctx, name))
}

/// Write a state of a context atomically; `empty` is refused.
pub fn write(paths: &Paths, ctx: &ContextName, name: &StateName, state: &State) -> Result<()> {
    ensure_writable(name)?;
    state.save(&paths.state_file(ctx, name))
}

pub fn exists(paths: &Paths, ctx: &ContextName, name: &StateName) -> bool {
    name.is_empty_state() || paths.state_file(ctx, name).is_file()
}

/// Which role a state name plays in a context, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Default,
    Fresh,
}

impl Role {
    pub fn of(name: &StateName, roles: &StateRoles) -> Option<Self> {
        if *name == roles.default {
            Some(Self::Default)
        } else if *name == roles.fresh {
            Some(Self::Fresh)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Fresh => "fresh",
        }
    }
}

/// What a stored state contains, for `koje state list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    Ok {
        windows: usize,
        layouts: usize,
    },
    /// The file exists but cannot be loaded; the message says why.
    Invalid(String),
}

/// One row of `koje state list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateInfo {
    pub name: StateName,
    pub role: Option<Role>,
    /// `None` for role states that were never saved and for `empty`.
    pub summary: Option<Summary>,
    pub modified: Option<SystemTime>,
}

/// All stored states, then role states not saved yet, then `empty`.
pub fn list(paths: &Paths, ctx: &ContextName, roles: &StateRoles) -> Result<Vec<StateInfo>> {
    let mut rows = Vec::new();
    for name in stored_names(paths, ctx)? {
        let path = paths.state_file(ctx, &name);
        let summary = match State::load(&path) {
            Ok(Some(loaded)) => Summary::Ok {
                windows: loaded.state.windows.len(),
                layouts: loaded.state.layout.len(),
            },
            Ok(None) => continue, // removed in the meantime
            Err(e) => Summary::Invalid(e.to_string()),
        };
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        rows.push(StateInfo {
            role: Role::of(&name, roles),
            name,
            summary: Some(summary),
            modified,
        });
    }
    for name in [&roles.default, &roles.fresh] {
        if !rows.iter().any(|r| r.name == *name) {
            rows.push(StateInfo {
                name: name.clone(),
                role: Role::of(name, roles),
                summary: None,
                modified: None,
            });
        }
    }
    rows.push(StateInfo {
        name: StateName::new(StateName::EMPTY)?,
        role: None,
        summary: None,
        modified: None,
    });
    Ok(rows)
}

/// Names of the state files of a context, sorted. Files that are not
/// `<valid-name>.toml` are ignored.
fn stored_names(paths: &Paths, ctx: &ContextName) -> Result<Vec<StateName>> {
    let dir = paths.states_dir(ctx);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Read { path: dir, source }),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Read {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toml") || !path.is_file() {
            continue;
        }
        if let Some(name) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| StateName::new(s).ok())
            .filter(|n| !n.is_empty_state())
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn ensure_exists(paths: &Paths, ctx: &ContextName, name: &StateName) -> Result<()> {
    if exists(paths, ctx, name) {
        Ok(())
    } else {
        Err(Error::UnknownState {
            context: ctx.clone(),
            state: name.clone(),
        })
    }
}

/// `cp`/`mv` never overwrite: deleting is only done by [`remove`].
fn ensure_free(paths: &Paths, ctx: &ContextName, name: &StateName) -> Result<()> {
    ensure_writable(name)?;
    if exists(paths, ctx, name) {
        Err(Error::StateExists {
            context: ctx.clone(),
            state: name.clone(),
        })
    } else {
        Ok(())
    }
}

/// Copy a state. `from` may be `empty` (gives a state without windows).
/// The source is validated first; a stored source is copied byte for byte.
pub fn copy(paths: &Paths, ctx: &ContextName, from: &StateName, to: &StateName) -> Result<()> {
    ensure_exists(paths, ctx, from)?;
    ensure_free(paths, ctx, to)?;
    if from.is_empty_state() {
        return write(paths, ctx, to, &State::new(LayoutName::initial()));
    }
    let src = paths.state_file(ctx, from);
    let text = std::fs::read_to_string(&src).map_err(|source| Error::Read {
        path: src.clone(),
        source,
    })?;
    State::parse(&text, &src)?;
    fsutil::write_atomic(&paths.state_file(ctx, to), text.as_bytes())
}

/// Rename a state.
pub fn rename(paths: &Paths, ctx: &ContextName, from: &StateName, to: &StateName) -> Result<()> {
    ensure_writable(from)?;
    ensure_exists(paths, ctx, from)?;
    ensure_free(paths, ctx, to)?;
    let (src, dst) = (paths.state_file(ctx, from), paths.state_file(ctx, to));
    std::fs::rename(&src, &dst).map_err(|source| Error::Write { path: dst, source })?;
    sync_dir(&src)
}

/// Delete a state: the only way states are deleted (ADR 0005).
pub fn remove(paths: &Paths, ctx: &ContextName, name: &StateName) -> Result<()> {
    ensure_writable(name)?;
    ensure_exists(paths, ctx, name)?;
    let path = paths.state_file(ctx, name);
    fsutil::remove_if_exists(&path)?;
    sync_dir(&path)
}

fn sync_dir(file: &Path) -> Result<()> {
    let Some(dir) = file.parent() else {
        return Ok(());
    };
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|source| Error::Write {
            path: dir.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"
active_layout = "code"
windows = [{ id = "editor", integration = "editor" }]
[layout.code.windows]
editor = { monitor = "DP-1", x = 0, y = 0, w = 100, h = 100 }
"#;

    fn s(n: &str) -> StateName {
        StateName::new(n).unwrap()
    }

    fn ctx() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    fn setup() -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        let paths = Paths::with_roots(r.join("c"), r.join("d"), r.join("s"));
        (tmp, paths)
    }

    fn store(paths: &Paths, name: &str, text: &str) {
        fsutil::write_atomic(&paths.state_file(&ctx(), &s(name)), text.as_bytes()).unwrap();
    }

    #[test]
    fn write_and_read_round_trip() {
        let (_tmp, paths) = setup();
        let st = State::from_toml(STATE, Path::new("x")).unwrap();
        assert_eq!(read(&paths, &ctx(), &s("default")).unwrap(), None);
        write(&paths, &ctx(), &s("default"), &st).unwrap();
        let loaded = read(&paths, &ctx(), &s("default")).unwrap().unwrap();
        assert_eq!(loaded.state, st);
        assert_eq!(loaded.repaired, []);
    }

    #[test]
    fn empty_is_virtual_and_read_only() {
        let (_tmp, paths) = setup();
        let loaded = read(&paths, &ctx(), &s("empty")).unwrap().unwrap();
        assert_eq!(loaded.state.windows, []);
        assert_eq!(loaded.state.issues(), []);
        let st = State::new(LayoutName::initial());
        for result in [
            write(&paths, &ctx(), &s("empty"), &st),
            rename(&paths, &ctx(), &s("empty"), &s("x")),
            remove(&paths, &ctx(), &s("empty")),
            copy(&paths, &ctx(), &s("empty"), &s("empty")),
        ] {
            assert!(matches!(result, Err(Error::ReservedState(_))), "{result:?}");
        }
        assert!(!paths.states_dir(&ctx()).exists());
    }

    #[test]
    fn copy_keeps_bytes_and_never_overwrites() {
        let (_tmp, paths) = setup();
        store(&paths, "default", STATE);
        copy(&paths, &ctx(), &s("default"), &s("fresh")).unwrap();
        let copied = std::fs::read_to_string(paths.state_file(&ctx(), &s("fresh"))).unwrap();
        assert_eq!(copied, STATE);
        assert!(matches!(
            copy(&paths, &ctx(), &s("default"), &s("fresh")),
            Err(Error::StateExists { .. })
        ));
        assert!(matches!(
            copy(&paths, &ctx(), &s("missing"), &s("x")),
            Err(Error::UnknownState { .. })
        ));
    }

    #[test]
    fn copy_from_empty_resets_a_state() {
        let (_tmp, paths) = setup();
        copy(&paths, &ctx(), &s("empty"), &s("fresh")).unwrap();
        let loaded = read(&paths, &ctx(), &s("fresh")).unwrap().unwrap();
        assert_eq!(loaded.state, State::new(LayoutName::initial()));
    }

    #[test]
    fn copy_refuses_invalid_source() {
        let (_tmp, paths) = setup();
        store(&paths, "broken", "not = [toml");
        assert!(copy(&paths, &ctx(), &s("broken"), &s("x")).is_err());
        assert!(!exists(&paths, &ctx(), &s("x")));
    }

    #[test]
    fn rename_and_remove() {
        let (_tmp, paths) = setup();
        store(&paths, "default", STATE);
        store(&paths, "review", STATE);
        assert!(matches!(
            rename(&paths, &ctx(), &s("default"), &s("review")),
            Err(Error::StateExists { .. })
        ));
        rename(&paths, &ctx(), &s("default"), &s("workshop")).unwrap();
        assert!(!exists(&paths, &ctx(), &s("default")));
        assert!(exists(&paths, &ctx(), &s("workshop")));
        remove(&paths, &ctx(), &s("workshop")).unwrap();
        assert!(matches!(
            remove(&paths, &ctx(), &s("workshop")),
            Err(Error::UnknownState { .. })
        ));
    }

    #[test]
    fn list_shows_stored_roles_and_empty() {
        let (_tmp, paths) = setup();
        store(&paths, "review", STATE);
        store(&paths, "default", STATE);
        store(&paths, "broken", "x");
        std::fs::write(paths.states_dir(&ctx()).join("notes.txt"), "").unwrap();
        let rows = list(&paths, &ctx(), &StateRoles::default()).unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["broken", "default", "review", "fresh", "empty"]);
        assert!(matches!(rows[0].summary, Some(Summary::Invalid(_))));
        assert_eq!(rows[1].role, Some(Role::Default));
        assert_eq!(
            rows[2].summary,
            Some(Summary::Ok {
                windows: 1,
                layouts: 1
            })
        );
        assert!(rows[2].modified.is_some());
        assert_eq!((rows[3].role, &rows[3].summary), (Some(Role::Fresh), &None));
    }
}
