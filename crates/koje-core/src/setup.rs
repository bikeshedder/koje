//! The plan of `koje setup` (ADR 0021, 0045, 0061): koje's GNOME Shell
//! extension and the base image. The package installs everything else. Pure:
//! the CLI reads the current state, this module decides, the CLI prints, asks
//! once and executes.

use std::fmt;
use std::path::Path;

/// What a step does, as the plan shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    New,
    Unchanged,
    /// Not done (e.g. Podman is missing).
    Skipped,
}

impl Mark {
    pub fn symbol(self) -> char {
        match self {
            Self::New => '+',
            Self::Unchanged => '=',
            Self::Skipped => '-',
        }
    }

    /// The step changes something.
    pub fn changes(self) -> bool {
        self == Self::New
    }
}

/// What executing a step means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Build the base image (last; a failure does not undo the rest).
    BuildImage,
    /// Enable koje's GNOME Shell extension (ADR 0059).
    EnableGnomeExtension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub mark: Mark,
    pub what: String,
    pub action: Action,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.mark.symbol(), self.what)
    }
}

/// koje's units with the binary they should run.
pub const UNITS: [(&str, &str); 1] = [("kojed", "kojed")];

/// A running unit whose executable is not the package's `binary` is
/// restarted after setup: an old one from `~/.local/bin`, or the package was
/// upgraded under it (`/usr/bin/kojed (deleted)`).
pub fn outdated(exe: &Path, binary: &str) -> bool {
    exe != Path::new(crate::paths::BIN_DIR).join(binary)
}

/// The images the contexts use, with your user (ADR 0040, 0067): built when
/// missing.
pub fn image(exists: Option<bool>) -> Step {
    match exists {
        Some(true) => Step {
            mark: Mark::Unchanged,
            what: "images of the contexts (rebuild with: koje image build)".into(),
            action: Action::Nothing,
        },
        Some(false) => Step {
            mark: Mark::New,
            what: "images of the contexts (built with Podman, takes a few minutes)".into(),
            action: Action::BuildImage,
        },
        None => Step {
            mark: Mark::Skipped,
            what: "images: Podman is not installed".into(),
            action: Action::Nothing,
        },
    }
}

/// koje's GNOME Shell extension (ADR 0059).
pub const GNOME_EXTENSION: &str = "koje@koje.io";

/// Enable koje's extension unless `enabled` (GNOME's `enabled-extensions`)
/// has it.
pub fn gnome_extension(enabled: &[String]) -> Step {
    if enabled.iter().any(|e| e == GNOME_EXTENSION) {
        Step {
            mark: Mark::Unchanged,
            what: format!("GNOME Shell extension {GNOME_EXTENSION}: enabled"),
            action: Action::Nothing,
        }
    } else {
        Step {
            mark: Mark::New,
            what: format!(
                "GNOME Shell extension {GNOME_EXTENSION}: enable (log out and in once if GNOME does not know it yet)"
            ),
            action: Action::EnableGnomeExtension,
        }
    }
}

/// A string list as `gsettings get` prints it (`['a', 'b']`, `@as []`).
pub fn parse_strv(text: &str) -> Vec<String> {
    let text = text.trim().trim_start_matches("@as").trim();
    let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A string list for `gsettings set`.
pub fn format_strv(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_extension_step_and_lists() {
        assert_eq!(parse_strv("@as []"), Vec::<String>::new());
        let list = parse_strv("['ubuntu-dock@ubuntu.com', 'ding@rastersoft.com']\n");
        assert_eq!(list, ["ubuntu-dock@ubuntu.com", "ding@rastersoft.com"]);
        assert_eq!(
            format_strv(&list),
            "['ubuntu-dock@ubuntu.com', 'ding@rastersoft.com']"
        );
        assert_eq!(gnome_extension(&list).action, Action::EnableGnomeExtension);
        let on = parse_strv("['koje@koje.io']");
        assert_eq!(gnome_extension(&on).mark, Mark::Unchanged);
    }

    #[test]
    fn kojed_is_outdated_unless_it_runs_the_packages_binary() {
        assert!(!outdated(Path::new("/usr/bin/kojed"), "kojed"));
        assert!(outdated(Path::new("/usr/bin/kojed (deleted)"), "kojed"));
        assert!(outdated(Path::new("/home/mj/.local/bin/kojed"), "kojed"));
    }

    #[test]
    fn image_steps() {
        assert_eq!(image(Some(false)).action, Action::BuildImage);
        assert_eq!(image(Some(true)).mark, Mark::Unchanged);
        assert_eq!(image(None).mark, Mark::Skipped);
        assert!(image(Some(false)).mark.changes());
        assert_eq!(image(None).to_string(), "- images: Podman is not installed");
    }
}
