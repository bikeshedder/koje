//! Contexts on disk: list, load, create and remove their configuration.
//!
//! Defaults for new contexts follow `docs/decisions/0012-new-context-defaults.md`.

use std::path::PathBuf;

use crate::color::Color;
use crate::config::ContextConfig;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::name::ContextName;
use crate::paths::Paths;
use crate::size::ByteSize;

/// Colors `koje new` picks from, in order (ADR 0012).
pub const PALETTE: [Color; 10] = [
    Color::rgb(0x2E, 0x7D, 0x5B), // green
    Color::rgb(0x1E, 0x88, 0xE5), // blue
    Color::rgb(0xE5, 0x39, 0x35), // red
    Color::rgb(0xFB, 0x8C, 0x00), // orange
    Color::rgb(0x8E, 0x24, 0xAA), // purple
    Color::rgb(0x00, 0xAC, 0xC1), // cyan
    Color::rgb(0xFD, 0xD8, 0x35), // yellow
    Color::rgb(0xD8, 0x1B, 0x60), // pink
    Color::rgb(0x6D, 0x4C, 0x41), // brown
    Color::rgb(0x54, 0x6E, 0x7A), // blue-grey
];

/// Initials of the `-`-separated parts, or the first two letters of a single
/// word, uppercase: `kunden-portal` → `KP`, `kundenportal` → `KU`.
pub fn default_prefix(name: &ContextName) -> String {
    let parts: Vec<&str> = name.as_str().split('-').filter(|p| !p.is_empty()).collect();
    let prefix: String = if parts.len() >= 2 {
        parts.iter().filter_map(|p| p.chars().next()).collect()
    } else {
        name.as_str().chars().take(2).collect()
    };
    prefix.to_ascii_uppercase()
}

/// The first palette color no context uses; if all are used, the least used one.
pub fn pick_color(used: &[Color]) -> Color {
    let count = |c: &Color| used.iter().filter(|u| *u == c).count();
    // `min_by_key` keeps the first of equal elements, so palette order breaks ties.
    PALETTE
        .iter()
        .copied()
        .min_by_key(count)
        .unwrap_or(PALETTE[0])
}

/// Contents of a new `context.toml`: the given values plus commented hints for
/// the optional keys, so the file is easy to edit by hand.
pub fn new_config_toml(name: &ContextName, prefix: &str, color: Color, image: &str) -> String {
    let image = toml::Value::String(image.to_owned());
    let prefix = toml::Value::String(prefix.to_owned());
    format!(
        r#"name   = "{name}"
prefix = {prefix}
color  = "{color}"
# The image (ADR 0040, 0067): localhost/koje/<name>:<version> – koje's own
# (fedora:45, ubuntu:26.04, debian:13) or a folder in ~/.config/koje/images –,
# a folder's path (~/…), or a registry image (docker.io/…).
image  = {image}

# Host folders bound into the context at the same path (ADR 0009).
mounts = []

# Dotfiles presets from ~/.config/koje/dotfiles/<name> (ADR 0038): mounted
# read-only and linked into the home, later presets win.
dotfiles = []

# [limits]                # unset values come from config.toml
# memory_soft = "6G"      # warn at 90 % and 100 %
# memory_hard = "8G"      # orderly shutdown

# [access]                # host resources, ADR 0017 (default from config.toml)
# profile = "full"        # full | desktop | headless | isolated
# gpu     = false         # override single switches (gpu, sound, wayland, nested, network,
#                         # notifications, file_chooser, settings, inhibit, screencast, print)

# [states]                # rename the roles
# default = "default"
# fresh   = "fresh"
"#
    )
}

/// Load a context's configuration. The `name` inside must match the directory.
pub fn load(paths: &Paths, name: &ContextName) -> Result<ContextConfig> {
    let path = paths.context_config(name);
    if !path.is_file() {
        return Err(Error::UnknownContext(name.clone()));
    }
    let cfg = ContextConfig::load(&path)?;
    if cfg.name != *name {
        return Err(Error::ContextNameMismatch {
            path,
            expected: name.clone(),
            found: cfg.name,
        });
    }
    Ok(cfg)
}

