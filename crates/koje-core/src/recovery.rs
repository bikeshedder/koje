//! The crash recovery file `~/.local/state/koje/<context>/recovery.toml`
//! (ADR 0006).
//!
//! It has the format of a state file and holds the context's current state.
//! `kojed` rewrites it after every change (M5); a clean `down` deletes it, so
//! finding one at `up` means the context did not shut down cleanly. Its
//! modification time is when that state was current.

use std::time::SystemTime;

use crate::error::{Error, Result};
use crate::fsutil;
use crate::name::ContextName;
use crate::paths::Paths;
use crate::state::{Loaded, State};

/// A recovery file found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    pub loaded: Loaded,
    /// When the file was last written.
    pub saved_at: SystemTime,
}

/// Write the current state atomically.
pub fn write(paths: &Paths, ctx: &ContextName, state: &State) -> Result<()> {
    state.save(&paths.recovery_file(ctx))
}

/// Read the recovery file; `None` if there is none.
pub fn read(paths: &Paths, ctx: &ContextName) -> Result<Option<Recovery>> {
    let path = paths.recovery_file(ctx);
    let Some(loaded) = State::load(&path)? else {
        return Ok(None);
    };
    let saved_at = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
    Ok(Some(Recovery { loaded, saved_at }))
}

/// When the recovery file was written, if there is one. Does not parse it.
pub fn present(paths: &Paths, ctx: &ContextName) -> Option<SystemTime> {
    std::fs::metadata(paths.recovery_file(ctx))
        .and_then(|m| m.modified())
        .ok()
}

/// Delete the recovery file (clean `down`, or discarded on `up`).
/// Returns whether there was one.
pub fn delete(paths: &Paths, ctx: &ContextName) -> Result<bool> {
    fsutil::remove_if_exists(&paths.recovery_file(ctx))
}

/// `12 minutes ago`, for the question at `up`.
pub fn ago(elapsed: std::time::Duration) -> String {
    let mins = elapsed.as_secs() / 60;
    match mins {
        0 => "just now".to_owned(),
        1 => "1 minute ago".to_owned(),
        2..=59 => format!("{mins} minutes ago"),
        60..=119 => "1 hour ago".to_owned(),
        120..=1439 => format!("{} hours ago", mins / 60),
        1440..=2879 => "1 day ago".to_owned(),
        _ => format!("{} days ago", mins / 1440),
    }
}

/// What `up` does about a recovery file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// No recovery file: load the requested state as usual.
    None,
    /// Ask the user whether to restore it.
    Ask,
    /// Restore it (`--recover`).
    Restore,
    /// Delete it and load the requested state (`--no-recover`).
    Discard,
}

/// Decide from whether a recovery file exists and the `up` flags (which clap
/// keeps mutually exclusive).
pub fn decide(present: bool, recover: bool, no_recover: bool) -> Action {
    match (present, recover, no_recover) {
        (false, _, _) => Action::None,
        (true, true, _) => Action::Restore,
        (true, false, true) => Action::Discard,
        (true, false, false) => Action::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::LayoutName;

    fn ctx() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    fn setup() -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        let paths = Paths::with_roots(r.join("c"), r.join("d"), r.join("s"));
        (tmp, paths)
    }

    #[test]
    fn write_read_delete() {
        let (_tmp, paths) = setup();
        assert_eq!(present(&paths, &ctx()), None);
        assert_eq!(read(&paths, &ctx()).unwrap(), None);

        let state = State::new(LayoutName::initial());
        write(&paths, &ctx(), &state).unwrap();
        let saved = present(&paths, &ctx()).unwrap();
        let rec = read(&paths, &ctx()).unwrap().unwrap();
        assert_eq!(rec.loaded.state, state);
        assert_eq!(rec.saved_at, saved);

        assert!(delete(&paths, &ctx()).unwrap());
        assert!(!delete(&paths, &ctx()).unwrap());
        assert_eq!(present(&paths, &ctx()), None);
    }

    #[test]
    fn overwrite_is_atomic_and_leaves_no_temp_files() {
        let (_tmp, paths) = setup();
        let mut state = State::new(LayoutName::initial());
        write(&paths, &ctx(), &state).unwrap();
        state.active_layout = LayoutName::new("code").unwrap();
        state.layout.insert(
            state.active_layout.clone(),
            crate::layout::Layout::default(),
        );
        write(&paths, &ctx(), &state).unwrap();
        assert_eq!(read(&paths, &ctx()).unwrap().unwrap().loaded.state, state);
        let dir = paths.recovery_file(&ctx()).parent().unwrap().to_owned();
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
    }

    #[test]
    fn broken_file_is_present_but_not_readable() {
        let (_tmp, paths) = setup();
        fsutil::write_atomic(&paths.recovery_file(&ctx()), b"broken").unwrap();
        assert!(present(&paths, &ctx()).is_some());
        assert!(read(&paths, &ctx()).is_err());
    }

    #[test]
    fn decision_follows_flags() {
        assert_eq!(decide(false, true, false), Action::None);
        assert_eq!(decide(true, false, false), Action::Ask);
        assert_eq!(decide(true, true, false), Action::Restore);
        assert_eq!(decide(true, false, true), Action::Discard);
    }

    #[test]
    fn ages() {
        use std::time::Duration;
        let m = |n: u64| ago(Duration::from_secs(n * 60));
        assert_eq!(ago(Duration::from_secs(30)), "just now");
        assert_eq!(m(1), "1 minute ago");
        assert_eq!(m(12), "12 minutes ago");
        assert_eq!(m(90), "1 hour ago");
        assert_eq!(m(300), "5 hours ago");
        assert_eq!(m(1500), "1 day ago");
        assert_eq!(m(4320), "3 days ago");
    }
}
