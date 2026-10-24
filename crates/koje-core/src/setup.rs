//! The plan of `koje setup` (ADR 0021): which of koje's files go where, and
//! what happens to files that exist already. Pure: the CLI reads the current
//! files, this module decides, the CLI prints, asks once and executes.

use std::fmt;
use std::path::{Path, PathBuf};

/// What a step does to its target, as the plan shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    New,
    /// An existing file is replaced (warned about).
    Overwrite,
    /// A line is appended to the user's file (warned about).
    Append,
    Unchanged,
    /// Deliberately not done (e.g. the user has an own Waybar config).
    Skipped,
}

impl Mark {
    pub fn symbol(self) -> char {
        match self {
            Self::New => '+',
            Self::Overwrite => '!',
            Self::Append => '~',
            Self::Unchanged => '=',
            Self::Skipped => '-',
        }
    }

    /// The step changes something.
    pub fn changes(self) -> bool {
        matches!(self, Self::New | Self::Overwrite | Self::Append)
    }

    /// The step changes something that exists.
    pub fn warns(self) -> bool {
        matches!(self, Self::Overwrite | Self::Append)
    }
}

/// What executing a step means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Write `contents` to the target (atomically) with `mode`.
    Write {
        path: PathBuf,
        contents: &'static [u8],
        mode: u32,
    },
    /// Add `line` to the end of the user's Hyprland config (created if missing).
    SourceLine {
        path: PathBuf,
        line: String,
    },
    /// Build the base image (last; a failure does not undo the rest).
    BuildImage,
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

/// One of koje's files: replaced when it differs.
pub fn file(
    label: &str,
    path: &Path,
    contents: &'static [u8],
    mode: u32,
    current: Option<&[u8]>,
) -> Step {
    let what = format!("{label}: {}", path.display());
    let write = Action::Write {
        path: path.to_owned(),
        contents,
        mode,
    };
    match current {
        None => Step {
            mark: Mark::New,
            what,
            action: write,
        },
        Some(c) if c == contents => Step {
            mark: Mark::Unchanged,
            what,
            action: Action::Nothing,
        },
        Some(_) => Step {
            mark: Mark::Overwrite,
            what,
            action: write,
        },
    }
}

/// The `source = <snippet>` line in the user's `hyprland.conf`. `home` lets a
/// line written as `~/…` count too.
pub fn hyprland_source(conf: &Path, snippet: &Path, home: &Path, current: Option<&str>) -> Step {
    let line = format!("source = {}", snippet.display());
    let tilde = snippet
        .strip_prefix(home)
        .ok()
        .map(|rel| format!("source = ~/{}", rel.display()));
    let action = Action::SourceLine {
        path: conf.to_owned(),
        line: line.clone(),
    };
    match current {
        None => Step {
            mark: Mark::New,
            what: format!(
                "Hyprland config: {} (new, sources koje's snippet)",
                conf.display()
            ),
            action,
        },
        Some(text)
            if text
                .lines()
                .map(str::trim)
                .any(|l| l == line || tilde.as_deref() == Some(l)) =>
        {
            Step {
                mark: Mark::Unchanged,
                what: format!(
                    "Hyprland config: {} already sources koje's snippet",
                    conf.display()
                ),
                action: Action::Nothing,
            }
        }
        Some(_) => Step {
            mark: Mark::Append,
            what: format!("Hyprland config: {} (append: {line})", conf.display()),
            action,
        },
    }
}

/// A file in the Waybar config directory, as setup sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaybarFile {
    Missing,
    /// Installed by koje before (its header) or a link to the old spike draft.
    Koje(Vec<u8>),
    /// The user's own.
    Foreign,
}

/// Whether `contents` is a Waybar file koje installed (first line is koje's header).
pub fn is_koje_waybar(contents: &[u8]) -> bool {
    let first = contents.split(|b| *b == b'\n').next().unwrap_or_default();
    let first = String::from_utf8_lossy(first);
    (first.starts_with("//") || first.starts_with("/*")) && first.contains("koje's Waybar")
}

