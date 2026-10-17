//! Access profiles: which host resources a context gets
//! (`docs/decisions/0017-access-profiles.md`, host interfaces: ADR 0019).
//!
//! `[access]` in `config.toml` sets the default, `[access]` in `context.toml`
//! adjusts it: a profile sets all switches, single switches override it.

use serde::{Deserialize, Serialize};

/// Declares the switches once and generates [`Access`] (the result),
/// [`AccessConfig`] (as written in the files) and the override step.
macro_rules! switches {
    ($($(#[doc = $doc:literal])* $name:ident),* $(,)?) => {
        /// The host resources a context gets, after resolving profiles and switches.
        #[allow(clippy::struct_excessive_bools)] // independent switches, one per resource
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        pub struct Access {
            $($(#[doc = $doc])* pub $name: bool,)*
        }

        impl Access {
            /// Every switch on.
            pub const ALL: Self = Self { $($name: true,)* };
            /// Every switch off.
            pub const NONE: Self = Self { $($name: false,)* };

            /// `(name, on)` for every switch, in declaration order (as in the files).
            pub fn switches(&self) -> Vec<(&'static str, bool)> {
                vec![$((stringify!($name), self.$name),)*]
            }
        }

        /// `[access]` as written in `config.toml` or `context.toml`.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct AccessConfig {
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub profile: Option<Profile>,
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $name: Option<bool>,
            )*
        }

        impl AccessConfig {
            /// Override the switches of `base` that are set here.
            fn apply_switches(&self, base: Access) -> Access {
                Access { $($name: self.$name.unwrap_or(base.$name),)* }
            }
        }
    };
}

switches! {
    /// `/dev/dri` and NVIDIA via CDI (ADR 0016).
    gpu,
    /// `PipeWire` and `PulseAudio` sockets (playback and microphone).
    sound,
    /// The compositor's Wayland socket; without it the context has no GUI.
    wayland,
    /// Devices and `/proc/sys` for nested Podman (ADR 0014).
    nested,
    /// An own network; without it `--network none`.
    network,
    /// Desktop notifications on the host (ADR 0019).
    notifications,
    /// The host's file chooser portal.
    file_chooser,
    /// The host's appearance settings, read-only (dark mode, fonts).
    settings,
    /// May prevent screen lock and suspend.
    inhibit,
    /// Screen sharing through the host's portal (it asks each time).
    screencast,
    /// The host's print dialog.
    print,
}

/// Named sets of switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Everything (the default).
    #[default]
    Full,
    /// Everything but the GPU.
    Desktop,
    /// No GUI, sound or GPU: shells and dev containers; notifications stay.
    Headless,
    /// No host resources at all: offline and without GUI.
    Isolated,
}

impl Profile {
    /// The profile table of ADR 0017 / ADR 0019.
    pub const fn access(self) -> Access {
        match self {
            Self::Full => Access::ALL,
            Self::Desktop => Access {
                gpu: false,
                ..Access::ALL
            },
            Self::Headless => Access {
                nested: true,
                network: true,
                notifications: true,
                ..Access::NONE
            },
            Self::Isolated => Access::NONE,
        }
    }
}

impl AccessConfig {
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

    /// The switches that are on, by name.
    fn on(a: Access) -> Vec<&'static str> {
        a.switches()
            .into_iter()
            .filter(|(_, v)| *v)
            .map(|(n, _)| n)
            .collect()
    }

    #[test]
    fn nothing_configured_means_full() {
        assert_eq!(
            resolve(&AccessConfig::default(), &AccessConfig::default()),
            Access::ALL
        );
    }

    #[test]
    fn profile_table_matches_adr() {
        assert_eq!(on(Profile::Full.access()).len(), 11);
        assert_eq!(
            on(Profile::Desktop.access()),
            [
                "sound",
                "wayland",
                "nested",
                "network",
                "notifications",
                "file_chooser",
                "settings",
                "inhibit",
                "screencast",
                "print"
            ]
        );
        assert_eq!(
            on(Profile::Headless.access()),
            ["nested", "network", "notifications"]
        );
        assert_eq!(on(Profile::Isolated.access()), Vec::<&str>::new());
    }

    #[test]
    fn profile_names_in_toml() {
        for (name, profile) in [
            ("full", Profile::Full),
            ("desktop", Profile::Desktop),
            ("headless", Profile::Headless),
            ("isolated", Profile::Isolated),
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
    fn every_switch_is_a_toml_key() {
        for (name, _) in Access::ALL.switches() {
            let cfg = parse(&format!("{name} = false"));
            assert!(!on(cfg.standalone()).contains(&name), "{name}");
            assert_eq!(on(cfg.standalone()).len(), 10, "{name}");
        }
    }

    #[test]
    fn context_switches_override_global_result() {
        let global = parse("profile = \"desktop\"\nnetwork = false");
        let context = parse("sound = false\nscreencast = false");
        let a = resolve(&global, &context);
        assert!(!a.gpu && !a.sound && !a.network && !a.screencast);
        assert!(a.wayland && a.nested && a.file_chooser && a.print);
    }

    #[test]
    fn context_profile_replaces_global_profile_and_switches() {
        // The global `gpu = false` does not leak into a context that names a profile.
        let global = parse("profile = \"headless\"\ngpu = false");
        let context = parse("profile = \"full\"\nnetwork = false");
        let a = resolve(&global, &context);
        assert_eq!(
            a,
            Access {
                network: false,
                ..Access::ALL
            }
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
        let cfg = parse("file_chooser = false");
        assert_eq!(
            toml::to_string(&cfg).unwrap().trim(),
            "file_chooser = false"
        );
    }
}
