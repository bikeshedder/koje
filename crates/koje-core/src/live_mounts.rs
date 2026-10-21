//! Folders mounted into a running context (M5.11, spike M2.12, ADR 0033).
//!
//! Mounts in `context.toml` are real mounts at their own path when the
//! container is created; the container records them in a label. The
//! context's home is the home volume's directory, shared inside rootless
//! Podman's namespace and mounted at `$HOME` with `rslave`: a folder bound to
//! `<volume dir>/<its path below $HOME>` there appears at its own path in the
//! running context, as a real mount. This module decides; koje-podman executes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Label with the configured mounts a container was created with (a TOML
/// array). Only containers whose home is shared for live mounts have it.
pub const LABEL_MOUNTS: &str = "io.koje.home-mounts";

/// The label value for the configured mounts: a TOML array of paths.
pub fn encode(mounts: &[PathBuf]) -> String {
    toml::Value::Array(
        mounts
            .iter()
            .map(|m| toml::Value::String(m.display().to_string()))
            .collect(),
    )
    .to_string()
}

/// The paths in a label value; `None` if it is missing or broken (a
/// container from before live mounts).
pub fn decode(label: &str) -> Option<Vec<PathBuf>> {
    #[derive(serde::Deserialize)]
    struct Wrap {
        m: Vec<String>,
    }
    let w: Wrap = toml::from_str(&format!("m = {label}")).ok()?;
    Some(w.m.into_iter().map(PathBuf::from).collect())
}

