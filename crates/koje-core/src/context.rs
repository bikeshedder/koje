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
pub fn new_config_toml(name: &ContextName, prefix: &str, color: Color) -> String {
    let prefix = toml::Value::String(prefix.to_owned());
    format!(
        r#"name   = "{name}"
prefix = {prefix}
color  = "{color}"
image  = "koje/base:latest"

# Host folders bound into the context at the same path (ADR 0009).
mounts = []

# [limits]                # unset values come from config.toml
# memory_soft = "6G"      # warn at 90 % and 100 %
# memory_hard = "8G"      # orderly shutdown

# [access]                # host resources, ADR 0017 (default from config.toml)
# profile = "full"        # full | desktop | headless | isolated
# gpu     = false         # override single switches: gpu, sound, wayland, nested, network

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
    let text = new_config_toml(name, &prefix, color);
    let cfg = ContextConfig::from_toml(&text, &path)?;
    fsutil::write_atomic(&path, text.as_bytes())?;
    Ok(cfg)
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
    fn template_parses_and_escapes_prefix() {
        let text = new_config_toml(&n("kp"), "K\"P", PALETTE[1]);
        let cfg = ContextConfig::from_toml(&text, std::path::Path::new("x")).unwrap();
        assert_eq!(cfg.prefix, "K\"P");
        assert_eq!(cfg.color, PALETTE[1]);
        assert_eq!(cfg.image, "koje/base:latest");
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
        let a = create(&paths, &n("kunden-portal"), None, None).unwrap();
        assert_eq!((a.prefix.as_str(), a.color), ("KP", PALETTE[0]));
        let b = create(&paths, &n("shop"), Some("SH".into()), None).unwrap();
        assert_eq!(b.color, PALETTE[1]);
        assert!(matches!(
            create(&paths, &n("shop"), None, None),
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
            new_config_toml(&n("other"), "O", PALETTE[0]).as_bytes(),
        )
        .unwrap();
        assert!(matches!(
            load(&paths, &n("kp")),
            Err(Error::ContextNameMismatch { .. })
        ));
    }

    #[test]
    fn validate_checks_name_and_mounts() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&tmp);
        let good = new_config_toml(&n("kp"), "KP", PALETTE[0]);
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
        create(&paths, &n("kp"), None, None).unwrap();
        create(&paths, &n("shop"), None, None).unwrap();
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
        create(&paths, &n("kp"), None, None).unwrap();
        assert_eq!(list(&paths).unwrap().len(), 1);
    }
}
