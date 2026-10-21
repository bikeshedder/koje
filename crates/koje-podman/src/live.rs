//! Live mounts (ADR 0033, spike M2.12): the context's home is the home
//! volume's directory, shared inside rootless Podman's namespace and mounted at
//! `$HOME` with `rslave`; folders bound below it there appear at their own path
//! in the running context. All mounting happens in Podman's namespace
//! (`podman unshare`): no root, the host's own view is unaffected.

use std::path::{Path, PathBuf};

use koje_core::container::home_volume;
use koje_core::live_mounts::{LABEL_MOUNTS, decode};
use koje_core::name::ContextName;

use crate::{Podman, Result, Runner, container_name};

impl<R: Runner> Podman<R> {
    fn unshare(&self, script: &str, args: &[&str]) -> Result<String> {
        let mut all = vec!["unshare", "sh", "-c", script, "sh"];
        all.extend_from_slice(args);
        self.podman(&all)
    }

    /// The home volume's directory on the host, created if needed and made a
    /// shared mount point in Podman's namespace (idempotent); before the
    /// container starts, so its `rslave` home follows it.
    pub fn prepare_home(&self, ctx: &ContextName) -> Result<PathBuf> {
        let volume = home_volume(ctx);
        if !self.volume_exists(&volume)? {
            self.podman(&["volume", "create", &volume])?;
        }
        let dir = self.podman(&["volume", "inspect", "--format", "{{.Mountpoint}}", &volume])?;
        self.unshare(
            r#"mountpoint -q "$1" || { mount --bind "$1" "$1" && mount --make-rshared "$1"; }"#,
            &[&dir],
        )?;
        Ok(PathBuf::from(dir))
    }

    /// Undo [`Podman::prepare_home`] and every live mount below it, before the
    /// volume is removed (`koje rm`): a mount point cannot be deleted.
    pub fn release_home(&self, ctx: &ContextName) -> Result<()> {
        let volume = home_volume(ctx);
        if !self.volume_exists(&volume)? {
            return Ok(());
        }
        let dir = self.podman(&["volume", "inspect", "--format", "{{.Mountpoint}}", &volume])?;
        self.unshare(
            r#"while mountpoint -q "$1"; do umount -R -l "$1" || break; done; true"#,
            &[&dir],
        )
        .map(drop)
    }

    /// Of `rels` (paths below the home), those mounted live now.
    pub fn live_mounted(&self, home_dir: &Path, rels: &[PathBuf]) -> Result<Vec<PathBuf>> {
        if rels.is_empty() {
            return Ok(Vec::new());
        }
        let d = home_dir.display().to_string();
        let strs: Vec<String> = rels.iter().map(|r| r.display().to_string()).collect();
        let mut args = vec![d.as_str()];
        args.extend(strs.iter().map(String::as_str));
        let out = self.unshare(
            r#"d=$1; shift; for r; do mountpoint -q "$d/$r" && printf '%s\n' "$r"; done; true"#,
            &args,
        )?;
        Ok(out.lines().map(PathBuf::from).collect())
    }

    /// Bind `source` to `home_dir/rel`: it appears at its own path in the
    /// context (read-write).
    pub fn live_mount(&self, home_dir: &Path, rel: &Path, source: &Path) -> Result<()> {
        let (d, r, s) = (
            home_dir.display().to_string(),
            rel.display().to_string(),
            source.display().to_string(),
        );
        self.unshare(
            r#"mkdir -p "$1/$2" && mount --bind "$3" "$1/$2""#,
            &[&d, &r, &s],
        )
        .map(drop)
    }

    /// Unmount `home_dir/rel` lazily: a process using it keeps running (M2.12).
    /// The empty mount point stays only if something else is in it.
    pub fn live_unmount(&self, home_dir: &Path, rel: &Path) -> Result<()> {
        let (d, r) = (home_dir.display().to_string(), rel.display().to_string());
        self.unshare(
            r#"umount -l "$1/$2" && { rmdir "$1/$2" 2>/dev/null; true; }"#,
            &[&d, &r],
        )
        .map(drop)
    }

    /// The configured mounts the container was created with; `None` for a
    /// container without a shared home (from before live mounts).
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Fake;

    #[test]
    fn calls_podman_unshare() {
        let fake = Fake::default();
        let podman = Podman::with_runner(&fake, "/x", "/x");
        let ctx = ContextName::new("kp").unwrap();
        let _ = podman.prepare_home(&ctx);
        let home = Path::new("/v/koje-kp-home/_data");
        podman
            .live_mount(home, Path::new("src/kp"), Path::new("/home/mj/src/kp"))
            .unwrap();
        podman.live_unmount(home, Path::new("src/kp")).unwrap();
        let calls = fake.calls.borrow();
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with("volume inspect --format {{.Mountpoint}} koje-kp-home")),
            "{calls:?}"
        );
        let mount = calls
            .iter()
            .find(|c| c.contains("mount --bind \"$3\""))
            .unwrap();
        assert!(mount.starts_with("unshare sh -c"), "{mount}");
        assert!(
            mount.ends_with("sh /v/koje-kp-home/_data src/kp /home/mj/src/kp"),
            "{mount}"
        );
        assert!(calls.last().unwrap().contains("umount -l"), "lazily");
    }
}
