//! Integrations (ADR 0031): optional descriptions of how koje starts a program
//! again and what it remembers. Declarative TOML; built-ins ship with koje,
//! files in `~/.config/koje/integrations/` add to or replace them by name.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// How a remembered value is found (kojed reads `/proc` on the host).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Detect {
    /// The last argument of the window's process (not an option).
    ArgvLast,
    /// The working directory of the window's process.
    Cwd,
    /// A terminal's shell: one shell below it → its directory; several → a
    /// path in the window title; else nothing.
    ShellCwd,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Wayland app ids (Hyprland's `class`), compared exactly.
    #[serde(default)]
    pub app_id: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Integration {
    pub name: String,
    #[serde(rename = "match", default)]
    pub matches: Match,
    /// Start template: `{program}` (the original program), `{command}` (the
    /// whole original command line) and remembered values (`{path}`). A
    /// remembered `cwd` is the working directory to start in.
    #[serde(default = "default_launch")]
    pub launch: String,
    /// Keys stored in the state.
    #[serde(default)]
    pub remember: Vec<String>,
    #[serde(default)]
    pub detect: BTreeMap<String, Detect>,
    /// One start restores all windows (browsers with session restore).
    #[serde(default)]
    pub launch_once: bool,
}

fn default_launch() -> String {
    "{command}".to_owned()
}

const BUILT_IN: [(&str, &str); 3] = [
    ("terminal", include_str!("integrations/terminal.toml")),
    ("editor", include_str!("integrations/editor.toml")),
    ("browser", include_str!("integrations/browser.toml")),
];

impl Integration {
    pub fn from_toml(text: &str, path: &Path) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })
    }
}

/// The built-in integrations.
pub fn built_in() -> Vec<Integration> {
    BUILT_IN
        .iter()
        .filter_map(|(name, text)| Integration::from_toml(text, Path::new(name)).ok())
        .collect()
}

/// Built-ins, then the user's files (`*.toml` in `dir`) replacing by name.
/// Broken files are skipped and returned as errors, so one bad file does not
/// stop the others.
pub fn load(dir: &Path) -> (Vec<Integration>, Vec<Error>) {
    let mut all: BTreeMap<String, Integration> = built_in()
        .into_iter()
        .map(|i| (i.name.clone(), i))
        .collect();
    let mut errors = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for path in files {
        let parsed = std::fs::read_to_string(&path)
            .map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })
            .and_then(|text| Integration::from_toml(&text, &path));
        match parsed {
            Ok(i) => {
                all.insert(i.name.clone(), i);
            }
            Err(e) => errors.push(e),
        }
    }
    (all.into_values().collect(), errors)
}

/// The integration for a window's app id; `None` means `generic`.
pub fn find<'a>(all: &'a [Integration], app_id: &str) -> Option<&'a Integration> {
    all.iter()
        .find(|i| i.matches.app_id.iter().any(|a| a == app_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_parse() {
        let all = built_in();
        assert_eq!(all.len(), BUILT_IN.len(), "every built-in parses");
        assert_eq!(find(&all, "foot").unwrap().name, "terminal");
        assert_eq!(find(&all, "code").unwrap().detect["path"], Detect::ArgvLast);
        assert!(find(&all, "firefox").unwrap().launch_once);
        assert_eq!(find(&all, "org.gnome.Nautilus"), None, "generic");
    }

    #[test]
    fn user_files_replace_and_add() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("editor.toml"),
            "name = \"editor\"\nmatch = { app_id = [\"emacs\"] }\nlaunch = \"{program} {path}\"\nremember = [\"path\"]\ndetect = { path = \"argv-last\" }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("viewer.toml"),
            "name = \"viewer\"\nmatch = { app_id = [\"org.gnome.Evince\"] }\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("broken.toml"), "name = \"x\"\nnope = 1\n").unwrap();
        let (all, errors) = load(tmp.path());
        assert_eq!(errors.len(), 1);
        assert_eq!(find(&all, "emacs").unwrap().name, "editor");
        assert_eq!(
            find(&all, "code"),
            None,
            "the user's editor replaced the built-in"
        );
        assert_eq!(find(&all, "org.gnome.Evince").unwrap().launch, "{command}");
        assert_eq!(find(&all, "foot").unwrap().name, "terminal");
    }
}
