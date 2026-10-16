//! Configuration files written by the user.
//!
//! - `~/.config/koje/config.toml` → [`GlobalConfig`]
//! - `~/.config/koje/contexts/<context>/context.toml` → [`ContextConfig`]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::error::{Error, Result};
use crate::layout::Rect;
use crate::mount::{self, HostDirs, Mount};
use crate::name::{ContextName, StateName};
use crate::size::ByteSize;

/// `context.toml`: everything the user maintains by hand for one context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    pub name: ContextName,
    /// Short label for bars and menus, e.g. `KP`.
    pub prefix: String,
    /// Border and accent color.
    pub color: Color,
    /// Container image the context is created from. Defaults to the shared base image.
    #[serde(default = "default_image")]
    pub image: String,
    /// Host folders bound into the context under the same path.
    /// See [`ContextConfig::expand_mounts`].
    #[serde(default)]
    pub mounts: Vec<Mount>,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub states: StateRoles,
}

fn default_image() -> String {
    "koje/base:latest".to_owned()
}

/// Memory limits of a context. Unset values fall back to [`GlobalConfig::default_limits`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Warn at 90 % and 100 % of this value.
    pub memory_soft: Option<ByteSize>,
    /// Shut the context down in an orderly way when exceeded.
    pub memory_hard: Option<ByteSize>,
}

impl Limits {
    /// Fill unset values from `defaults`.
    #[must_use]
    pub fn or(&self, defaults: &Limits) -> Limits {
        Limits {
            memory_soft: self.memory_soft.or(defaults.memory_soft),
            memory_hard: self.memory_hard.or(defaults.memory_hard),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let (Some(soft), Some(hard)) = (self.memory_soft, self.memory_hard)
            && soft >= hard
        {
            return Err(Error::LimitOrder {
                soft: soft.to_string(),
                hard: hard.to_string(),
            });
        }
        Ok(())
    }
}

/// Which state names play the `default` and `fresh` roles (renamable, like git's `main`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRoles {
    /// Read by `up` and written by `down` when no `-s` is given.
    pub default: StateName,
    /// The curated start state; first fallback when a requested state is missing.
    pub fresh: StateName,
}

#[allow(clippy::expect_used)] // literals are known-valid names
impl Default for StateRoles {
    fn default() -> Self {
        Self {
            default: StateName::new("default").expect("valid literal"),
            fresh: StateName::new("fresh").expect("valid literal"),
        }
    }
}

impl ContextConfig {
    pub fn from_toml(text: &str, path: &Path) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| Error::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })?;
        cfg.limits.validate()?;
        if cfg.states.default.is_empty_state() || cfg.states.fresh.is_empty_state() {
            return Err(Error::ReservedState(StateName::EMPTY.to_owned()));
        }
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Read {
            path: path.to_owned(),
            source: e,
        })?;
        Self::from_toml(&text, path)
    }

    /// Limits after filling unset values from the global defaults.
    ///
    /// Validated again: a context's soft limit may be fine on its own but end up
    /// above an inherited hard limit.
    pub fn effective_limits(&self, global: &GlobalConfig) -> Result<Limits> {
        let limits = self.limits.or(&global.default_limits);
        limits.validate()?;
        Ok(limits)
    }

    /// Absolute host paths of all mounts; rejects refused targets (ADR 0009) and
    /// two entries naming the same folder.
    ///
    /// Each entry is already validated on parse; these rules need the host's
    /// folders (see [`crate::paths::Paths::host_dirs`]).
    pub fn expand_mounts(&self, host: &HostDirs) -> Result<Vec<PathBuf>> {
        mount::expand_all(&self.mounts, host)
    }
}

