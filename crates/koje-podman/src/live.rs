//! Live mounts (ADR 0033, spike M2.12): a per-context directory shared inside
//! rootless Podman's namespace and mounted into the container with `rslave`;
//! folders bound into it there reach the running context. All mounting happens
//! in Podman's namespace (`podman unshare`): no root, the host is unaffected.

use std::path::{Path, PathBuf};

use koje_core::live_mounts::{LABEL_MOUNTS, decode};
use koje_core::name::ContextName;

use crate::{Error, Podman, Result, Runner, container_name};

/// What [`Podman::link_in_context`] found at the folder's own path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Created,
    /// Something real is there (e.g. a folder in the context's home): left
    /// alone, the folder is only below the live directory.
    Occupied,
}

impl<R: Runner> Podman<R> {
    fn unshare(&self, script: &str, args: &[&str]) -> Result<String> {
        let mut all = vec!["unshare", "sh", "-c", script, "sh"];
        all.extend_from_slice(args);
        self.podman(&all)
    }

    /// Make `dir` a shared mount point in Podman's namespace (idempotent);
    /// before the container starts, so its `rslave` mount follows it.
    pub fn prepare_live_dir(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|source| Error::Read {
            path: dir.to_owned(),
            source,
        })?;
        let d = dir.display().to_string();
        self.unshare(
            r#"mountpoint -q "$1" || { mount --bind "$1" "$1" && mount --make-rshared "$1"; }"#,
            &[&d],
        )
        .map(drop)
    }

    /// The entries of `dir` that are mounted now.
    pub fn live_slots(&self, dir: &Path) -> Result<Vec<String>> {
        let d = dir.display().to_string();
        let out = self.unshare(
            r#"for e in "$1"/*/; do e=${e%/}; [ -d "$e" ] && mountpoint -q "$e" && basename "$e"; done; true"#,
            &[&d],
        )?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// Bind `source` into `dir/slot` (read-write).
    pub fn live_mount(&self, dir: &Path, slot: &str, source: &Path) -> Result<()> {
        let (d, s) = (dir.display().to_string(), source.display().to_string());
        self.unshare(
            r#"mkdir -p "$1/$2" && mount --bind "$3" "$1/$2""#,
            &[&d, slot, &s],
        )
        .map(drop)
    }

    /// Unmount `dir/slot` lazily: a process using it keeps running (M2.12).
    pub fn live_unmount(&self, dir: &Path, slot: &str) -> Result<()> {
        let d = dir.display().to_string();
        self.unshare(r#"umount -l "$1/$2" && rmdir "$1/$2""#, &[&d, slot])
            .map(drop)
    }

    /// The configured mounts the container was created with; `None` for a
    /// container from before live mounts (no label).
    pub fn container_mounts(&self, ctx: &ContextName) -> Result<Option<Vec<PathBuf>>> {
        let format = format!("{{{{index .Config.Labels \"{LABEL_MOUNTS}\"}}}}");
        let out = self.podman(&[
            "container",
            "inspect",
            "--format",
            &format,
            &container_name(ctx),
        ])?;
        Ok(decode(out.trim()))
    }

    /// A symlink at `path` inside the context to `target`, unless something
    /// real is there. `as_root` for paths outside the context's home.
    pub fn link_in_context(
        &self,
        ctx: &ContextName,
        path: &Path,
        target: &Path,
        as_root: bool,
    ) -> Result<Link> {
        let name = container_name(ctx);
        let (p, t) = (path.display().to_string(), target.display().to_string());
        let mut args = vec!["exec"];
        if as_root {
            args.extend(["--user", "root"]);
        }
        args.extend([
            name.as_str(),
            "sh",
            "-c",
            r#"if [ -L "$1" ] || [ ! -e "$1" ]; then mkdir -p "$(dirname "$1")" && ln -sfn "$2" "$1" && echo linked; else echo occupied; fi"#,
            "sh",
            &p,
            &t,
        ]);
        let out = self.podman(&args)?;
        Ok(if out.trim() == "linked" {
            Link::Created
        } else {
            Link::Occupied
        })
    }

    /// Remove the symlink at `path` inside the context (nothing else).
    pub fn unlink_in_context(&self, ctx: &ContextName, path: &Path) -> Result<()> {
        let name = container_name(ctx);
        let p = path.display().to_string();
        self.podman(&[
            "exec",
            "--user",
            "root",
            &name,
            "sh",
            "-c",
            r#"[ -L "$1" ] && rm "$1"; true"#,
            "sh",
            &p,
        ])
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Fake;

    #[test]
    fn calls_podman_unshare_and_exec() {
        let fake = Fake::default().answer(true, "linked\n", "");
        let podman = Podman::with_runner(&fake, "/x", "/x");
        let ctx = ContextName::new("kp").unwrap();
        let dir = Path::new("/run/user/1000/koje/kp/mounts");
        assert_eq!(
            podman
                .link_in_context(
                    &ctx,
                    Path::new("/home/mj/src"),
                    Path::new("/run/user/1000/koje-mounts/s"),
                    false
                )
                .unwrap(),
            Link::Created
        );
        podman
            .live_mount(dir, "home-mj-src-1", Path::new("/home/mj/src"))
            .unwrap();
        podman.live_unmount(dir, "home-mj-src-1").unwrap();
        let calls = fake.calls.borrow();
        assert!(
            calls[0].starts_with("exec koje-kp sh -c"),
            "as the user: {}",
            calls[0]
        );
        assert!(
            calls[0].ends_with("sh /home/mj/src /run/user/1000/koje-mounts/s"),
            "{}",
            calls[0]
        );
        assert!(calls[1].starts_with("unshare sh -c"), "{}", calls[1]);
        assert!(
            calls[1].ends_with("sh /run/user/1000/koje/kp/mounts home-mj-src-1 /home/mj/src"),
            "{}",
            calls[1]
        );
        assert!(calls[2].contains("umount -l"), "lazily: {}", calls[2]);
    }
}
