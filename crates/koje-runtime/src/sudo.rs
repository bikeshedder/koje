//! `sudo` without a password for your user inside a context (ADR 0066): a
//! sudoers entry koje writes as root in the running container at every
//! start and when the `sudo` switch changes, and removes when it is off.
//! Root inside is not root on the host (rootless Podman, ADR 0014).

use anyhow::{Context as _, Result, bail};
use koje_core::name::ContextName;
use koje_podman::Podman;

/// What the script says when the image has no sudo.
const NO_SUDO: &str = "koje: the image has no sudo";

/// koje's sudoers entry in the container.
pub const SUDOERS: &str = "/etc/sudoers.d/90-koje";

/// The `sh` script that writes (`on`) or removes the entry; the user name
/// comes as `$1`, never inside the script. [`NO_SUDO`] on stderr: the
/// image has no sudo.
pub fn script(on: bool) -> String {
    if on {
        format!(
            "set -e\n\
             command -v sudo >/dev/null 2>&1 || {{ echo '{NO_SUDO}' >&2; exit 3; }}\n\
             mkdir -p /etc/sudoers.d\n\
             printf '%s ALL=(ALL) NOPASSWD: ALL\\n' \"$1\" > {SUDOERS}.tmp\n\
             chmod 0440 {SUDOERS}.tmp\n\
             mv {SUDOERS}.tmp {SUDOERS}\n"
        )
    } else {
        format!("rm -f {SUDOERS}\n")
    }
}

/// Give `ctx`'s user passwordless sudo, or take it away; the container runs.
pub fn apply(podman: &Podman, ctx: &ContextName, on: bool) -> Result<()> {
    let user = koje_podman::host::image_user().context("finding the current user")?;
    match podman.exec_root(ctx, &["sh", "-c", &script(on), "sh", &user.name]) {
        Ok(_) => Ok(()),
        Err(koje_podman::Error::Failed { stderr, .. }) if stderr.contains(NO_SUDO) => {
            bail!("the image has no sudo – install it in the image or turn sudo off")
        }
        Err(e) => Err(e).with_context(|| format!("setting up sudo in {ctx}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_writes_the_entry_atomically_or_removes_it() {
        let on = script(true);
        assert!(on.contains("NOPASSWD: ALL"), "{on}");
        assert!(on.contains("\"$1\""), "the user is an argument");
        assert!(on.contains("chmod 0440"), "{on}");
        assert!(on.contains(NO_SUDO), "{on}");
        assert!(
            on.ends_with("mv /etc/sudoers.d/90-koje.tmp /etc/sudoers.d/90-koje\n"),
            "{on}"
        );
        assert_eq!(script(false), "rm -f /etc/sudoers.d/90-koje\n");
    }

    #[test]
    fn the_script_runs_in_sh() {
        // Syntax only: `sh -n`.
        for on in [true, false] {
            let ok = std::process::Command::new("sh")
                .args(["-n", "-c", &script(on)])
                .status()
                .map_or(true, |s| s.success());
            assert!(ok, "{}", script(on));
        }
    }
}
