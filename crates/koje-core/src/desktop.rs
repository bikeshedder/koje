//! `[desktop]` in `config.toml` (ADR 0056): the look and animations koje's
//! Hyprland snippet sets – the windows' borders, gaps, corners, shadows and
//! snapping, how an open overlay darkens and blurs what is behind it and
//! zooms in, and the overview's animation. kojed applies the keys that are
//! set with `keyword`; unset ones are left to `koje.conf` and the user's
//! `hyprland.conf`. Keyboard shortcuts, input and focus are GNOME Settings'
//! (ADR 0044) and not here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// `[desktop]` as written: every key optional.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gaps_inside: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gaps_outside: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap: Option<bool>,
    /// How much an open overlay darkens what is behind it, in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_dim: Option<u32>,
    /// How much an open overlay blurs what is behind it (Hyprland's blur
    /// size); 0: no blur.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_blur: Option<u32>,
    /// Overlays zoom in when they open (M8.86) instead of sliding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_zoom: Option<bool>,
    /// How long the overview's windows fly, in ms; 0: at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview_duration: Option<u32>,
}

/// `[desktop]` with every value filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desktop {
    pub border_width: u32,
    pub gaps_inside: u32,
    pub gaps_outside: u32,
    pub corner_radius: u32,
    pub shadows: bool,
    pub snap: bool,
    pub overlay_dim: u32,
    pub overlay_blur: u32,
    pub overlay_zoom: bool,
    pub overview_duration: u32,
}

/// What `koje.conf` and koje's plugin set: the values of unset keys.
pub const DEFAULT: Desktop = Desktop {
    border_width: 3,
    gaps_inside: 4,
    gaps_outside: 8,
    corner_radius: 6,
    shadows: true,
    snap: true,
    overlay_dim: 50,
    overlay_blur: 10,
    overlay_zoom: true,
    overview_duration: 250,
};

/// The numbers' ranges: (key, smallest, largest).
pub const RANGES: [(&str, u32, u32); 7] = [
    ("border_width", 0, 20),
    ("gaps_inside", 0, 50),
    ("gaps_outside", 0, 100),
    ("corner_radius", 0, 40),
    ("overlay_dim", 0, 100),
    ("overlay_blur", 0, 40),
    ("overview_duration", 0, 2000),
];

impl Default for Desktop {
    fn default() -> Self {
        DEFAULT
    }
}

impl DesktopConfig {
    /// The values, unset ones from [`DEFAULT`].
    pub fn resolve(&self) -> Desktop {
        let d = DEFAULT;
        Desktop {
            border_width: self.border_width.unwrap_or(d.border_width),
            gaps_inside: self.gaps_inside.unwrap_or(d.gaps_inside),
            gaps_outside: self.gaps_outside.unwrap_or(d.gaps_outside),
            corner_radius: self.corner_radius.unwrap_or(d.corner_radius),
            shadows: self.shadows.unwrap_or(d.shadows),
            snap: self.snap.unwrap_or(d.snap),
            overlay_dim: self.overlay_dim.unwrap_or(d.overlay_dim),
            overlay_blur: self.overlay_blur.unwrap_or(d.overlay_blur),
            overlay_zoom: self.overlay_zoom.unwrap_or(d.overlay_zoom),
            overview_duration: self.overview_duration.unwrap_or(d.overview_duration),
        }
    }

    /// `values` as the settings app saves them: what equals the default is
    /// left unset, so `koje.conf` and `hyprland.conf` keep deciding it.
    pub fn from_values(values: &Desktop) -> Self {
        fn own<T: PartialEq + Copy>(v: T, d: T) -> Option<T> {
            (v != d).then_some(v)
        }
        let d = DEFAULT;
        Self {
            border_width: own(values.border_width, d.border_width),
            gaps_inside: own(values.gaps_inside, d.gaps_inside),
            gaps_outside: own(values.gaps_outside, d.gaps_outside),
            corner_radius: own(values.corner_radius, d.corner_radius),
            shadows: own(values.shadows, d.shadows),
            snap: own(values.snap, d.snap),
            overlay_dim: own(values.overlay_dim, d.overlay_dim),
            overlay_blur: own(values.overlay_blur, d.overlay_blur),
            overlay_zoom: own(values.overlay_zoom, d.overlay_zoom),
            overview_duration: own(values.overview_duration, d.overview_duration),
        }
    }