/// All contexts with a `context.toml`, sorted by name. Directories that are not
/// valid context names are skipped.
pub fn list(paths: &Paths) -> Result<Vec<ContextConfig>> {
    let mut configs = Vec::new();
    for name in names(paths)? {
        configs.push(load(paths, &name)?);
    }
    Ok(configs)
}

/// Names of all contexts with a `context.toml`, sorted.
pub fn names(paths: &Paths) -> Result<Vec<ContextName>> {
    let dir = paths.contexts_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Read { path: dir, source }),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Read {
            path: dir.clone(),
            source,
        })?;
        let Some(name) = entry
            .file_name()
            .to_str()
            .and_then(|n| ContextName::new(n).ok())
        else {
            continue;
        };
        if paths.context_config(&name).is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

pub fn exists(paths: &Paths, name: &ContextName) -> bool {
    paths.context_config(name).exists()
}

/// Create a context's `context.toml`. Refuses an existing context.
/// `prefix` and `color` default per ADR 0012.
pub fn create(
    paths: &Paths,
    name: &ContextName,
    prefix: Option<String>,
    color: Option<Color>,
    image: &str,
) -> Result<ContextConfig> {
    if exists(paths, name) {
        return Err(Error::ContextExists(name.clone()));
    }
    let prefix = prefix.unwrap_or_else(|| default_prefix(name));
    let color = match color {
        Some(c) => c,
        None => pick_color(&list(paths)?.iter().map(|c| c.color).collect::<Vec<_>>()),
    };
    let path = paths.context_config(name);
    let text = new_config_toml(name, &prefix, color, image);
    let cfg = ContextConfig::from_toml(&text, &path)?;
    fsutil::write_atomic(&path, text.as_bytes())?;
    Ok(cfg)
}

/// A change of one limit (`koje limit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitChange {
    Keep,
    Set(ByteSize),
    /// Remove it from `context.toml` (then `config.toml`'s default applies).
    Clear,
}

/// `text` with the limits changed, comments and layout kept.
pub fn with_limits(text: &str, soft: LimitChange, hard: LimitChange) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidLimits(e.to_string()))?;
    if !doc.contains_key("limits") {
        doc["limits"] = toml_edit::table();
    }
    let limits = doc["limits"]
        .as_table_mut()
        .ok_or_else(|| Error::InvalidLimits("`limits` is not a table".into()))?;
    for (key, change) in [("memory_soft", soft), ("memory_hard", hard)] {
        match change {
            LimitChange::Keep => {}
            LimitChange::Set(size) => limits[key] = toml_edit::value(size.to_string()),
            LimitChange::Clear => {
                limits.remove(key);
            }
        }
    }
    Ok(doc.to_string())
}

/// `text` with the dash's favorites set – `None` removes them (GNOME's
/// apply again) –, comments and layout kept.
pub fn with_favorites(text: &str, favorites: Option<&[String]>) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidFavorites(e.to_string()))?;
    match favorites {
        Some(ids) => {
            if let Some(bad) = ids
                .iter()
                .find(|id| !id.ends_with(".desktop") || id.contains('/'))
            {
                return Err(Error::InvalidFavorites(format!(
                    "{bad:?} is no desktop file id (like firefox.desktop)"
                )));
            }
            let new = !doc.contains_key("favorites");
            // At the top, before the tables.
            doc.insert(
                "favorites",
                toml_edit::value(ids.iter().collect::<toml_edit::Array>()),
            );
            if new && let Some(mut key) = doc.key_mut("favorites") {
                key.leaf_decor_mut()
                    .set_prefix("# The dash's favorites, pinned from it (ADR 0053).\n");
            }
        }
        None => {
            doc.remove("favorites");
        }
    }
    Ok(doc.to_string())
}

