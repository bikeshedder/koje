//! Folders mounted into a running context (M5.11, spike M2.12, ADR 0033).
//!
//! Mounts in `context.toml` are real mounts at their own path when the
//! container is created; the container records them in a label. Each context
//! also has a *live directory*: shared inside rootless Podman's namespace and
//! mounted into the container with `rslave`, so folders bound into it later
//! appear in the running context. There a symlink at the folder's own path
//! shows it where it belongs. This module decides; koje-podman executes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::name::ContextName;

/// Label with the configured mounts a container was created with (JSON list).
pub const LABEL_MOUNTS: &str = "io.koje.mounts";

/// The live directory on the host: `<runtime>/koje/<context>/mounts`.
pub fn host_dir(runtime: &Path, ctx: &ContextName) -> PathBuf {
    runtime.join("koje").join(ctx.as_str()).join("mounts")
}

/// Where the live directory is in the context, below its `XDG_RUNTIME_DIR`.
pub const DIR_IN_CONTEXT: &str = "koje-mounts";

/// FNV-1a, enough to tell paths apart in a directory name.
fn short_hash(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The name of a folder's entry in the live directory: readable path parts
/// plus a hash (`/home/mj/src/kp` → `home-mj-src-kp-1a2b3c4d`).
pub fn slot(path: &Path) -> String {
    let text = path.display().to_string();
    let readable: String = text
        .trim_start_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut readable = readable.trim_matches('-').to_owned();
    readable.truncate(60);
    format!("{readable}-{:08x}", short_hash(&text))
}

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

/// What to do so the running context has exactly the configured mounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Configured but neither a real nor a live mount yet: mount live.
    pub add: Vec<PathBuf>,
    /// Live entries (slots) no longer configured: unmount (lazily).
    pub remove: Vec<String>,
    /// Real mounts no longer configured: only a recreate removes them.
    pub needs_recreate: Vec<PathBuf>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty() && self.needs_recreate.is_empty()
    }
}

/// `desired`: the configured mounts (resolved); `real`: those the container
/// was created with (its label); `live`: the slots mounted in the live
/// directory now.
pub fn plan(desired: &[PathBuf], real: &[PathBuf], live: &[String]) -> Plan {
    let real_set: BTreeSet<&PathBuf> = real.iter().collect();
    let desired_set: BTreeSet<&PathBuf> = desired.iter().collect();
    let live_set: BTreeSet<&str> = live.iter().map(String::as_str).collect();
    let wanted_slots: BTreeSet<String> = desired
        .iter()
        .filter(|p| !real_set.contains(p))
        .map(|p| slot(p))
        .collect();
    Plan {
        add: desired
            .iter()
            .filter(|p| !real_set.contains(p) && !live_set.contains(slot(p).as_str()))
            .cloned()
            .collect(),
        remove: live
            .iter()
            .filter(|s| !wanted_slots.contains(*s))
            .cloned()
            .collect(),
        needs_recreate: real
            .iter()
            .filter(|p| !desired_set.contains(p))
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
    fn slots_are_readable_and_distinct() {
        let a = slot(&p("/home/mj/src/kp"));
        assert!(a.starts_with("home-mj-src-kp-"), "{a}");
        assert_eq!(a.len(), "home-mj-src-kp-".len() + 8);
        assert_ne!(
            slot(&p("/home/mj/a-b")),
            slot(&p("/home/mj/a/b")),
            "same readable part"
        );
        assert_eq!(slot(&p("/x")), slot(&p("/x")));
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
    fn plan_adds_removes_and_asks() {
        let real = [p("/home/mj/src/kp"), p("/home/mj/docs")];
        let live = [slot(&p("/home/mj/old")), slot(&p("/home/mj/new"))];
        let desired = [p("/home/mj/src/kp"), p("/home/mj/new"), p("/home/mj/more")];
        let plan = plan(&desired, &real, &live);
        assert_eq!(
            plan.add,
            [p("/home/mj/more")],
            "new is live already, kp real"
        );
        assert_eq!(plan.remove, [slot(&p("/home/mj/old"))]);
        assert_eq!(plan.needs_recreate, [p("/home/mj/docs")]);
        assert!(super::plan(&real, &real, &[]).is_empty());
    }
}