/// Waybar: koje's files only if the user has no config of their own.
/// `present` lists every file Waybar would read (`config`, `config.jsonc`,
/// `style.css`); `files` are koje's (`config.jsonc`, `style.css`).
pub fn waybar(
    dir: &Path,
    files: &[(&str, &'static [u8])],
    present: &[(&str, WaybarFile)],
) -> Vec<Step> {
    if present.iter().any(|(_, f)| *f == WaybarFile::Foreign) {
        return vec![Step {
            mark: Mark::Skipped,
            what: format!("Waybar: your own config in {} – left alone", dir.display()),
            action: Action::Nothing,
        }];
    }
    files
        .iter()
        .map(|(name, contents)| {
            let current = present.iter().find(|(n, _)| n == name).map(|(_, f)| f);
            let path = dir.join(name);
            match current {
                Some(WaybarFile::Koje(c)) => file("Waybar", &path, contents, 0o644, Some(c)),
                _ => file("Waybar", &path, contents, 0o644, None),
            }
        })
        .collect()
}

/// The base image: built when missing.
pub fn image(exists: Option<bool>) -> Step {
    match exists {
        Some(true) => Step {
            mark: Mark::Unchanged,
            what: "base image: koje/base (rebuild with: koje image build)".into(),
            action: Action::Nothing,
        },
        Some(false) => Step {
            mark: Mark::New,
            what: "base image: koje/base (built with Podman, takes a few minutes)".into(),
            action: Action::BuildImage,
        },
        None => Step {
            mark: Mark::Skipped,
            what: "base image: Podman is not installed".into(),
            action: Action::Nothing,
        },
    }
}

/// A program koje's Hyprland snippet starts at login (`exec-once`, ADR 0037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    pub name: &'static str,
    /// `pgrep` arguments that find it running.
    pub pgrep: &'static [&'static str],
    /// Its `exec-once` command in the snippet.
    pub command: &'static str,
}

/// The snippet's services, in start order: kojed first (the bar asks it).
pub const SESSION: [Service; 5] = [
    Service {
        name: "kojed",
        pgrep: &["-x", "kojed"],
        command: "systemctl --user start kojed",
    },
    Service {
        name: "Waybar",
        pgrep: &["-x", "waybar"],
        command: "waybar",
    },
    Service {
        name: "mako",
        pgrep: &["-x", "mako"],
        command: "mako",
    },
    Service {
        name: "hypridle",
        pgrep: &["-x", "hypridle"],
        command: "hypridle",
    },
    Service {
        name: "polkit agent",
        pgrep: &["-f", "polkit-gnome-authentication-agent-1"],
        command: "sh -c 'for a in /usr/libexec/polkit-gnome-authentication-agent-1 /usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1; do [ -x \"$a\" ] && exec \"$a\"; done'",
    },
];

/// The running session after setup changed files: what is not current yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    /// `hyprctl reload`: the snippet or the `source` line changed.
    Hyprland,
    /// Waybar reads its changed config again (SIGUSR2).
    Waybar,
    /// Start a service that is not running (`exec-once` runs only at login).
    Start(Service),
}

impl fmt::Display for Refresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyprland => write!(f, "reload Hyprland's config"),
            Self::Waybar => write!(f, "reload Waybar's config"),
            Self::Start(s) => write!(f, "start {}", s.name),
        }
    }
}