    /// The numbers as written, each against [`RANGES`].
    fn numbers(&self) -> [(&'static str, Option<u32>); 7] {
        [
            ("border_width", self.border_width),
            ("gaps_inside", self.gaps_inside),
            ("gaps_outside", self.gaps_outside),
            ("corner_radius", self.corner_radius),
            ("overlay_dim", self.overlay_dim),
            ("overlay_blur", self.overlay_blur),
            ("overview_duration", self.overview_duration),
        ]
    }

    /// Every number inside its range.
    pub fn check(&self) -> Result<()> {
        for ((key, value), (_, min, max)) in self.numbers().into_iter().zip(RANGES) {
            if let Some(v) = value
                && !(min..=max).contains(&v)
            {
                return Err(Error::InvalidDesktop {
                    key,
                    reason: format!("{v} is not between {min} and {max}"),
                });
            }
        }
        Ok(())
    }
}

/// Hyprland's settings for `d`: keyword → value.
pub fn keywords(d: &Desktop) -> BTreeMap<&'static str, String> {
    let flag = |b: bool| if b { "1" } else { "0" }.to_owned();
    let mut out = BTreeMap::new();
    out.insert("general:border_size", d.border_width.to_string());
    out.insert("general:gaps_in", d.gaps_inside.to_string());
    out.insert("general:gaps_out", d.gaps_outside.to_string());
    out.insert("decoration:rounding", d.corner_radius.to_string());
    out.insert("decoration:shadow:enabled", flag(d.shadows));
    out.insert("general:snap:enabled", flag(d.snap));
    out.insert(
        "decoration:dim_special",
        format!("{:.2}", f64::from(d.overlay_dim) / 100.0),
    );
    out.insert("decoration:blur:special", flag(d.overlay_blur > 0));
    if d.overlay_blur > 0 {
        out.insert("decoration:blur:size", d.overlay_blur.to_string());
    }
    out.insert("plugin:koje:overlay_zoom", flag(d.overlay_zoom));
    out.insert(
        "plugin:koje:overview_duration",
        d.overview_duration.to_string(),
    );
    out
}

/// The keywords of the keys `cfg` sets – only those are kojed's to apply.
pub fn wanted(cfg: &DesktopConfig) -> BTreeMap<&'static str, String> {
    let all = keywords(&cfg.resolve());
    let set: &[(bool, &[&str])] = &[
        (cfg.border_width.is_some(), &["general:border_size"]),
        (cfg.gaps_inside.is_some(), &["general:gaps_in"]),
        (cfg.gaps_outside.is_some(), &["general:gaps_out"]),
        (cfg.corner_radius.is_some(), &["decoration:rounding"]),
        (cfg.shadows.is_some(), &["decoration:shadow:enabled"]),
        (cfg.snap.is_some(), &["general:snap:enabled"]),
        (cfg.overlay_dim.is_some(), &["decoration:dim_special"]),
        (
            cfg.overlay_blur.is_some(),
            &["decoration:blur:special", "decoration:blur:size"],
        ),
        (cfg.overlay_zoom.is_some(), &["plugin:koje:overlay_zoom"]),
        (
            cfg.overview_duration.is_some(),
            &["plugin:koje:overview_duration"],
        ),
    ];
    all.into_iter()
        .filter(|(k, _)| set.iter().any(|(on, keys)| *on && keys.contains(k)))
        .collect()
}

/// The `keyword` lines that take Hyprland from `applied` to `wanted`: what
/// changed, and a key no longer set goes back to its default.
pub fn changes(
    applied: &BTreeMap<&'static str, String>,
    wanted: &BTreeMap<&'static str, String>,
) -> Vec<String> {
    let defaults = keywords(&DEFAULT);
    let mut out: Vec<String> = wanted
        .iter()
        .filter(|(k, v)| applied.get(*k) != Some(*v))
        .map(|(k, v)| format!("keyword {k} {v}"))
        .collect();
    out.extend(
        applied
            .keys()
            .filter(|k| !wanted.contains_key(*k))
            .filter_map(|k| defaults.get(k).map(|v| format!("keyword {k} {v}"))),
    );
    out
}

