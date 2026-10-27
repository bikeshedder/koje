//! Overlays (M7.1): Hyprland special workspaces for cross-context apps, as
//! the user configures them in `config.toml` (`[overlay.<name>]`: `key`,
//! optional `label`, `apps`). Without an `[overlay]` table koje's `mail` and
//! `chat` (Super+M, Super+C) are there, as before they were configurable.
//! kojed binds their keys in Hyprland ([`binds`]); the bar shows them.

use crate::config::GlobalConfig;
use crate::error::{Error, Result};

/// koje's overlays when `config.toml` has no `[overlay]` table.
const DEFAULTS: [(&str, &str); 2] = [("chat", "SUPER+C"), ("mail", "SUPER+M")];

/// Hyprland's modifier names.
const MODIFIERS: [&str; 8] = [
    "SUPER", "SHIFT", "CTRL", "CONTROL", "ALT", "MOD3", "MOD5", "CAPS",
];

/// One overlay, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayEntry {
    /// The special workspace is `special:<name>`.
    pub name: String,
    pub key: KeyCombo,
    /// For the bar.
    pub label: String,
    /// An icon name; the bar shows the overlay by it (ADR 0055).
    pub icon: String,
}

/// A key combination as Hyprland binds it: modifiers and a key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyCombo {
    /// Upper case, in the order given.
    pub mods: Vec<String>,
    pub key: String,
}

impl KeyCombo {
    /// `SUPER+M`, `SUPER SHIFT+M`, `CTRL+ALT+F1`.
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text
            .split(['+', ' '])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let (key, mods) = parts.split_last()?;
        let mods: Vec<String> = mods.iter().map(|m| m.to_uppercase()).collect();
        if mods.iter().any(|m| !MODIFIERS.contains(&m.as_str())) {
            return None;
        }
        if key.is_empty()
            || MODIFIERS.contains(&key.to_uppercase().as_str())
            || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        let key = if key.len() == 1 {
            key.to_uppercase()
        } else {
            (*key).to_owned()
        };
        Some(Self { mods, key })
    }

    fn with_shift(&self) -> Option<Self> {
        if self.mods.iter().any(|m| m == "SHIFT") {
            return None;
        }
        let mut mods = self.mods.clone();
        mods.push("SHIFT".to_owned());
        Some(Self {
            mods,
            key: self.key.clone(),
        })
    }

    /// `SUPER SHIFT, M` – Hyprland's `bind` arguments.
    fn hyprland(&self) -> String {
        format!("{}, {}", self.mods.join(" "), self.key)
    }
}

/// The overlays of `cfg`, validated: names usable as `special:<name>` and not
/// koje's own special workspaces, keys Hyprland can bind, no key twice.
pub fn overlays(cfg: &GlobalConfig) -> Result<Vec<OverlayEntry>> {
    let invalid = |name: &str, reason: String| Error::InvalidOverlay {
        name: name.to_owned(),
        reason,
    };
    let entries: Vec<(String, String, Option<String>, Option<String>)> = match &cfg.overlay {
        None => DEFAULTS
            .iter()
            .map(|(n, k)| ((*n).to_owned(), (*k).to_owned(), None, None))
            .collect(),
        Some(map) => map
            .iter()
            .map(|(n, o)| (n.clone(), o.key.clone(), o.label.clone(), o.icon.clone()))
            .collect(),
    };
    let mut out: Vec<OverlayEntry> = Vec::new();
    for (name, key, label, icon) in entries {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(invalid(
                &name,
                "use lower-case letters, digits, - and _".to_owned(),
            ));
        }
        if name.starts_with("stash-") || name.starts_with("koje-") {
            return Err(invalid(
                &name,
                "names starting with stash- or koje- are koje's".to_owned(),
            ));
        }
        let Some(key) = KeyCombo::parse(&key) else {
            return Err(invalid(
                &name,
                format!("cannot bind key {key:?} – write it as SUPER+M"),
            ));
        };
        let label = label.unwrap_or_else(|| name.clone());
        if label.contains([',', ';', '\n']) {
            return Err(invalid(
                &name,
                "the label may not contain , or ;".to_owned(),
            ));
        }
        if let Some(other) = out.iter().find(|o| o.key == key) {
            return Err(invalid(
                &name,
                format!("its key is already the key of overlay {:?}", other.name),
            ));
        }
        crate::look::check(&format!("overlay {name:?}"), None, icon.as_deref())?;
        let icon = icon.unwrap_or_else(|| crate::look::overlay_icon(&name).to_owned());
        out.push(OverlayEntry {
            name,
            key,
            label,
            icon,
        });
    }
    Ok(out)
}

/// One Hyprland key binding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bind {
    pub key: KeyCombo,
    pub description: String,
    pub dispatcher: &'static str,
    pub arg: String,
}

impl Bind {
    /// `keyword bindd …`: bound at runtime, with the description Super+F1 lists.
    pub fn keyword(&self) -> String {
        format!(
            "keyword bindd {}, {}, {}, {}",
            self.key.hyprland(),
            self.description,
            self.dispatcher,
            self.arg
        )
    }

