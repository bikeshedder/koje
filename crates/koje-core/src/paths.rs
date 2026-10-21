//! File locations following the XDG base directory spec.
//!
//! | Path | Content |
//! |---|---|
//! | `~/.config/koje/config.toml` | global settings |
//! | `~/.config/koje/contexts/<context>/context.toml` | hand-maintained context config |
//! | `~/.config/koje/integrations/*.toml` | user integrations |
//! | `~/.local/share/koje/<context>/states/<state>.toml` | states incl. layouts |
//! | `~/.local/state/koje/<context>/recovery.toml` | crash recovery |

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::mount::HostDirs;
use crate::name::{ContextName, StateName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    config: PathBuf,
    data: PathBuf,
    state: PathBuf,
}

impl Paths {
    /// Resolve from the environment (`XDG_*` variables or their defaults).
    pub fn from_env() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "koje").ok_or(Error::NoHomeDir)?;
        let state = dirs
            .state_dir()
            .map_or_else(|| dirs.data_local_dir().join("state"), Path::to_path_buf);
        Ok(Self {
            config: dirs.config_dir().to_path_buf(),
            data: dirs.data_dir().to_path_buf(),
            state,
        })
    }

    /// Explicit roots, for tests and `--root`-style overrides.
    pub fn with_roots(config: PathBuf, data: PathBuf, state: PathBuf) -> Self {
        Self {
            config,
            data,
            state,
        }
    }

    /// The host folders mounts are checked against (ADR 0009): the real home,
    /// koje's own roots, the host's container storage and `$XDG_RUNTIME_DIR`.
    pub fn host_dirs(&self) -> Result<HostDirs> {
        let dirs = directories::BaseDirs::new().ok_or(Error::NoHomeDir)?;
        let mut protected = vec![
            self.config.clone(),
            self.data.clone(),
            self.state.clone(),
            dirs.data_dir().join("containers"),
        ];
        protected.extend(dirs.runtime_dir().map(Path::to_path_buf));
        Ok(HostDirs {
            home: dirs.home_dir().to_path_buf(),
            protected,
        })
    }

    /// `$XDG_DATA_HOME` (koje's data root is `$XDG_DATA_HOME/koje`).
    fn data_home(&self) -> PathBuf {
        self.data
            .parent()
            .map_or_else(|| self.data.clone(), Path::to_path_buf)
    }

    /// `$XDG_CONFIG_HOME`.
    fn config_home(&self) -> PathBuf {
        self.config
            .parent()
            .map_or_else(|| self.config.clone(), Path::to_path_buf)
    }

    /// koje's Hyprland snippet (ADR 0021), sourced from `hyprland.conf`.
    pub fn hyprland_snippet(&self) -> PathBuf {
        self.data.join("hyprland/koje.conf")
    }

    /// The user's Hyprland config.
    pub fn hyprland_conf(&self) -> PathBuf {
        self.config_home().join("hypr/hyprland.conf")
    }

    /// The icon font's directory (ADR 0020).
    pub fn font_dir(&self) -> PathBuf {
        self.data_home().join("fonts/koje")
    }

    /// kojed's systemd user unit.
    pub fn kojed_unit(&self) -> PathBuf {
        self.config_home().join("systemd/user/kojed.service")
    }

    pub fn waybar_dir(&self) -> PathBuf {
        self.config_home().join("waybar")
    }

    pub fn global_config(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn contexts_dir(&self) -> PathBuf {
        self.config.join("contexts")
    }

    /// `~/.config/koje/contexts/<context>/`
    pub fn context_dir(&self, ctx: &ContextName) -> PathBuf {
        self.contexts_dir().join(ctx.as_str())
    }

    pub fn context_config(&self, ctx: &ContextName) -> PathBuf {
        self.context_dir(ctx).join("context.toml")
    }

    /// `~/.local/share/koje/<context>/`
    pub fn context_data_dir(&self, ctx: &ContextName) -> PathBuf {
        self.data.join(ctx.as_str())
    }

    /// `~/.local/state/koje/<context>/`
    pub fn context_state_dir(&self, ctx: &ContextName) -> PathBuf {
        self.state.join(ctx.as_str())
    }

    pub fn integrations_dir(&self) -> PathBuf {
        self.config.join("integrations")
    }

    pub fn states_dir(&self, ctx: &ContextName) -> PathBuf {
        self.context_data_dir(ctx).join("states")
    }

    pub fn state_file(&self, ctx: &ContextName, state: &StateName) -> PathBuf {
        self.states_dir(ctx).join(format!("{state}.toml"))
    }

    pub fn recovery_file(&self, ctx: &ContextName) -> PathBuf {
        self.context_state_dir(ctx).join("recovery.toml")
    }

    /// The name of the state a running context is in (ADR 0032), beside the
    /// recovery file; kojed writes it.
    pub fn current_state_file(&self, ctx: &ContextName) -> PathBuf {
        self.context_state_dir(ctx).join("current-state")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_concept() {
        let p = Paths::with_roots("/c".into(), "/d".into(), "/s".into());
        let kp = ContextName::new("kp").unwrap();
        let st = StateName::new("fresh").unwrap();
        assert_eq!(
            p.context_config(&kp),
            PathBuf::from("/c/contexts/kp/context.toml")
        );
        assert_eq!(
            p.state_file(&kp, &st),
            PathBuf::from("/d/kp/states/fresh.toml")
        );
        assert_eq!(p.recovery_file(&kp), PathBuf::from("/s/kp/recovery.toml"));
    }

    #[test]
    fn setup_targets() {
        let p = Paths::with_roots(
            "/h/.config/koje".into(),
            "/h/.local/share/koje".into(),
            "/h/.local/state/koje".into(),
        );
        assert_eq!(
            p.hyprland_snippet(),
            PathBuf::from("/h/.local/share/koje/hyprland/koje.conf")
        );
        assert_eq!(
            p.hyprland_conf(),
            PathBuf::from("/h/.config/hypr/hyprland.conf")
        );
        assert_eq!(p.font_dir(), PathBuf::from("/h/.local/share/fonts/koje"));
        assert_eq!(
            p.kojed_unit(),
            PathBuf::from("/h/.config/systemd/user/kojed.service")
        );
        assert_eq!(p.waybar_dir(), PathBuf::from("/h/.config/waybar"));
    }
}