/// `config.toml`'s `text` with `[desktop]` replaced by `cfg`, comments and
/// the rest kept; no table when nothing is set.
pub fn with_desktop(text: &str, cfg: &DesktopConfig) -> Result<String> {
    cfg.check()?;
    let bad = |e: &dyn std::fmt::Display| Error::InvalidEdit(e.to_string());
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e: toml_edit::TomlError| bad(&e))?;
    let values: toml_edit::DocumentMut = toml::to_string(cfg)
        .map_err(|e| bad(&e))?
        .parse()
        .map_err(|e: toml_edit::TomlError| bad(&e))?;
    if values.is_empty() {
        doc.remove("desktop");
        return Ok(doc.to_string());
    }
    if !doc.get("desktop").is_some_and(toml_edit::Item::is_table) {
        doc["desktop"] = toml_edit::table();
    }
    if let Some(table) = doc["desktop"].as_table_mut() {
        let gone: Vec<String> = table
            .iter()
            .map(|(k, _)| k.to_owned())
            .filter(|k| !values.contains_key(k))
            .collect();
        for k in gone {
            table.remove(&k);
        }
        for (k, v) in values.iter() {
            if let Some(v) = v.as_value() {
                table[k] = toml_edit::value(v.clone());
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GlobalConfig;
    use std::path::Path;

    fn global(text: &str) -> Result<GlobalConfig> {
        GlobalConfig::from_toml(text, Path::new("config.toml"))
    }

    #[test]
    fn unset_keys_are_not_applied() {
        let cfg = global("").unwrap();
        assert!(wanted(&cfg.desktop).is_empty());
        assert_eq!(cfg.desktop.resolve(), DEFAULT);
        let cfg = global("[desktop]\noverlay_dim = 70\noverlay_blur = 0\n").unwrap();
        let w = wanted(&cfg.desktop);
        assert_eq!(w["decoration:dim_special"], "0.70");
        assert_eq!(w["decoration:blur:special"], "0");
        assert!(!w.contains_key("decoration:blur:size"));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn bad_values_are_refused() {
        for bad in [
            "[desktop]\noverlay_dim = 101\n",
            "[desktop]\nborder_width = -1\n",
            "[desktop]\nfocus = \"mouse\"\n",
        ] {
            assert!(global(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn changes_send_what_differs_and_reset_what_went() {
        let applied = wanted(&DesktopConfig {
            gaps_inside: Some(10),
            overlay_zoom: Some(false),
            ..DesktopConfig::default()
        });
        let now = wanted(&DesktopConfig {
            gaps_inside: Some(12),
            ..DesktopConfig::default()
        });
        assert_eq!(
            changes(&applied, &now),
            [
                "keyword general:gaps_in 12",
                "keyword plugin:koje:overlay_zoom 1"
            ]
        );
        assert_eq!(changes(&now, &now), Vec::<String>::new());
    }

    #[test]
    fn the_settings_app_saves_only_what_differs() {
        let values = Desktop {
            corner_radius: 0,
            ..DEFAULT
        };
        let cfg = DesktopConfig::from_values(&values);
        assert_eq!(cfg.corner_radius, Some(0));
        assert_eq!(cfg.gaps_inside, None);
        assert_eq!(cfg.resolve(), values);

        let text = "# mine\n[desktop]\n# round\ncorner_radius = 12\nsnap = false\n\n[bar]\nworkspaces = \"dots\"\n";
        let out = with_desktop(text, &cfg).unwrap();
        assert!(
            out.starts_with("# mine\n[desktop]\n# round\ncorner_radius = 0\n"),
            "{out}"
        );
        assert!(!out.contains("snap"), "{out}");
        assert_eq!(global(&out).unwrap().desktop, cfg);
        let out = with_desktop(&out, &DesktopConfig::default()).unwrap();
        assert!(!out.contains("[desktop]"), "{out}");
        assert!(out.contains("workspaces = \"dots\""), "{out}");
    }
}