/// `config.toml`: settings shared by all contexts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    #[serde(default)]
    pub default_limits: Limits,
    /// Snap zones per monitor connector name (e.g. `DP-1`), then per zone name.
    #[serde(default)]
    pub zones: BTreeMap<String, BTreeMap<String, Rect>>,
    /// Overlays (Hyprland special workspaces) for cross-context apps.
    #[serde(default)]
    pub overlay: BTreeMap<String, Overlay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    /// Hyprland key combination, e.g. `SUPER+M`.
    pub key: String,
    /// Integrations or launch commands started in the `global` context.
    #[serde(default)]
    pub apps: Vec<String>,
}

impl GlobalConfig {
    pub fn from_toml(text: &str, path: &Path) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| Error::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })?;
        cfg.default_limits.validate()?;
        Ok(cfg)
    }

    /// Load `config.toml`; a missing file means all defaults.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Read {
                path: path.to_owned(),
                source: e,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from the concept document.
    const EXAMPLE: &str = r##"
name   = "kundenportal"
prefix = "KP"
color  = "#2E7D5B"
image  = "koje/base:latest"

mounts = ["~/src/kundenportal", "~/Austausch"]

[limits]
memory_soft = "6G"
memory_hard = "8G"

[states]
default = "default"
fresh   = "fresh"
"##;

    fn host() -> HostDirs {
        HostDirs {
            home: "/home/mj".into(),
            protected: vec!["/home/mj/.config/koje".into()],
        }
    }

    #[test]
    fn parses_concept_example() {
        let cfg = ContextConfig::from_toml(EXAMPLE, Path::new("context.toml")).unwrap();
        assert_eq!(cfg.name.as_str(), "kundenportal");
        assert_eq!(cfg.color.to_string(), "#2E7D5B");
        assert_eq!(cfg.limits.memory_hard, Some(ByteSize(8 << 30)));
        assert_eq!(
            cfg.expand_mounts(&host()).unwrap(),
            [
                PathBuf::from("/home/mj/src/kundenportal"),
                PathBuf::from("/home/mj/Austausch")
            ]
        );
    }

    #[test]
    fn rejects_relative_mount_on_parse() {
        let text = EXAMPLE.replace("\"~/Austausch\"", "\"Austausch\"");
        assert!(ContextConfig::from_toml(&text, Path::new("x")).is_err());
    }

    #[test]
    fn rejects_duplicate_mounts() {
        let text = EXAMPLE.replace("\"~/Austausch\"", "\"$HOME/src/kundenportal\"");
        let cfg = ContextConfig::from_toml(&text, Path::new("x")).unwrap();
        assert!(matches!(
            cfg.expand_mounts(&host()),
            Err(Error::DuplicateMount { .. })
        ));
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let cfg = ContextConfig::from_toml(
            "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#000000\"\n",
            Path::new("x"),
        )
        .unwrap();
        assert_eq!(cfg.image, "koje/base:latest");
        assert_eq!(cfg.states, StateRoles::default());
    }

    #[test]
    fn mounts_round_trip_both_forms() {
        let text = EXAMPLE.replace(
            "\"~/Austausch\"]",
            "\"~/Austausch\", { path = \"~/.config\", unsafe = true }]",
        );
        let cfg = ContextConfig::from_toml(&text, Path::new("x")).unwrap();
        assert!(!cfg.mounts[0].allow_unsafe);
        assert!(cfg.mounts[2].allow_unsafe);
        assert_eq!(cfg.expand_mounts(&host()).unwrap().len(), 3);
        let written = toml::to_string(&cfg).unwrap();
        assert!(written.contains("\"~/Austausch\""), "{written}");
        assert_eq!(
            ContextConfig::from_toml(&written, Path::new("x")).unwrap(),
            cfg
        );
    }

    #[test]
    fn mount_table_errors_keep_their_reason() {
        let bad = EXAMPLE.replace("\"~/Austausch\"]", "{ path = \"rel\" }]");
        let err = ContextConfig::from_toml(&bad, Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("relative"), "{err}");
        let unknown = EXAMPLE.replace("\"~/Austausch\"]", "{ path = \"~/a\", rw = 1 }]");
        assert!(ContextConfig::from_toml(&unknown, Path::new("x")).is_err());
        let string = EXAMPLE.replace("\"~/Austausch\"]", "\"Austausch\"]");
        let err = ContextConfig::from_toml(&string, Path::new("x")).unwrap_err();
        assert!(err.to_string().contains("relative"), "{err}");
    }

    #[test]
    fn refuses_protected_mount() {
        let text = EXAMPLE.replace("\"~/Austausch\"]", "\"~/.config\"]");
        let cfg = ContextConfig::from_toml(&text, Path::new("x")).unwrap();
        assert!(matches!(
            cfg.expand_mounts(&host()),
            Err(Error::RefusedMount { .. })
        ));
    }

    #[test]
    fn rejects_soft_above_hard() {
        let text = EXAMPLE.replace("memory_soft = \"6G\"", "memory_soft = \"9G\"");
        assert!(matches!(
            ContextConfig::from_toml(&text, Path::new("x")),
            Err(Error::LimitOrder { .. })
        ));
    }

    #[test]
    fn rejects_empty_as_role() {
        let text = EXAMPLE.replace("fresh   = \"fresh\"", "fresh   = \"empty\"");
        assert!(matches!(
            ContextConfig::from_toml(&text, Path::new("x")),
            Err(Error::ReservedState(_))
        ));
    }

    #[test]
    fn rejects_unknown_keys() {
        let text = format!("{EXAMPLE}\nunknown = 1\n");
        assert!(ContextConfig::from_toml(&text, Path::new("x")).is_err());
    }

    #[test]
    fn parses_global_config() {
        let text = r#"
[default_limits]
memory_soft = "4G"
memory_hard = "6G"

[zones.DP-1]
links = { x = 0, y = 0, w = 1700, h = 2160 }

[overlay.mail]
key  = "SUPER+M"
apps = ["mail", "calendar"]
"#;
        let cfg = GlobalConfig::from_toml(text, Path::new("config.toml")).unwrap();
        assert_eq!(cfg.zones["DP-1"]["links"].w, 1700);
        assert_eq!(cfg.overlay["mail"].apps, ["mail", "calendar"]);
    }

    #[test]
    fn missing_global_config_is_default() {
        let tmp = std::env::temp_dir().join("koje-test-does-not-exist/config.toml");
        assert_eq!(GlobalConfig::load(&tmp).unwrap(), GlobalConfig::default());
    }

    #[test]
    fn effective_limits_fill_from_global() {
        let ctx = ContextConfig::from_toml(
            "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#000000\"\n[limits]\nmemory_soft = \"2G\"\n",
            Path::new("x"),
        )
        .unwrap();
        let global = GlobalConfig::from_toml(
            "[default_limits]\nmemory_soft = \"4G\"\nmemory_hard = \"6G\"\n",
            Path::new("x"),
        )
        .unwrap();
        let limits = ctx.effective_limits(&global).unwrap();
        assert_eq!(limits.memory_soft, Some(ByteSize(2 << 30)));
        assert_eq!(limits.memory_hard, Some(ByteSize(6 << 30)));
        assert_eq!(
            ctx.effective_limits(&GlobalConfig::default()).unwrap(),
            ctx.limits
        );
    }

    #[test]
    fn effective_limits_are_validated_after_merging() {
        // Soft 7G alone is fine, but not below an inherited hard limit of 6G.
        let ctx = ContextConfig::from_toml(
            "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#000000\"\n[limits]\nmemory_soft = \"7G\"\n",
            Path::new("x"),
        )
        .unwrap();
        let global = GlobalConfig::from_toml(
            "[default_limits]\nmemory_soft = \"4G\"\nmemory_hard = \"6G\"\n",
            Path::new("x"),
        )
        .unwrap();
        assert!(matches!(
            ctx.effective_limits(&global),
            Err(Error::LimitOrder { .. })
        ));
    }
}
