//! What `koje open <context> <target>` hands to the context (M3.6). Pure:
//! the caller resolves paths on the host and runs `xdg-open` in the context.

use std::path::{Path, PathBuf};

/// A target for `koje open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Anything with a URL scheme (`https:`, `mailto:`, …), passed on unchanged.
    Url(String),
    /// A file on the host, absolute (relative paths joined to the cwd).
    File(PathBuf),
}

/// Tell URLs from files. `file://` URLs become files.
pub fn classify(target: &str, cwd: &Path) -> Target {
    if let Some(path) = target.strip_prefix("file://") {
        return Target::File(PathBuf::from(path));
    }
    let scheme = target.split_once(':').map(|(s, _)| s);
    let is_url = scheme.is_some_and(|s| {
        s.len() > 1
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    if is_url {
        Target::Url(target.to_owned())
    } else if Path::new(target).is_absolute() {
        Target::File(PathBuf::from(target))
    } else {
        Target::File(cwd.join(target))
    }
}

/// A host file is visible in the context only inside a mounted folder (same
/// path, ADR 0009). `file` and `mounts` are resolved host paths.
pub fn visible_in(file: &Path, mounts: &[PathBuf]) -> bool {
    mounts.iter().any(|m| file.starts_with(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_files() {
        let cwd = Path::new("/home/mj/src/kp");
        assert_eq!(
            classify("https://example.org", cwd),
            Target::Url("https://example.org".into())
        );
        assert_eq!(
            classify("mailto:a@b.c", cwd),
            Target::Url("mailto:a@b.c".into())
        );
        assert_eq!(
            classify("file:///home/mj/x.pdf", cwd),
            Target::File("/home/mj/x.pdf".into())
        );
        assert_eq!(
            classify("/etc/hosts", cwd),
            Target::File("/etc/hosts".into())
        );
        assert_eq!(
            classify("docs/spec.pdf", cwd),
            Target::File("/home/mj/src/kp/docs/spec.pdf".into())
        );
        // A colon alone does not make a URL: "C:" style, "a:b" with one letter, odd chars.
        assert_eq!(
            classify("x:y", cwd),
            Target::File("/home/mj/src/kp/x:y".into())
        );
        assert_eq!(
            classify("my file: notes.txt", cwd),
            Target::File("/home/mj/src/kp/my file: notes.txt".into())
        );
    }

    #[test]
    fn visible_only_inside_mounts() {
        let mounts = vec![
            PathBuf::from("/home/mj/src/kp"),
            PathBuf::from("/home/mj/Austausch"),
        ];
        assert!(visible_in(
            Path::new("/home/mj/src/kp/docs/spec.pdf"),
            &mounts
        ));
        assert!(visible_in(Path::new("/home/mj/Austausch/a.txt"), &mounts));
        assert!(!visible_in(Path::new("/home/mj/Downloads/a.txt"), &mounts));
        assert!(!visible_in(Path::new("/home/mj/src/kp2/a.txt"), &mounts));
    }
}