/// What the settings app's context page edits (M8.10). Switches are
/// overrides of the profile: `None` takes the profile's value. Mounts are
/// paths as written; one marked `unsafe` keeps its mark while it stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSettings {
    pub prefix: String,
    /// For the bar (ADR 0055); `None`: the name.
    pub label: Option<String>,
    /// `None`: koje's default icon.
    pub icon: Option<String>,
    pub color: Color,
    /// `image = …` (ADR 0040).
    pub image: String,
    pub profile: Option<crate::access::Profile>,
    pub switches: Vec<(&'static str, Option<bool>)>,
    pub mounts: Vec<String>,
    pub memory_soft: Option<ByteSize>,
    pub memory_hard: Option<ByteSize>,
}

impl ContextSettings {
    /// What `cfg` sets now.
    pub fn of(cfg: &ContextConfig) -> Self {
        let set = cfg.access.overrides();
        Self {
            prefix: cfg.prefix.clone(),
            label: cfg.label.clone(),
            icon: cfg.icon.clone(),
            color: cfg.color,
            image: cfg.image.clone(),
            profile: cfg.access.profile,
            switches: crate::access::Access::ALL
                .switches()
                .into_iter()
                .map(|(name, _)| (name, set.get(name).copied()))
                .collect(),
            mounts: cfg.mounts.iter().map(|m| m.path.to_string()).collect(),
            memory_soft: cfg.limits.memory_soft,
            memory_hard: cfg.limits.memory_hard,
        }
    }
}

/// `text` with the settings applied, comments and layout kept.
pub fn with_settings(text: &str, s: &ContextSettings) -> Result<String> {
    let bad = |e: &dyn std::fmt::Display| Error::InvalidEdit(e.to_string());
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e: toml_edit::TomlError| bad(&e))?;
    doc["prefix"] = toml_edit::value(s.prefix.clone());
    for (key, value) in [("label", &s.label), ("icon", &s.icon)] {
        match value {
            // Next to the prefix, before the tables.
            Some(v) => {
                doc.insert(key, toml_edit::value(v.as_str()));
            }
            None => {
                doc.remove(key);
            }
        }
    }
    doc["color"] = toml_edit::value(s.color.to_string());
    // Unset is the default image: written only when it changes.
    let image = doc
        .get("image")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or(crate::image::DEFAULT);
    if image != s.image {
        doc = with_image(&doc.to_string(), &s.image)?
            .parse()
            .map_err(|e: toml_edit::TomlError| bad(&e))?;
    }

    // Mounts: an `unsafe` table entry stays a table while its path stays.
    let old: Vec<toml_edit::Value> = doc
        .get("mounts")
        .and_then(toml_edit::Item::as_array)
        .map(|a| a.iter().cloned().collect())
        .unwrap_or_default();
    let mut mounts = toml_edit::Array::new();
    for path in &s.mounts {
        let kept = old.iter().find(|v| {
            v.as_inline_table()
                .and_then(|t| t.get("path"))
                .and_then(toml_edit::Value::as_str)
                == Some(path.as_str())
        });
        match kept {
            Some(table) => mounts.push_formatted(table.clone().decorated(" ", "")),
            None => mounts.push(path.as_str()),
        }
    }
    doc["mounts"] = toml_edit::value(mounts);

    let table = |doc: &mut toml_edit::DocumentMut, key: &str| -> Result<()> {
        if !doc.contains_key(key) {
            doc[key] = toml_edit::table();
        }
        doc[key]
            .as_table_mut()
            .map(|_| ())
            .ok_or_else(|| Error::InvalidEdit(format!("`{key}` is not a table")))
    };
    table(&mut doc, "limits")?;
    for (key, size) in [
        ("memory_soft", s.memory_soft),
        ("memory_hard", s.memory_hard),
    ] {
        match size {
            Some(size) => doc["limits"][key] = toml_edit::value(size.to_string()),
            None => {
                if let Some(t) = doc["limits"].as_table_mut() {
                    t.remove(key);
                }
            }
        }
    }
    table(&mut doc, "access")?;
    match s.profile {
        Some(p) => doc["access"]["profile"] = toml_edit::value(p.as_str()),
        None => {
            if let Some(t) = doc["access"].as_table_mut() {
                t.remove("profile");
            }
        }
    }
    for (name, on) in &s.switches {
        match on {
            Some(on) => doc["access"][name] = toml_edit::value(*on),
            None => {
                if let Some(t) = doc["access"].as_table_mut() {
                    t.remove(name);
                }
            }
        }
    }
    // An empty table is left out, as in a new context.toml.
    for key in ["limits", "access"] {
        if doc[key].as_table().is_some_and(toml_edit::Table::is_empty) {
            doc.remove(key);
        }
    }
    Ok(doc.to_string())
}

