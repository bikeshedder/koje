//! Helpers for koje's own files.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::Path;

use crate::error::{Error, Result};

/// Write `bytes` to `path` so that readers see either the old or the new
/// content, also after a power loss: temp file in the same directory, fsync,
/// rename, fsync of the directory. Creates missing parent directories.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let err = |source| Error::Write {
        path: path.to_owned(),
        source,
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(err)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));

    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(err)
}

/// Remove a file; a missing file is not an error. Returns whether it existed.
pub fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Write {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Read a file to a string; `None` if it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_owned(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_creates_dirs_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/state.toml");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["state.toml"]);
    }

    #[test]
    fn failed_write_keeps_old_content() {
        let tmp = tempfile::tempdir().unwrap();
        // The target is a directory, so the rename fails after the temp file is written.
        let path = tmp.path().join("busy");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inner"), "x").unwrap();
        assert!(matches!(
            write_atomic(&path, b"new"),
            Err(Error::Write { .. })
        ));
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn remove_and_read_tolerate_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        assert_eq!(read_optional(&path).unwrap(), None);
        assert!(!remove_if_exists(&path).unwrap());
        fs::write(&path, "y").unwrap();
        assert_eq!(read_optional(&path).unwrap().as_deref(), Some("y"));
        assert!(remove_if_exists(&path).unwrap());
    }
}