/// Where a folder is mounted live: its path below `home`; `None` outside the
/// home (or the home itself).
pub fn below_home(path: &Path, home: &Path) -> Option<PathBuf> {
    path.strip_prefix(home)
        .ok()
        .filter(|rel| !rel.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// The folders mounted live, from `findmnt -rn -o TARGET -R <home dir>`
/// inside Podman's namespace: below `home_dir`, top-level ones only (a mount
/// inside a mounted folder belongs to it), as paths below the home.
pub fn parse_findmnt(out: &str, home_dir: &Path) -> Vec<PathBuf> {
    let unescape = |s: &str| {
        // findmnt -r writes unusual characters as \xHH.
        let mut bytes = Vec::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\'
                && b.get(i + 1) == Some(&b'x')
                && let Some(v) = s
                    .get(i + 2..i + 4)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                bytes.push(v);
                i += 4;
            } else {
                bytes.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let mut rels: Vec<PathBuf> = out
        .lines()
        .map(|l| PathBuf::from(unescape(l.trim())))
        .filter_map(|t| t.strip_prefix(home_dir).ok().map(Path::to_path_buf))
        .filter(|r| !r.as_os_str().is_empty())
        .collect();
    rels.sort();
    let mut top: Vec<PathBuf> = Vec::new();
    for r in rels {
        if !top.iter().any(|t| r.starts_with(t)) {
            top.push(r);
        }
    }
    top
}

/// What to do so the context has exactly the configured mounts: folders
/// below the home are live mounts (added and removed at any time), folders
/// outside it real mounts made when the container is created.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Configured below the home, not mounted live yet: mount.
    pub add: Vec<PathBuf>,
    /// Mounted live, no longer configured: unmount (lazily).
    pub remove: Vec<PathBuf>,
    /// Real mounts no longer configured: stay until the container is recreated.
    pub stale_real: Vec<PathBuf>,
    /// Configured outside the home, not a real mount: needs a recreate.
    pub missing_real: Vec<PathBuf>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty()
            && self.remove.is_empty()
            && self.stale_real.is_empty()
            && self.missing_real.is_empty()
    }
}

/// The real mounts for a new container: the configured folders outside the
/// home (those below it are mounted live, ADR 0033).
pub fn real_mounts(desired: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    desired
        .iter()
        .filter(|p| below_home(p, home).is_none())
        .cloned()
        .collect()
}

/// `desired`: the configured mounts (resolved); `real`: the real mounts of the
/// container (its label; `None` before it exists – then it will get
/// [`real_mounts`]); `live`: the folders mounted live now.
pub fn plan(desired: &[PathBuf], real: Option<&[PathBuf]>, live: &[PathBuf], home: &Path) -> Plan {
    let live_set: BTreeSet<&PathBuf> = live.iter().collect();
    let desired_set: BTreeSet<&PathBuf> = desired.iter().collect();
    let below: Vec<&PathBuf> = desired
        .iter()
        .filter(|p| below_home(p, home).is_some())
        .collect();
    let below_set: BTreeSet<&PathBuf> = below.iter().copied().collect();
    let exists = real.is_some();
    let real = real.unwrap_or_default();
    let real_set: BTreeSet<&PathBuf> = real.iter().collect();
    Plan {
        add: below
            .iter()
            .filter(|p| !live_set.contains(*p) && !real_set.contains(*p))
            .map(|p| (*p).clone())
            .collect(),
        remove: live
            .iter()
            .filter(|p| !below_set.contains(p))
            .cloned()
            .collect(),
        stale_real: real
            .iter()
            .filter(|p| !desired_set.contains(p))
            .cloned()
            .collect(),
        // A new container gets them; an existing one only when recreated.
        missing_real: desired
            .iter()
            .filter(|p| exists && below_home(p, home).is_none() && !real_set.contains(p))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn label_round_trip() {
        let m = [p("/home/mj/src/kp"), p("/home/mj/with \"quote\" and ,")];
        assert_eq!(decode(&encode(&m)), Some(m.to_vec()));
        assert_eq!(decode(&encode(&[])), Some(Vec::new()));
        assert_eq!(decode(""), None);
        assert_eq!(decode("<no value>"), None);
    }

    #[test]
    fn below_the_home() {
        let home = p("/home/mj");
        assert_eq!(below_home(&p("/home/mj/src/kp"), &home), Some(p("src/kp")));
        assert_eq!(below_home(&p("/home/mj"), &home), None);
        assert_eq!(below_home(&p("/home/mjx/a"), &home), None);
        assert_eq!(below_home(&p("/srv/data"), &home), None);
    }

    #[test]
    fn findmnt_output() {
        let data = p("/v/koje-kp-home/_data");
        let out = "/v/koje-kp-home/_data\n/v/koje-kp-home/_data/src/kp\n/v/koje-kp-home/_data/src/kp/sub\n/v/koje-kp-home/_data/My\\x20Docs\n";
        assert_eq!(parse_findmnt(out, &data), [p("My Docs"), p("src/kp")]);
        assert_eq!(parse_findmnt("", &data), Vec::<PathBuf>::new());
    }

    #[test]
    fn plan_adds_removes_and_asks() {
        let home = p("/home/mj");
        let desired = [p("/home/mj/src/kp"), p("/home/mj/more"), p("/srv/data")];
        // A new container: below the home live, outside it real.
        let fresh = plan(&desired, None, &[], &home);
        assert_eq!(fresh.add, [p("/home/mj/src/kp"), p("/home/mj/more")]);
        assert_eq!(real_mounts(&desired, &home), [p("/srv/data")]);
        // Running: kp live already, an old live mount goes; a real mount
        // removed from the config stays; /srv/data was never a real mount.
        let live = [p("/home/mj/src/kp"), p("/home/mj/old")];
        let real = [p("/srv/old")];
        let running = plan(&desired, Some(&real), &live, &home);
        assert_eq!(running.add, [p("/home/mj/more")]);
        assert_eq!(running.remove, [p("/home/mj/old")]);
        assert_eq!(running.stale_real, [p("/srv/old")]);
        assert_eq!(running.missing_real, [p("/srv/data")]);
        // A container from before (a creation-time mount below the home in
        // its label): not mounted live a second time.
        let old = plan(
            &[p("/home/mj/src/kp")],
            Some(&[p("/home/mj/src/kp")]),
            &[],
            &home,
        );
        assert!(old.is_empty(), "{old:?}");
    }
}