/// What to refresh in the current Hyprland session after `steps` ran;
/// `running` tells which of [`SESSION`] run now. Nothing when setup changed
/// nothing: it stops there.
pub fn refresh(
    steps: &[Step],
    snippet: &Path,
    waybar: &Path,
    running: &dyn Fn(&Service) -> bool,
) -> Vec<Refresh> {
    if !steps.iter().any(|s| s.mark.changes()) {
        return Vec::new();
    }
    let changed = |s: &&Step| s.mark.changes();
    let hyprland = steps.iter().filter(changed).any(|s| match &s.action {
        Action::Write { path, .. } => path == snippet,
        Action::SourceLine { .. } => true,
        _ => false,
    });
    let waybar_changed = steps
        .iter()
        .filter(changed)
        .any(|s| matches!(&s.action, Action::Write { path, .. } if path.starts_with(waybar)));
    let mut out = Vec::new();
    if hyprland {
        out.push(Refresh::Hyprland);
    }
    for service in SESSION {
        if !running(&service) {
            out.push(Refresh::Start(service));
        } else if service.command == "waybar" && waybar_changed {
            out.push(Refresh::Waybar);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNIPPET: &[u8] = b"# koje\n";

    #[test]
    fn files() {
        let p = Path::new("/h/.local/share/koje/hyprland/koje.conf");
        assert_eq!(file("snippet", p, SNIPPET, 0o644, None).mark, Mark::New);
        let same = file("snippet", p, SNIPPET, 0o644, Some(SNIPPET));
        assert_eq!((same.mark, same.action), (Mark::Unchanged, Action::Nothing));
        let other = file("snippet", p, SNIPPET, 0o644, Some(b"old"));
        assert_eq!(other.mark, Mark::Overwrite);
        assert!(other.mark.warns() && other.mark.changes());
        assert_eq!(
            other.to_string(),
            "! snippet: /h/.local/share/koje/hyprland/koje.conf"
        );
    }

    #[test]
    fn source_line() {
        let conf = Path::new("/h/.config/hypr/hyprland.conf");
        let snippet = Path::new("/h/.local/share/koje/hyprland/koje.conf");
        let home = Path::new("/h");
        assert_eq!(hyprland_source(conf, snippet, home, None).mark, Mark::New);
        let appended = hyprland_source(conf, snippet, home, Some("monitor = ,preferred,auto,1\n"));
        assert_eq!(appended.mark, Mark::Append);
        assert_eq!(
            appended.action,
            Action::SourceLine {
                path: conf.to_owned(),
                line: "source = /h/.local/share/koje/hyprland/koje.conf".into()
            }
        );
        for text in [
            "a\nsource = /h/.local/share/koje/hyprland/koje.conf\n",
            "  source = ~/.local/share/koje/hyprland/koje.conf  \n",
        ] {
            assert_eq!(
                hyprland_source(conf, snippet, home, Some(text)).mark,
                Mark::Unchanged,
                "{text}"
            );
        }
        // Commented out: the user turned it off, setup appends again – visible as ~.
        assert_eq!(
            hyprland_source(
                conf,
                snippet,
                home,
                Some("# source = /h/.local/share/koje/hyprland/koje.conf\n")
            )
            .mark,
            Mark::Append
        );
    }

    #[test]
    fn waybar_only_without_an_own_config() {
        let dir = Path::new("/h/.config/waybar");
        let ours: [(&str, &'static [u8]); 2] = [
            ("config.jsonc", b"// koje's Waybar config\n{}"),
            ("style.css", b"/* koje's Waybar style */"),
        ];
        assert!(is_koje_waybar(ours[0].1) && is_koje_waybar(ours[1].1));
        assert!(!is_koje_waybar(b"{ \"layer\": \"top\" }"));

        let fresh = waybar(dir, &ours, &[]);
        assert_eq!(
            fresh.iter().map(|s| s.mark).collect::<Vec<_>>(),
            [Mark::New, Mark::New]
        );

        let update = waybar(
            dir,
            &ours,
            &[
                (
                    "config.jsonc",
                    WaybarFile::Koje(b"// koje's Waybar config\nold".to_vec()),
                ),
                ("style.css", WaybarFile::Koje(ours[1].1.to_vec())),
            ],
        );
        assert_eq!(
            update.iter().map(|s| s.mark).collect::<Vec<_>>(),
            [Mark::Overwrite, Mark::Unchanged]
        );

        let own = waybar(dir, &ours, &[("config", WaybarFile::Foreign)]);
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].mark, Mark::Skipped);
    }

    #[test]
    fn image_steps() {
        assert_eq!(image(Some(false)).action, Action::BuildImage);
        assert_eq!(image(Some(true)).mark, Mark::Unchanged);
        assert_eq!(image(None).mark, Mark::Skipped);
    }

    fn write(path: &str) -> Step {
        file("f", Path::new(path), SNIPPET, 0o644, None)
    }

    #[test]
    fn session_services_are_the_snippets() {
        let snippet = include_str!("../../../assets/hyprland/koje.conf");
        for s in SESSION {
            let line = format!("exec-once = {}", s.command);
            assert!(snippet.lines().any(|l| l == line), "{line}");
        }
    }

    #[test]
    fn refresh_after_setup() {
        let snippet = Path::new("/d/koje/hyprland/koje.conf");
        let waybar = Path::new("/c/waybar");
        let all = |_: &Service| true;
        let none = |_: &Service| false;

        // Nothing changed: nothing to do, even with services not running.
        let same = Step {
            mark: Mark::Unchanged,
            what: String::new(),
            action: Action::Nothing,
        };
        assert_eq!(refresh(&[same], snippet, waybar, &none), []);

        // New snippet and bar: reload Hyprland, Waybar rereads its config.
        let steps = [
            write("/d/koje/hyprland/koje.conf"),
            write("/c/waybar/config.jsonc"),
        ];
        assert_eq!(
            refresh(&steps, snippet, waybar, &all),
            [Refresh::Hyprland, Refresh::Waybar]
        );

        // First install: nothing runs yet – start all, kojed first; no SIGUSR2
        // for a Waybar that starts anyway.
        let r = refresh(&steps, snippet, waybar, &none);
        assert_eq!(r[0], Refresh::Hyprland);
        assert_eq!(r[1], Refresh::Start(SESSION[0]));
        assert_eq!(r.len(), 1 + SESSION.len());

        // Only the source line: a reload.
        let source = hyprland_source(
            Path::new("/c/hypr/hyprland.conf"),
            snippet,
            Path::new("/h"),
            Some(""),
        );
        assert_eq!(
            refresh(&[source], snippet, waybar, &all),
            [Refresh::Hyprland]
        );
    }
}