/// `text` with `image = value`, comments and layout kept.
pub fn with_image(text: &str, value: &str) -> Result<String> {
    crate::image::parse(value)?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidEdit(e.to_string()))?;
    match doc.get_mut("image").and_then(toml_edit::Item::as_value_mut) {
        // In place: the comments around it stay.
        Some(v) => {
            let decor = v.decor().clone();
            *v = toml_edit::Value::from(value);
            *v.decor_mut() = decor;
        }
        None => {
            doc.insert("image", toml_edit::value(value));
        }
    }
    Ok(doc.to_string())
}

/// Set a context's image (ADR 0063) in `context.toml` (validated, atomic).
pub fn set_image(paths: &Paths, name: &ContextName, value: &str) -> Result<ContextConfig> {
    let path = paths.context_config(name);
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Read {
        path: path.clone(),
        source,
    })?;
    replace_config(paths, name, &with_image(&text, value)?)
}

/// `koje favorites`: change the dash's favorites in `context.toml`.
pub fn set_favorites(
    paths: &Paths,
    name: &ContextName,
    favorites: Option<&[String]>,
) -> Result<ContextConfig> {
    let path = paths.context_config(name);
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Read {
        path: path.clone(),
        source,
    })?;
    replace_config(paths, name, &with_favorites(&text, favorites)?)
}

/// `koje limit`: change the limits in `context.toml` (validated, atomic).
pub fn set_limits(
    paths: &Paths,
    name: &ContextName,
    soft: LimitChange,
    hard: LimitChange,
) -> Result<ContextConfig> {
    let path = paths.context_config(name);
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Read {
        path: path.clone(),
        source,
    })?;
    replace_config(paths, name, &with_limits(&text, soft, hard)?)
}

/// Check an edited `context.toml` for `name` completely: syntax and values,
/// the name matching the directory, and the mount rules (ADR 0009).
pub fn validate(paths: &Paths, name: &ContextName, text: &str) -> Result<ContextConfig> {
    let path = paths.context_config(name);
    let cfg = ContextConfig::from_toml(text, &path)?;
    if cfg.name != *name {
        return Err(Error::ContextNameMismatch {
            path,
            expected: name.clone(),
            found: cfg.name,
        });
    }
    cfg.expand_mounts(&paths.host_dirs()?)?;
    Ok(cfg)
}

/// Replace a context's `context.toml` after [`validate`] accepted `text`.
pub fn replace_config(paths: &Paths, name: &ContextName, text: &str) -> Result<ContextConfig> {
    let cfg = validate(paths, name, text)?;
    fsutil::write_atomic(&paths.context_config(name), text.as_bytes())?;
    Ok(cfg)
}

/// Directories koje keeps for a context: config, states, recovery.
/// [`remove`] deletes them; the ones that exist are what `koje rm` lists.
pub fn dirs(paths: &Paths, name: &ContextName) -> Vec<PathBuf> {
    vec![
        paths.context_dir(name),
        paths.context_data_dir(name),
        paths.context_state_dir(name),
    ]
}

