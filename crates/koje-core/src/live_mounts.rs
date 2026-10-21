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

/// What to do so the running context has exactly the configured mounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Configured, below the home, neither real nor live yet: mount live.
    pub add: Vec<PathBuf>,
    /// Live mounts no longer configured: unmount (lazily).
    pub remove: Vec<PathBuf>,
    /// Only a recreate helps: real mounts no longer configured, and new mounts
    /// outside the home.
    pub needs_recreate: Vec<PathBuf>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty() && self.needs_recreate.is_empty()
    }
}

/// `desired`: the configured mounts (resolved); `real`: those the container
/// was created with (its label); `live`: the folders mounted live now.
pub fn plan(desired: &[PathBuf], real: &[PathBuf], live: &[PathBuf], home: &Path) -> Plan {
    let real_set: BTreeSet<&PathBuf> = real.iter().collect();
    let desired_set: BTreeSet<&PathBuf> = desired.iter().collect();
    let live_set: BTreeSet<&PathBuf> = live.iter().collect();
    let mut plan = Plan::default();
    for p in desired
        .iter()
        .filter(|p| !real_set.contains(p) && !live_set.contains(p))
    {
        if below_home(p, home).is_some() {
            plan.add.push(p.clone());
        } else {
            plan.needs_recreate.push(p.clone());
        }
    }
    plan.remove = live
        .iter()
        .filter(|p| !desired_set.contains(p))
        .cloned()
        .collect();
    plan.needs_recreate
        .extend(real.iter().filter(|p| !desired_set.contains(p)).cloned());
    plan
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
    fn plan_adds_removes_and_asks() {
        let home = p("/home/mj");
        let real = [p("/home/mj/src/kp"), p("/home/mj/docs")];
        let live = [p("/home/mj/old"), p("/home/mj/new")];
        let desired = [
            p("/home/mj/src/kp"),
            p("/home/mj/new"),
            p("/home/mj/more"),
            p("/srv/data"),
        ];
        let plan = plan(&desired, &real, &live, &home);
        assert_eq!(
            plan.add,
            [p("/home/mj/more")],
            "new is live already, kp real"
        );
        assert_eq!(plan.remove, [p("/home/mj/old")]);
        assert_eq!(
            plan.needs_recreate,
            [p("/srv/data"), p("/home/mj/docs")],
            "outside the home; a removed real mount"
        );
        assert!(super::plan(&real, &real, &[], &home).is_empty());
    }
}
