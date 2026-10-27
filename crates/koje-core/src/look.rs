//! Icons and labels (M8.89, ADR 0055): every workspace, context and overlay
//! has an icon and a label for the bar and the settings app. Host
//! workspaces take theirs from `[workspace.<number>]` in `config.toml`,
//! contexts from `label` and `icon` in `context.toml`, overlays from their
//! `[overlay.<name>]`. Unset values have defaults, so a fresh install shows
//! something useful.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{ContextConfig, GlobalConfig, WorkspaceStyle};
use crate::error::{Error, Result};

/// A host workspace's icon when none is set.
pub const HOST_ICON: &str = "computer-symbolic";
/// A context's icon when none is set.
pub const CONTEXT_ICON: &str = "folder-symbolic";
/// An overlay's icon when none is set (koje's own overlays have their own).
pub const OVERLAY_ICON: &str = "view-dual-symbolic";

/// `[workspace.<number>]` in `config.toml`: a host workspace's look.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostWorkspace {
    /// The name shown in the bar; the number when unset.
    #[serde(default)]
    pub label: Option<String>,
    /// An icon name from the icon theme, e.g. `web-browser-symbolic`.
    #[serde(default)]
    pub icon: Option<String>,
}

/// What the bar shows for a workspace, context or overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub label: String,
    pub icon: String,
}

/// An icon theme name: letters, digits, `-`, `_` and `.`, no path.
pub fn valid_icon(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A label and an icon as written, checked; `what` names the entry.
pub fn check(what: &str, label: Option<&str>, icon: Option<&str>) -> Result<()> {
    let bad = |reason: String| Error::InvalidLook {
        what: what.to_owned(),
        reason,
    };
    if let Some(l) = label
        && l.trim().is_empty()
    {
        return Err(bad("the label is empty".to_owned()));
    }
    if let Some(i) = icon
        && !valid_icon(i)
    {
        return Err(bad(format!("{i:?} is no icon name (like folder-symbolic)")));
    }
    Ok(())
}

/// The host workspaces of `config.toml`, checked: numbered from 1.
pub fn host_workspaces(cfg: &GlobalConfig) -> Result<BTreeMap<u32, HostWorkspace>> {
    let mut out = BTreeMap::new();
    for (key, w) in &cfg.workspace {
        let what = format!("workspace {key:?}");
        let number =
            key.parse::<u32>()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| Error::InvalidLook {
                    what: what.clone(),
                    reason: "host workspaces are numbered from 1 ([workspace.1])".to_owned(),
                })?;
        check(&what, w.label.as_deref(), w.icon.as_deref())?;
        out.insert(number, w.clone());
    }
    Ok(out)
}

/// The look of host workspace `id` (a Hyprland workspace number); a
/// workspace with a name but no number (`id` ≤ 0) shows its name.
pub fn host(cfg: &GlobalConfig, id: i64, name: &str) -> Look {
    let own = u32::try_from(id)
        .ok()
        .and_then(|n| cfg.workspace.get(&n.to_string()));
    Look {
        label: own
            .and_then(|w| w.label.clone())
            .unwrap_or_else(|| name.to_owned()),
        icon: own
            .and_then(|w| w.icon.clone())
            .unwrap_or_else(|| HOST_ICON.to_owned()),
    }
}

/// A context's look: its label, else its name.
pub fn context(cfg: &ContextConfig) -> Look {
    Look {
        label: cfg.label.clone().unwrap_or_else(|| cfg.name.to_string()),
        icon: cfg.icon.clone().unwrap_or_else(|| CONTEXT_ICON.to_owned()),
    }
}

/// An overlay's icon when none is set.
pub fn overlay_icon(name: &str) -> &'static str {
    match name {
        "mail" => "mail-unread-symbolic",
        "chat" => "chat-message-new-symbolic",
        _ => OVERLAY_ICON,
    }
}

/// One overlay as the settings app edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySettings {
    pub name: String,
    pub key: String,
    pub label: Option<String>,
    pub icon: Option<String>,
}

/// What the settings app's Workspaces page edits in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarSettings {
    pub style: WorkspaceStyle,
    pub workspaces: BTreeMap<u32, HostWorkspace>,
    /// `None` leaves `[overlay]` as it is – also absent, with koje's own.
    pub overlays: Option<Vec<OverlaySettings>>,
}