/// Delete everything koje stores for a context. Containers and volumes are
/// removed separately (M3.8).
pub fn remove(paths: &Paths, name: &ContextName) -> Result<()> {
    if !exists(paths, name) {
        return Err(Error::UnknownContext(name.clone()));
    }
    // Config last: if anything fails, the context still exists and `rm` can be retried.
    for dir in dirs(paths, name).into_iter().rev() {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(Error::Write { path: dir, source }),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> ContextName {
        ContextName::new(s).unwrap()
    }

    fn temp_paths(tmp: &tempfile::TempDir) -> Paths {
        let r = tmp.path();
        Paths::with_roots(r.join("c"), r.join("d"), r.join("s"))
    }

    #[test]
    fn settings_of_a_config_round_trip() {
        let text = "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#2E7D5B\"\nmounts = [\"~/src\"]\n\n[access]\nsound = false\n\n[limits]\nmemory_soft = \"4G\"\n";
        let cfg = ContextConfig::from_toml(text, std::path::Path::new("context.toml")).unwrap();
        let s = ContextSettings::of(&cfg);
        assert_eq!(s.prefix, "KP");
        assert_eq!(s.mounts, ["~/src"]);
        assert_eq!(s.memory_soft, Some(ByteSize(4 << 30)));
        assert!(s.switches.contains(&("sound", Some(false))));
        assert!(s.switches.contains(&("gpu", None)));
        // Unchanged settings write the same file.
        assert_eq!(with_settings(text, &s).unwrap(), text);
    }

    #[test]
    fn prefix_rule() {
        for (name, prefix) in [
            ("kunden-portal", "KP"),
            ("kundenportal", "KU"),
            ("x", "X"),
            ("my-big-project", "MBP"),
            ("a1", "A1"),
            ("shop--relaunch", "SR"),
        ] {
            assert_eq!(default_prefix(&n(name)), prefix, "{name}");
        }
    }

    #[test]
    fn color_rule() {
        assert_eq!(pick_color(&[]), PALETTE[0]);
        assert_eq!(pick_color(&[PALETTE[0], PALETTE[2]]), PALETTE[1]);
        let mut all = PALETTE.to_vec();
        assert_eq!(pick_color(&all), PALETTE[0]);
        all.push(PALETTE[0]);
        assert_eq!(pick_color(&all), PALETTE[1]);
    }

    #[test]
    fn image_is_changed_in_place() {
        let text = new_config_toml(&n("kp"), "KP", PALETTE[1], "localhost/koje/fedora:45");
        let changed = with_image(&text, "kp").unwrap();
        let cfg = ContextConfig::from_toml(&changed, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.image, "kp");
        assert!(
            changed.contains("# The image (ADR 0040, 0067)"),
            "{changed}"
        );
        assert!(changed.contains("image  = \"kp\""), "{changed}");
        assert_eq!(changed.lines().count(), text.lines().count());
        let added = with_image("name = \"kp\"\n", "dev").unwrap();
        assert!(added.contains("image = \"dev\""), "{added}");
        assert!(with_image(&text, " ").is_err());
    }

    #[test]
    fn template_parses_and_escapes_prefix() {
        let text = new_config_toml(&n("kp"), "K\"P", PALETTE[1], "localhost/koje/fedora:45");
        let cfg = ContextConfig::from_toml(&text, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.prefix, "K\"P");
        assert_eq!(cfg.color, PALETTE[1]);
        assert_eq!(cfg.image, "localhost/koje/fedora:45");
        assert_eq!(cfg.mounts, []);
        assert_eq!(cfg.access, crate::access::AccessConfig::default());
        // The commented [access] example is valid once uncommented.
        let uncommented = text
            .replace("# [access]", "[access]")
            .replace("# profile = \"full\"", "profile = \"full\"")
            .replace("# gpu     = false", "gpu     = false")
            .replace("# [states]", "[states]")
            .replace("# default = \"default\"", "default = \"default\"")
            .replace("# fresh   = \"fresh\"", "fresh   = \"fresh\"");
        let cfg = ContextConfig::from_toml(&uncommented, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.access.profile, Some(crate::access::Profile::Full));
    }

    #[test]
    fn create_list_and_refuse_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        let a = create(
            &paths,
            &n("kunden-portal"),
            None,
            None,
            "localhost/koje/fedora:45",
        )
        .unwrap();
        assert_eq!((a.prefix.as_str(), a.color), ("KP", PALETTE[0]));
        let b = create(
            &paths,
            &n("shop"),
            Some("SH".into()),
            None,
            "localhost/koje/fedora:45",
        )
        .unwrap();
        assert_eq!(b.color, PALETTE[1]);
        assert!(matches!(
            create(&paths, &n("shop"), None, None, "localhost/koje/fedora:45"),
            Err(Error::ContextExists(_))
        ));
        let names: Vec<_> = list(&paths).unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, [n("kunden-portal"), n("shop")]);
    }

    #[test]
    fn load_checks_name_against_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        assert!(matches!(
            load(&paths, &n("kp")),
            Err(Error::UnknownContext(_))
        ));
        let path = paths.context_config(&n("kp"));
        fsutil::write_atomic(
            &path,
            new_config_toml(&n("other"), "O", PALETTE[0], "localhost/koje/fedora:45").as_bytes(),
        )
        .unwrap();
        assert!(matches!(
            load(&paths, &n("kp")),
            Err(Error::ContextNameMismatch { .. })
        ));
    }

    #[test]
    fn settings_edit_the_file_keeping_comments() {
        let name = ContextName::new("kp").unwrap();
        let text = new_config_toml(
            &name,
            "KP",
            "#3584e4".parse().unwrap(),
            "localhost/koje/fedora:45",
        )
        .replace(
            "mounts = []",
            "mounts = [\"~/src\", { path = \"/tmp\", unsafe = true }, \"~/old\"]",
        );
        let settings = ContextSettings {
            prefix: "Kunde".into(),
            label: Some("Kundenportal".into()),
            icon: Some("briefcase-symbolic".into()),
            color: "#e01b24".parse().unwrap(),
            image: "web".into(),
            profile: Some(crate::access::Profile::Desktop),
            switches: vec![("gpu", Some(true)), ("sound", None)],
            mounts: vec!["~/src".into(), "/tmp".into(), "~/docs".into()],
            memory_soft: Some("4G".parse().unwrap()),
            memory_hard: None,
        };
        let out = with_settings(&text, &settings).unwrap();
        let cfg = ContextConfig::from_toml(&out, std::path::Path::new("context.toml")).unwrap();
        assert_eq!(cfg.prefix, "Kunde");
        assert_eq!(cfg.label.as_deref(), Some("Kundenportal"));
        assert_eq!(cfg.icon.as_deref(), Some("briefcase-symbolic"));
        assert_eq!(cfg.color.to_string(), "#E01B24");
        assert_eq!(cfg.image, "web");
        assert!(
            out.contains("# The image (ADR 0040, 0067)"),
            "its comment stays"
        );
        assert_eq!(cfg.access.profile, Some(crate::access::Profile::Desktop));
        assert_eq!((cfg.access.gpu, cfg.access.sound), (Some(true), None));
        assert_eq!(cfg.mounts.len(), 3);
        assert!(cfg.mounts[1].allow_unsafe, "the unsafe mark stays");
        assert!(!cfg.mounts[2].allow_unsafe);
        assert_eq!(
            cfg.limits.memory_soft.map(|s| s.to_string()).as_deref(),
            Some("4G")
        );
        assert_eq!(cfg.limits.memory_hard, None);
        assert!(
            out.contains("# Host folders bound into the context"),
            "comments kept"
        );
        // Nothing set: no empty tables.
        let plain = ContextSettings {
            label: None,
            icon: None,
            profile: None,
            switches: vec![("gpu", None)],
            memory_soft: None,
            ..settings
        };
        let out = with_settings(&out, &plain).unwrap();
        assert!(
            !out.lines().any(|l| l == "[access]" || l == "[limits]"),
            "{out}"
        );
        assert!(!out.contains("label") && !out.contains("icon ="), "{out}");
    }

    #[test]
    fn validate_checks_name_and_mounts() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        let good = new_config_toml(&n("kp"), "KP", PALETTE[0], "localhost/koje/fedora:45");
        assert!(validate(&paths, &n("kp"), &good).is_ok());
        assert!(matches!(
            validate(&paths, &n("other"), &good),
            Err(Error::ContextNameMismatch { .. })
        ));
        let relative = good.replace("mounts = []", "mounts = [\"src\"]");
        assert!(validate(&paths, &n("kp"), &relative).is_err());
        let home = good.replace("mounts = []", "mounts = [\"~\"]");
        assert!(matches!(
            validate(&paths, &n("kp"), &home),
            Err(Error::RefusedMount { .. })
        ));
        assert!(validate(&paths, &n("kp"), "garbage").is_err());
    }

    #[test]
    fn remove_deletes_config_states_and_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        create(&paths, &n("kp"), None, None, "localhost/koje/fedora:45").unwrap();
        create(&paths, &n("shop"), None, None, "localhost/koje/fedora:45").unwrap();
        fsutil::write_atomic(&paths.recovery_file(&n("kp")), b"x").unwrap();
        let state = crate::name::StateName::new("default").unwrap();
        fsutil::write_atomic(&paths.state_file(&n("kp"), &state), b"x").unwrap();

        remove(&paths, &n("kp")).unwrap();
        for dir in dirs(&paths, &n("kp")) {
            assert!(!dir.exists(), "{}", dir.display());
        }
        assert!(exists(&paths, &n("shop")));
        assert!(matches!(
            remove(&paths, &n("kp")),
            Err(Error::UnknownContext(_))
        ));
    }

    #[test]
    fn list_skips_foreign_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        assert_eq!(list(&paths).unwrap(), []);
        std::fs::create_dir_all(paths.contexts_dir().join("Not A Name")).unwrap();
        std::fs::create_dir_all(paths.contexts_dir().join("no-config")).unwrap();
        create(&paths, &n("kp"), None, None, "localhost/koje/fedora:45").unwrap();
        assert_eq!(list(&paths).unwrap().len(), 1);
    }

    #[test]
    fn favorites_are_set_and_removed_comments_stay() {
        let name = ContextName::new("kp").unwrap();
        let text = with_limits(
            &new_config_toml(&name, "KP", Color::rgb(1, 2, 3), "localhost/koje/fedora:45"),
            LimitChange::Set(ByteSize(6 << 30)),
            LimitChange::Keep,
        )
        .unwrap();
        let ids = [
            "org.mozilla.firefox.desktop".to_owned(),
            "foot.desktop".to_owned(),
        ];
        let out = with_favorites(&text, Some(&ids)).unwrap();
        assert!(
            out.contains("# Host folders bound into the context"),
            "{out}"
        );
        let cfg = ContextConfig::from_toml(&out, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.favorites.as_deref(), Some(&ids[..]));
        assert_eq!(cfg.limits.memory_soft, Some(ByteSize(6 << 30)));
        // Pinned none: an empty list, not GNOME's.
        let none = with_favorites(&out, Some(&[])).unwrap();
        let cfg = ContextConfig::from_toml(&none, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.favorites, Some(vec![]));
        let reset = with_favorites(&none, None).unwrap();
        let cfg = ContextConfig::from_toml(&reset, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.favorites, None);
        assert!(with_favorites(&text, Some(&["firefox".to_owned()])).is_err());
    }

    #[test]
    fn limits_change_and_comments_stay() {
        let name = ContextName::new("kp").unwrap();
        let text = new_config_toml(&name, "KP", Color::rgb(1, 2, 3), "localhost/koje/fedora:45");
        let six = ByteSize(6 << 30);
        let out = with_limits(&text, LimitChange::Set(six), LimitChange::Keep).unwrap();
        assert!(
            out.contains("# Host folders bound into the context"),
            "comments kept"
        );
        assert!(out.contains("memory_soft = \"6G\""), "{out}");
        let cfg = ContextConfig::from_toml(&out, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.limits.memory_soft, Some(six));
        assert_eq!(cfg.limits.memory_hard, None);
        let both =
            with_limits(&out, LimitChange::Keep, LimitChange::Set(ByteSize(8 << 30))).unwrap();
        let cleared = with_limits(&both, LimitChange::Clear, LimitChange::Keep).unwrap();
        let cfg = ContextConfig::from_toml(&cleared, std::path::Path::new("x")).unwrap();
        assert_eq!(
            (cfg.limits.memory_soft, cfg.limits.memory_hard),
            (None, Some(ByteSize(8 << 30)))
        );
    }
}
