//! Configuration files written by the user.
//!
//! - `~/.config/koje/config.toml` → [`GlobalConfig`]
//! - `~/.config/koje/contexts/<context>/context.toml` → [`ContextConfig`]

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::error::{Error, Result};
use crate::layout::Rect;
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
    /// Host folders bound into the context under the same path. `~` is expanded later.
    #[serde(default)]
    pub mounts: Vec<String>,
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

    #[test]
    fn parses_concept_example() {
        let cfg = ContextConfig::from_toml(EXAMPLE, Path::new("context.toml")).unwrap();
        assert_eq!(cfg.name.as_str(), "kundenportal");
        assert_eq!(cfg.color.to_string(), "#2E7D5B");
        assert_eq!(cfg.limits.memory_hard, Some(ByteSize(8 << 30)));
        assert_eq!(cfg.mounts.len(), 2);
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
}