/// `config.toml`'s `text` with `s` applied, comments and layout kept; an
/// overlay keeps its `apps`.
pub fn with_bar(text: &str, s: &BarSettings) -> Result<String> {
    let bad = |e: &dyn std::fmt::Display| Error::InvalidEdit(e.to_string());
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e: toml_edit::TomlError| bad(&e))?;
    let style = toml::Value::try_from(s.style)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let has_style = doc.get("bar").and_then(|b| b.get("workspaces")).is_some();
    if has_style || s.style != WorkspaceStyle::default() {
        if !doc.contains_key("bar") {
            doc["bar"] = toml_edit::table();
        }
        doc["bar"]["workspaces"] = toml_edit::value(style);
    }

    let set = |t: &mut toml_edit::Table, key: &str, v: Option<&String>| match v {
        Some(v) => {
            t[key] = toml_edit::value(v.as_str());
        }
        None => {
            t.remove(key);
        }
    };
    let mut workspaces = toml_edit::Table::new();
    workspaces.set_implicit(true);
    let old = doc.get("workspace").and_then(toml_edit::Item::as_table);
    for (n, w) in &s.workspaces {
        let key = n.to_string();
        let mut t = old
            .and_then(|o| o.get(&key))
            .and_then(toml_edit::Item::as_table)
            .cloned()
            .unwrap_or_default();
        set(&mut t, "label", w.label.as_ref());
        set(&mut t, "icon", w.icon.as_ref());
        workspaces.insert(&key, toml_edit::Item::Table(t));
    }
    if s.workspaces.is_empty() {
        doc.remove("workspace");
    } else {
        doc["workspace"] = toml_edit::Item::Table(workspaces);
    }

    if let Some(list) = &s.overlays {
        let old = doc.get("overlay").and_then(toml_edit::Item::as_table);
        let mut overlays = toml_edit::Table::new();
        overlays.set_implicit(true);
        for o in list {
            let mut t = old
                .and_then(|t| t.get(&o.name))
                .and_then(toml_edit::Item::as_table)
                .cloned()
                .unwrap_or_default();
            t["key"] = toml_edit::value(o.key.as_str());
            set(&mut t, "label", o.label.as_ref());
            set(&mut t, "icon", o.icon.as_ref());
            overlays.insert(&o.name, toml_edit::Item::Table(t));
        }
        doc["overlay"] = toml_edit::Item::Table(overlays);
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn global(text: &str) -> Result<GlobalConfig> {
        GlobalConfig::from_toml(text, Path::new("config.toml"))
    }

    #[test]
    fn host_workspaces_have_defaults() {
        let cfg =
            global("[workspace.2]\nlabel = \"Web\"\nicon = \"web-browser-symbolic\"\n").unwrap();
        assert_eq!(
            host(&cfg, 2, "2"),
            Look {
                label: "Web".into(),
                icon: "web-browser-symbolic".into()
            }
        );
        assert_eq!(
            host(&cfg, 1, "1"),
            Look {
                label: "1".into(),
                icon: HOST_ICON.into()
            }
        );
        assert_eq!(host(&cfg, -98, "spare").label, "spare");
    }

    #[test]
    fn host_workspaces_are_checked() {
        for bad in [
            "[workspace.0]\nlabel = \"x\"\n",
            "[workspace.web]\nlabel = \"x\"\n",
            "[workspace.1]\nlabel = \" \"\n",
            "[workspace.1]\nicon = \"/tmp/x.svg\"\n",
            "[workspace.1]\ncolor = \"red\"\n",
        ] {
            assert!(global(bad).is_err(), "{bad}");
        }
        let cfg = global("[workspace.1]\n[workspace.10]\nicon = \"a.b_c-d\"\n").unwrap();
        assert_eq!(
            host_workspaces(&cfg)
                .unwrap()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [1, 10]
        );
    }

    #[test]
    fn contexts_default_to_their_name() {
        let base = "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#2E7D5B\"\n";
        let cfg = ContextConfig::from_toml(base, Path::new("context.toml")).unwrap();
        assert_eq!(
            context(&cfg),
            Look {
                label: "kp".into(),
                icon: CONTEXT_ICON.into()
            }
        );
        let own = format!("{base}label = \"Kundenportal\"\nicon = \"briefcase-symbolic\"\n");
        let cfg = ContextConfig::from_toml(&own, Path::new("context.toml")).unwrap();
        assert_eq!(context(&cfg).label, "Kundenportal");
        assert_eq!(context(&cfg).icon, "briefcase-symbolic");
        let bad = format!("{base}icon = \"../x\"\n");
        assert!(ContextConfig::from_toml(&bad, Path::new("context.toml")).is_err());
    }

    #[test]
    fn bar_settings_keep_the_rest() {
        let text = "# mine\n[access]\nprofile = \"desktop\"\n\n[overlay.mail]\nkey = \"SUPER+M\"\napps = [\"thunderbird\"]\n\n[workspace.3]\nlabel = \"Old\"\n";
        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            1,
            HostWorkspace {
                label: Some("Web".into()),
                icon: Some("web-browser-symbolic".into()),
            },
        );
        workspaces.insert(2, HostWorkspace::default());
        let s = BarSettings {
            style: WorkspaceStyle::Labels,
            workspaces,
            overlays: Some(vec![OverlaySettings {
                name: "mail".into(),
                key: "SUPER+E".into(),
                label: Some("Mail".into()),
                icon: None,
            }]),
        };
        let out = with_bar(text, &s).unwrap();
        assert!(out.starts_with("# mine\n[access]"), "{out}");
        assert!(
            !out.contains("[bar]"),
            "the default style is not written: {out}"
        );
        assert!(!out.contains("Old"), "{out}");
        let cfg = global(&out).unwrap();
        assert_eq!(host(&cfg, 1, "1").label, "Web");
        assert_eq!(host(&cfg, 2, "2").icon, HOST_ICON);
        let mail = &cfg.overlay.as_ref().unwrap()["mail"];
        assert_eq!(mail.key, "SUPER+E");
        assert_eq!(mail.apps, ["thunderbird"]);
        assert_eq!(mail.label.as_deref(), Some("Mail"));

        let dots = BarSettings {
            style: WorkspaceStyle::ColoredDots,
            workspaces: BTreeMap::new(),
            overlays: None,
        };
        let out = with_bar(&out, &dots).unwrap();
        let cfg = global(&out).unwrap();
        assert_eq!(cfg.bar.workspaces, WorkspaceStyle::ColoredDots);
        assert!(cfg.workspace.is_empty());
        assert_eq!(cfg.overlay.unwrap()["mail"].key, "SUPER+E");
    }
}
