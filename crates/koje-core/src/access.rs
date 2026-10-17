//! Access profiles: which host resources a context gets
//! (`docs/decisions/0017-access-profiles.md`).
//!
//! `[access]` in `config.toml` sets the default, `[access]` in `context.toml`
//! adjusts it: a profile sets all switches, single switches override it.

use serde::{Deserialize, Serialize};

/// The host resources a context gets, after resolving profiles and switches.
#[allow(clippy::struct_excessive_bools)] // five independent switches, one per resource (ADR 0017)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Access {
    /// `/dev/dri` and NVIDIA via CDI (ADR 0016).
    pub gpu: bool,
    /// `PipeWire` and `PulseAudio` sockets (playback and microphone).
    pub sound: bool,
    /// The compositor's Wayland socket; without it the context has no GUI.
    pub wayland: bool,
    /// Devices and `/proc/sys` for nested Podman (ADR 0014).
    pub nested: bool,
    /// An own network; without it `--network none`.
    pub network: bool,
}

/// Named sets of switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Everything (the default).
    #[default]
    Full,
    /// Desktop programs without the GPU.
    Desktop,
    /// No GUI, sound or GPU: shells and dev containers.
    Headless,
    /// No host resources at all: offline and without GUI.
    #[serde(rename = "none")]
    NoAccess,
}

impl Profile {
    pub const fn access(self) -> Access {
        let (gpu, sound, wayland, nested, network) = match self {
            Self::Full => (true, true, true, true, true),
            Self::Desktop => (false, true, true, true, true),
            Self::Headless => (false, false, false, true, true),
            Self::NoAccess => (false, false, false, false, false),
        };
        Access {
            gpu,
            sound,
            wayland,
            nested,
            network,
        }
    }
}

/// `[access]` as written in `config.toml` or `context.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wayland: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,
}

impl AccessConfig {
    /// Override the switches of `base` that are set here.
    fn apply_switches(&self, base: Access) -> Access {
        Access {
            gpu: self.gpu.unwrap_or(base.gpu),
            sound: self.sound.unwrap_or(base.sound),
            wayland: self.wayland.unwrap_or(base.wayland),
            nested: self.nested.unwrap_or(base.nested),
            network: self.network.unwrap_or(base.network),
        }
    }

    /// This config on its own: its profile (default `full`), then its switches.
    pub fn standalone(&self) -> Access {
        self.apply_switches(self.profile.unwrap_or_default().access())
    }
}

/// Effective access of a context (ADR 0017): a context's own `profile`
/// replaces the global profile *and* the global switches; otherwise the context
/// starts from the global result. The context's switches are applied last.
pub fn resolve(global: &AccessConfig, context: &AccessConfig) -> Access {
    let base = match context.profile {
        Some(profile) => profile.access(),
        None => global.standalone(),
    };
    context.apply_switches(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> AccessConfig {
        toml::from_str(text).unwrap()
    }

    const FULL: Access = Profile::Full.access();

    #[test]
    fn nothing_configured_means_full() {
        assert_eq!(
            resolve(&AccessConfig::default(), &AccessConfig::default()),
            FULL
        );
    }

    #[test]
    fn profile_table_matches_adr() {
        let row = |p: Profile| {
            let a = p.access();
            [a.gpu, a.sound, a.wayland, a.nested, a.network]
        };
        assert_eq!(row(Profile::Full), [true; 5]);
        assert_eq!(row(Profile::Desktop), [false, true, true, true, true]);
        assert_eq!(row(Profile::Headless), [false, false, false, true, true]);
        assert_eq!(row(Profile::NoAccess), [false; 5]);
    }

    #[test]
    fn profile_names_in_toml() {
        for (name, profile) in [
            ("full", Profile::Full),
            ("desktop", Profile::Desktop),
            ("headless", Profile::Headless),
            ("none", Profile::NoAccess),
        ] {
            let cfg = parse(&format!("profile = \"{name}\""));
            assert_eq!(cfg.profile, Some(profile));
            assert_eq!(
                toml::to_string(&cfg).unwrap().trim(),
                format!("profile = \"{name}\"")
            );
        }
        assert!(toml::from_str::<AccessConfig>("profile = \"paranoid\"").is_err());
        assert!(toml::from_str::<AccessConfig>("camera = true").is_err());
    }

    #[test]
    fn context_switches_override_global_result() {
        let global = parse("profile = \"desktop\"\nnetwork = false");
        let context = parse("sound = false");
        let a = resolve(&global, &context);
        assert_eq!(
            [a.gpu, a.sound, a.wayland, a.nested, a.network],
            [false, false, true, true, false]
        );
    }

    #[test]
    fn context_profile_replaces_global_profile_and_switches() {
        // The global `gpu = false` does not leak into a context that names a profile.
        let global = parse("profile = \"headless\"\ngpu = false");
        let context = parse("profile = \"full\"\nnetwork = false");
        let a = resolve(&global, &context);
        assert_eq!(
            [a.gpu, a.sound, a.wayland, a.nested, a.network],
            [true, true, true, true, false]
        );
    }

    #[test]
    fn global_switches_apply_without_context_profile() {
        let global = parse("gpu = false");
        assert!(!resolve(&global, &AccessConfig::default()).gpu);
        assert!(resolve(&global, &parse("gpu = true")).gpu);
    }

    #[test]
    fn unset_switches_are_not_written() {
        let cfg = parse("sound = false");
        assert_eq!(toml::to_string(&cfg).unwrap().trim(), "sound = false");
    }
}