    pub fn unbind(&self) -> String {
        format!("keyword unbind {}", self.key.hyprland())
    }
}

/// Each overlay's key shows or hides it; with Shift added it moves the
/// focused window there (not when the key has Shift already).
pub fn binds(overlays: &[OverlayEntry]) -> Vec<Bind> {
    let mut out = Vec::new();
    for o in overlays {
        out.push(Bind {
            key: o.key.clone(),
            description: format!("Show or hide the {} overlay", o.label),
            dispatcher: "togglespecialworkspace",
            arg: o.name.clone(),
        });
        if let Some(key) = o.key.with_shift() {
            out.push(Bind {
                key,
                description: format!("Move the window to the {} overlay", o.label),
                dispatcher: "movetoworkspacesilent",
                arg: format!("special:{}", o.name),
            });
        }
    }
    out
}

/// What to send Hyprland to get from the bindings `applied` to `wanted`:
/// unbind what goes or changes, bind what is new or changed.
pub fn changes(applied: &[Bind], wanted: &[Bind]) -> Vec<String> {
    let mut out: Vec<String> = applied
        .iter()
        .filter(|b| !wanted.contains(b))
        .map(Bind::unbind)
        .collect();
    out.extend(
        wanted
            .iter()
            .filter(|b| !applied.contains(b))
            .map(Bind::keyword),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn cfg(text: &str) -> Result<GlobalConfig> {
        GlobalConfig::from_toml(text, Path::new("config.toml"))
    }

    #[test]
    fn mail_and_chat_without_a_table() {
        let o = overlays(&cfg("").unwrap()).unwrap();
        let names: Vec<&str> = o.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["chat", "mail"]);
        assert_eq!(o[1].key, KeyCombo::parse("SUPER+M").unwrap());
        // An empty table: no overlays at all.
        assert_eq!(overlays(&cfg("[overlay]\n").unwrap()).unwrap(), []);
    }

    #[test]
    fn configured_overlays() {
        let c = cfg("[overlay.music]\nkey = \"SUPER ALT+p\"\nlabel = \"Music\"\n").unwrap();
        let o = overlays(&c).unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].label, "Music");
        assert_eq!(o[0].key.mods, ["SUPER", "ALT"]);
        assert_eq!(o[0].key.key, "P");
    }

    #[test]
    fn bad_overlays_are_refused() {
        for text in [
            "[overlay.Mail]\nkey = \"SUPER+M\"\n",
            "[overlay.stash-x]\nkey = \"SUPER+M\"\n",
            "[overlay.mail]\nkey = \"HYPER+M\"\n",
            "[overlay.mail]\nkey = \"SUPER+\"\n",
            "[overlay.mail]\nkey = \"SUPER+M\"\nlabel = \"a,b\"\n",
            "[overlay.a]\nkey = \"SUPER+M\"\n[overlay.b]\nkey = \"super+m\"\n",
        ] {
            assert!(cfg(text).is_err(), "{text}");
        }
    }

    #[test]
    fn keys_as_hyprland_binds_them() {
        assert_eq!(KeyCombo::parse("CTRL+ALT+F1").unwrap().key, "F1");
        assert_eq!(KeyCombo::parse("SUPER+m").unwrap().hyprland(), "SUPER, M");
        assert_eq!(KeyCombo::parse("F12").unwrap().hyprland(), ", F12");
        assert!(KeyCombo::parse("").is_none());
    }

    #[test]
    fn binds_toggle_and_move() {
        let o = overlays(&cfg("").unwrap()).unwrap();
        let b = binds(&o);
        assert_eq!(
            b[0].keyword(),
            "keyword bindd SUPER, C, Show or hide the chat overlay, togglespecialworkspace, chat"
        );
        assert_eq!(
            b[1].keyword(),
            "keyword bindd SUPER SHIFT, C, Move the window to the chat overlay, movetoworkspacesilent, special:chat"
        );
        assert_eq!(b[1].unbind(), "keyword unbind SUPER SHIFT, C");
        // A key with Shift already has no move binding.
        let s = overlays(&cfg("[overlay.x]\nkey = \"SUPER SHIFT+X\"\n").unwrap()).unwrap();
        assert_eq!(binds(&s).len(), 1);
    }

    #[test]
    fn only_changes_are_sent() {
        let old = binds(&overlays(&cfg("").unwrap()).unwrap());
        assert_eq!(changes(&old, &old), Vec::<String>::new());
        let new = binds(&overlays(&cfg("[overlay.mail]\nkey = \"SUPER+M\"\n").unwrap()).unwrap());
        assert_eq!(
            changes(&old, &new),
            ["keyword unbind SUPER, C", "keyword unbind SUPER SHIFT, C"]
        );
        // After Hyprland reloaded its config nothing is bound: all again.
        assert_eq!(changes(&[], &new).len(), 2);
    }
}
