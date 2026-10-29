//! The plan of `koje setup` (ADR 0021, 0045): which of koje's per-user files
//! go where, what happens to files that exist already, and which files of the
//! old `install.sh` layout go away. Pure: the CLI reads the current
//! files, this module decides, the CLI prints, asks once and executes.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::color::Color;
use crate::name::ContextName;

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
    /// One of koje's old files is removed (warned about).
    Remove,
}

impl Mark {
    pub fn symbol(self) -> char {
        match self {
            Self::New => '+',
            Self::Overwrite => '!',
            Self::Append => '~',
            Self::Unchanged => '=',
            Self::Skipped => '-',
            Self::Remove => 'x',
        }
    }

    /// The step changes something.
    pub fn changes(self) -> bool {
        matches!(
            self,
            Self::New | Self::Overwrite | Self::Append | Self::Remove
        )
    }

    /// The step changes something that exists.
    pub fn warns(self) -> bool {
        matches!(self, Self::Overwrite | Self::Append | Self::Remove)
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
    /// Add `line` to the end of the user's Hyprland config.
    SourceLine {
        path: PathBuf,
        line: String,
    },
    /// Write a complete Hyprland config (ADR 0037); an existing one is kept as
    /// `<path>.bak` first.
    HyprlandConf {
        path: PathBuf,
        contents: String,
    },
    /// Remove one of koje's old files.
    Remove {
        path: PathBuf,
    },
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

/// What `install.sh` installed below the home that the package now installs
/// system-wide (ADR 0045), with a label for the plan. `hyprbars.so` in
/// `~/.local/lib/koje/` is not among them: `scripts/setup-hyprbars.sh` builds
/// it per user.
pub fn old_install(
    home: &Path,
    font_dir: &Path,
    kojed_unit: &Path,
) -> Vec<(&'static str, PathBuf)> {
    let bin = home.join(".local/bin");
    let lib = home.join(".local/lib/koje");
    vec![
        ("old koje", bin.join("koje")),
        ("old kojed", bin.join("kojed")),
        ("old forwarder", lib.join("koje-forward")),
        ("old window picker", lib.join("koje-window-picker")),
        ("old context picker", lib.join("koje-context-picker")),
        ("old application menu", lib.join("koje-app-menu")),
        ("old keyboard shortcuts", lib.join("koje-keys")),
        ("old icon font", font_dir.join("lucide.ttf")),
        ("old icon font license", font_dir.join("LICENSE")),
        ("old kojed unit", kojed_unit.to_owned()),
    ]
}

/// The kojed unit `install.sh` installed: it starts `~/.local/bin/kojed`.
pub fn is_old_kojed_unit(contents: &[u8]) -> bool {
    String::from_utf8_lossy(contents)
        .lines()
        .any(|l| l.trim() == "ExecStart=%h/.local/bin/kojed")
}

/// Remove an old file of koje's: `None` when it is gone already. A kojed
/// unit that is not koje's old one stays, but the plan says that it hides
/// the package's.
pub fn remove_old(label: &str, path: &Path, current: Option<&[u8]>, unit: bool) -> Option<Step> {
    let current = current?;
    let what = format!("{label}: {}", path.display());
    Some(if unit && !is_old_kojed_unit(current) {
        Step {
            mark: Mark::Skipped,
            what: format!("{what} (your own – it hides the package's unit)"),
            action: Action::Nothing,
        }
    } else {
        Step {
            mark: Mark::Remove,
            what,
            action: Action::Remove {
                path: path.to_owned(),
            },
        }
    })
}

/// koje's units and their binaries: restarted after setup when they run an
/// executable that is not the package's (ADR 0045, 0048).
pub const UNITS: [(&str, &str); 2] = [("kojed", "kojed"), ("koje-shell", "koje-shell")];

/// A running unit whose executable is not the package's `binary` is
/// restarted after setup: an old one from `~/.local/bin`, or the package was
/// upgraded under it (`/usr/bin/kojed (deleted)`).
pub fn outdated(exe: &Path, binary: &str) -> bool {
    exe != Path::new(crate::paths::BIN_DIR).join(binary)
}

/// Keyboard layouts for Hyprland's `input` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyboard {
    pub layout: String,
    pub variant: String,
    pub options: String,
}

/// The system's keyboard (`/etc/default/keyboard`, Debian/Ubuntu), plus `us`
/// to switch to when it is a single other layout; `us` without one.
pub fn system_keyboard(text: Option<&str>) -> Keyboard {
    let get = |key: &str| {
        text.unwrap_or_default()
            .lines()
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_owned())
            .unwrap_or_default()
    };
    let (mut layout, mut variant) = (get("XKBLAYOUT"), get("XKBVARIANT"));
    if layout.is_empty() {
        layout = "us".into();
        variant.clear();
    }
    let single_other = !layout.contains(',') && (layout != "us" || !variant.is_empty());
    if single_other {
        layout.push_str(",us");
        variant.push(',');
    }
    Keyboard {
        layout,
        variant,
        options: get("XKBOPTIONS"),
    }
}

/// The `hyprland.conf` koje writes when the user has none of their own.
pub fn hyprland_conf(source: &str, kb: &Keyboard) -> String {
    let options = if kb.options.is_empty() {
        String::new()
    } else {
        format!("    kb_options = {}\n", kb.options)
    };
    format!(
        "# Hyprland config – written by koje setup (ADR 0037). It is yours now: koje
# setup never rewrites it. koje's session comes from the snippet below; your
# settings go after it (later settings win; `unbind = MODS, key` drops a binding).

{source}

# This machine's screens (names and modes: hyprctl monitors all).
monitor = , preferred, auto, 1

# Keyboard layouts, from the system's at setup. Super+Space or a click on the
# bar switches to the next one. Example: kb_layout = us,de / kb_variant = colemak,
input {{
    kb_layout = {}
    kb_variant = {}
{options}}}
",
        kb.layout, kb.variant
    )
}

/// The user's `hyprland.conf` must source koje's snippet. `home` lets a line
/// written as `~/…` count too. koje writes the whole file (`keyboard` in it)
/// when there is none, when it is Hyprland's generated default, or when it
/// holds nothing but the `source` line; any other config is the user's and
/// only gets the line appended.
pub fn hyprland_source(
    conf: &Path,
    snippet: &Path,
    home: &Path,
    current: Option<&str>,
    keyboard: &Keyboard,
) -> Step {
    let line = format!("source = {}", snippet.display());
    let tilde = snippet
        .strip_prefix(home)
        .ok()
        .map(|rel| format!("source = ~/{}", rel.display()));
    let is_source = |l: &str| l == line || tilde.as_deref() == Some(l);
    let write = Action::HyprlandConf {
        path: conf.to_owned(),
        contents: hyprland_conf(&line, keyboard),
    };
    let Some(text) = current else {
        return Step {
            mark: Mark::New,
            what: format!(
                "Hyprland config: {} (new, with koje's session)",
                conf.display()
            ),
            action: write,
        };
    };
    let settings: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let generated = settings
        .iter()
        .any(|l| l.split('#').next().unwrap_or_default().replace(' ', "") == "autogenerated=1");
    if generated || settings.iter().all(|l| is_source(l)) {
        let why = if generated {
            "Hyprland's generated default"
        } else {
            "nothing of yours"
        };
        return Step {
            mark: Mark::Overwrite,
            what: format!(
                "Hyprland config: {} ({why} – replaced with koje's session, old one kept as .bak)",
                conf.display()
            ),
            action: write,
        };
    }
    if settings.iter().any(|l| is_source(l)) {
        Step {
            mark: Mark::Unchanged,
            what: format!(
                "Hyprland config: {} already sources koje's snippet",
                conf.display()
            ),
            action: Action::Nothing,
        }
    } else {
        Step {
            mark: Mark::Append,
            what: format!("Hyprland config: {} (append: {line})", conf.display()),
            action: Action::SourceLine {
                path: conf.to_owned(),
                line,
            },
        }
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

/// The context colors for koje's Waybar style (M8.20), next to it; kojed
/// writes it, koje's `style.css` imports it.
pub const WAYBAR_COLORS: &str = "koje-contexts.css";

const WAYBAR_COLORS_HEADER: &str =
    "/* koje's Waybar context colors – written by kojed, do not edit. */\n";

/// The CSS class Waybar gives a workspace's button (`sanitizeCssClass` in its
/// hyprland/workspaces, after 0.15.0): lowercase, other characters as one
/// `-`, none at the ends, `ws-` before a leading digit.
pub fn waybar_class(name: &str) -> String {
    let mut class = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            class.push(c.to_ascii_lowercase());
        } else if !class.is_empty() && !class.ends_with('-') {
            class.push('-');
        }
    }
    if class.ends_with('-') {
        class.pop();
    }
    if class.starts_with(|c: char| c.is_ascii_digit()) {
        class.insert_str(0, "ws-");
    }
    class
}

/// The context color as the bar's accent: the mark of the active workspace
/// in the workspace list and of the focused window in the task list (it
/// shows the focused workspace's windows; a context's workspace is named
/// like it). Task buttons sit inside the workspace button, hence the `:not`.
pub fn waybar_colors(colors: &BTreeMap<ContextName, Color>) -> String {
    use std::fmt::Write as _;
    let mut css = WAYBAR_COLORS_HEADER.to_owned();
    for (ctx, color) in colors {
        let class = waybar_class(ctx.as_str());
        let _ = writeln!(
            css,
            "#workspaces button.{class}.active:not(.taskbar-window) {{ border-bottom-color: {color}; }}\n\
             #workspaces.tasks button.{class} .taskbar-window.active {{ border-bottom-color: {color}; }}"
        );
    }
    css
}

/// Waybar: koje's files only if the user has no config of their own.
/// `present` lists every file Waybar would read (`config`, `config.jsonc`,
/// `style.css`); `files` are koje's (`config.jsonc`, `style.css`). The
/// context colors file is created when missing and then left to kojed.
pub fn waybar(
    dir: &Path,
    files: &[(&str, &'static [u8])],
    present: &[(&str, WaybarFile)],
    colors_present: bool,
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
        .chain(std::iter::once({
            let path = dir.join(WAYBAR_COLORS);
            if colors_present {
                Step {
                    mark: Mark::Unchanged,
                    what: format!("Waybar: {} (kojed keeps it current)", path.display()),
                    action: Action::Nothing,
                }
            } else {
                // Waybar refuses a style whose import is missing.
                file(
                    "Waybar",
                    &path,
                    WAYBAR_COLORS_HEADER.as_bytes(),
                    0o644,
                    None,
                )
            }
        }))
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
    /// Restart one of koje's units: it runs an old binary ([`outdated`]).
    Restart(&'static str),
    /// `koje session restart`: all of the koje session's services with their
    /// unit files as they are now.
    Session,
}

impl fmt::Display for Refresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyprland => write!(f, "reload Hyprland's config"),
            Self::Waybar => write!(f, "reload Waybar's config"),
            Self::Start(s) => write!(f, "start {}", s.name),
            Self::Restart(unit) => write!(f, "restart {unit} (it runs an old binary)"),
            Self::Session => write!(
                f,
                "restart the koje session's services and reload koje's Hyprland plugin"
            ),
        }
    }
}

/// In the koje session systemd starts the services (ADR 0048): only the
/// config reload and koje's restarts are left of a `refresh`. An upgrade – a
/// unit runs an old binary – may have changed any file of the package: its
/// Hyprland reloads the package's snippet, and all services restart.
pub fn for_koje_session(refresh: Vec<Refresh>) -> Vec<Refresh> {
    let upgraded = refresh.iter().any(|r| matches!(r, Refresh::Restart(_)));
    if upgraded {
        return vec![Refresh::Hyprland, Refresh::Session];
    }
    refresh
        .into_iter()
        .filter(|r| *r == Refresh::Hyprland)
        .collect()
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
        // koje's snippets: the main one and the title bar one next to it.
        Action::Write { path, .. } => path.parent() == snippet.parent(),
        Action::SourceLine { .. } | Action::HyprlandConf { .. } => true,
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

/// koje's GNOME Shell extension (ADR 0059).
pub const GNOME_EXTENSION: &str = "koje@koje.io";

/// GNOME: enable koje's extension unless `enabled` (GNOME's
/// `enabled-extensions`) has it.
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
    #[test]
    fn gnome_extension_step_and_lists() {
        use super::{Action, Mark, format_strv, gnome_extension, parse_strv};
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
    fn old_files_are_removed_when_present() {
        let p = Path::new("/h/.local/bin/kojed");
        assert_eq!(remove_old("old kojed", p, None, false), None);
        let step = remove_old("old kojed", p, Some(b"\x7fELF"), false).unwrap();
        assert_eq!(step.mark, Mark::Remove);
        assert!(step.mark.warns() && step.mark.changes());
        assert_eq!(step.action, Action::Remove { path: p.into() });
        assert_eq!(step.to_string(), "x old kojed: /h/.local/bin/kojed");
    }

    #[test]
    fn only_kojes_old_unit_is_removed() {
        let p = Path::new("/h/.config/systemd/user/kojed.service");
        let old = b"[Service]\nType=dbus\nExecStart=%h/.local/bin/kojed\n";
        assert_eq!(
            remove_old("old kojed unit", p, Some(old), true)
                .unwrap()
                .mark,
            Mark::Remove
        );
        let own = b"[Service]\nExecStart=/opt/kojed --debug\n";
        let kept = remove_old("old kojed unit", p, Some(own), true).unwrap();
        assert_eq!((kept.mark, kept.action), (Mark::Skipped, Action::Nothing));
    }

    #[test]
    fn old_install_lists_kojes_files_but_not_hyprbars() {
        let files = old_install(
            Path::new("/h"),
            Path::new("/h/.local/share/fonts/koje"),
            Path::new("/h/.config/systemd/user/kojed.service"),
        );
        let paths: Vec<_> = files.iter().map(|(_, p)| p.display().to_string()).collect();
        assert!(paths.contains(&"/h/.local/bin/koje".to_owned()));
        assert!(paths.contains(&"/h/.local/lib/koje/koje-forward".to_owned()));
        assert!(paths.contains(&"/h/.config/systemd/user/kojed.service".to_owned()));
        assert!(!paths.iter().any(|p| p.ends_with("hyprbars.so")));
    }

    #[test]
    fn kojed_is_outdated_unless_it_runs_the_packages_binary() {
        assert!(!outdated(Path::new("/usr/bin/kojed"), "kojed"));
        assert!(outdated(Path::new("/usr/bin/kojed (deleted)"), "kojed"));
        assert!(outdated(Path::new("/home/mj/.local/bin/kojed"), "kojed"));
        assert!(outdated(
            Path::new("/usr/bin/koje-shell (deleted)"),
            "koje-shell"
        ));
    }

    fn us() -> Keyboard {
        system_keyboard(None)
    }

    #[test]
    fn source_line() {
        let conf = Path::new("/h/.config/hypr/hyprland.conf");
        let snippet = Path::new("/h/.local/share/koje/hyprland/koje.conf");
        let home = Path::new("/h");
        let step = |text: Option<&str>| hyprland_source(conf, snippet, home, text, &us());

        let new = step(None);
        assert_eq!(new.mark, Mark::New);
        let Action::HyprlandConf { contents, .. } = &new.action else {
            panic!("{new:?}");
        };
        assert!(
            contents.contains("\nsource = /h/.local/share/koje/hyprland/koje.conf\n"),
            "{contents}"
        );

        let appended = step(Some("monitor = ,preferred,auto,1\n"));
        assert_eq!(appended.mark, Mark::Append);
        assert_eq!(
            appended.action,
            Action::SourceLine {
                path: conf.to_owned(),
                line: "source = /h/.local/share/koje/hyprland/koje.conf".into()
            }
        );
        // Commented out: the user turned it off, setup appends again – visible as ~.
        assert_eq!(
            step(Some(
                "monitor = ,preferred,auto,1\n# source = /h/.local/share/koje/hyprland/koje.conf\n"
            ))
            .mark,
            Mark::Append
        );
        assert_eq!(
            step(Some(
                "a = b\n  source = ~/.local/share/koje/hyprland/koje.conf  \n"
            ))
            .mark,
            Mark::Unchanged
        );
        // What koje wrote is the user's from then on.
        assert_eq!(step(Some(contents)).mark, Mark::Unchanged);
    }

    #[test]
    fn hyprland_conf_replaced_only_when_nothing_is_the_users() {
        let conf = Path::new("/h/.config/hypr/hyprland.conf");
        let snippet = Path::new("/h/.local/share/koje/hyprland/koje.conf");
        let step = |text: &str| hyprland_source(conf, snippet, Path::new("/h"), Some(text), &us());
        for text in [
            "",
            // Written by an earlier koje setup.
            "# Hyprland config – your own settings go here.\n\n# added by koje setup\nsource = /h/.local/share/koje/hyprland/koje.conf\n",
            "source = ~/.local/share/koje/hyprland/koje.conf\n",
            // Hyprland's generated default, with koje's line appended.
            "autogenerated = 1 # remove this line\nbind = $mainMod, Q, exec, kitty\nsource = ~/.local/share/koje/hyprland/koje.conf\n",
        ] {
            let s = step(text);
            assert_eq!(s.mark, Mark::Overwrite, "{text}");
            assert!(matches!(s.action, Action::HyprlandConf { .. }), "{text}");
        }
        // Default config with the warning turned off: the user's now.
        assert_eq!(
            step("# autogenerated = 1\nbind = SUPER, Q, exec, kitty\n").mark,
            Mark::Append
        );
    }

    #[test]
    fn keyboard_from_the_system() {
        let kb = |t: &str| system_keyboard(Some(t));
        assert_eq!(
            kb("XKBMODEL=\"pc105\"\nXKBLAYOUT=\"us\"\nXKBVARIANT=\"colemak\"\nXKBOPTIONS=\"\"\n"),
            Keyboard {
                layout: "us,us".into(),
                variant: "colemak,".into(),
                options: String::new()
            },
            "plain us to switch to"
        );
        assert_eq!(kb("XKBLAYOUT=\"de\"\n").layout, "de,us");
        assert_eq!(
            kb("XKBLAYOUT=us\nXKBOPTIONS=\"caps:escape\"\n"),
            Keyboard {
                layout: "us".into(),
                variant: String::new(),
                options: "caps:escape".into()
            }
        );
        assert_eq!(
            kb("XKBLAYOUT=\"us,de\"\nXKBVARIANT=\",nodeadkeys\"").variant,
            ",nodeadkeys"
        );
        assert_eq!(system_keyboard(None).layout, "us");

        let conf = hyprland_conf(
            "source = x",
            &kb("XKBLAYOUT=us\nXKBVARIANT=colemak\nXKBOPTIONS=caps:escape\n"),
        );
        assert!(conf.contains("    kb_layout = us,us\n    kb_variant = colemak,\n    kb_options = caps:escape\n}\n"), "{conf}");
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

        let fresh = waybar(dir, &ours, &[], false);
        assert_eq!(
            fresh.iter().map(|s| s.mark).collect::<Vec<_>>(),
            [Mark::New, Mark::New, Mark::New],
            "with the context colors kojed fills later"
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
            true,
        );
        assert_eq!(
            update.iter().map(|s| s.mark).collect::<Vec<_>>(),
            [Mark::Overwrite, Mark::Unchanged, Mark::Unchanged],
            "kojed's colors are left alone"
        );

        let own = waybar(dir, &ours, &[("config", WaybarFile::Foreign)], false);
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].mark, Mark::Skipped);
    }

    #[test]
    fn waybar_context_colors() {
        assert_eq!(waybar_class("kp"), "kp");
        assert_eq!(waybar_class("kunden-portal"), "kunden-portal");
        assert_eq!(waybar_class("a--b-"), "a-b");
        assert_eq!(waybar_class("1"), "ws-1");
        assert_eq!(waybar_class("special:Stash_kp"), "special-stash-kp");

        let ctx = |s: &str| ContextName::new(s).unwrap();
        let red = Color::rgb(0xff, 0, 0);
        let css = waybar_colors(&BTreeMap::from([(ctx("kp"), red)]));
        assert!(is_koje_waybar(css.as_bytes()), "{css}");
        assert!(
            css.ends_with(
                "#workspaces button.kp.active:not(.taskbar-window) { border-bottom-color: #FF0000; }\n\
                 #workspaces.tasks button.kp .taskbar-window.active { border-bottom-color: #FF0000; }\n"
            ),
            "only the active workspace and the focused window: {css}"
        );
        assert_eq!(waybar_colors(&BTreeMap::new()), WAYBAR_COLORS_HEADER);
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

    /// A plain Hyprland session gets its services from the legacy file; the
    /// koje session's snippet has none (ADR 0048: systemd units).
    #[test]
    fn session_services_are_the_legacy_snippets() {
        let legacy = include_str!("../../../assets/hyprland/legacy-services.conf");
        let snippet = include_str!("../../../assets/hyprland/koje.conf");
        for s in SESSION {
            let line = format!("exec-once = {}", s.command);
            assert!(legacy.lines().any(|l| l == line), "{line}");
        }
        assert!(!snippet.lines().any(|l| l.starts_with("exec-once")));
    }

    /// Super+F1 lists the bindings by their descriptions: none of koje's may lack one.
    #[test]
    fn snippet_bindings_have_descriptions() {
        let snippet = include_str!("../../../assets/hyprland/koje.conf");
        let binds: Vec<&str> = snippet.lines().filter(|l| l.starts_with("bind")).collect();
        assert!(binds.iter().any(|l| l.contains("SUPER, F1,")));
        for line in binds {
            let flags = line.split_once(' ').map_or(line, |(b, _)| b);
            assert!(flags.contains('d'), "{line}");
        }
    }

    #[test]
    fn the_koje_session_starts_its_services_itself() {
        let all = vec![
            Refresh::Hyprland,
            Refresh::Waybar,
            Refresh::Start(SESSION[0]),
            Refresh::Restart("kojed"),
        ];
        assert_eq!(for_koje_session(all), [Refresh::Hyprland, Refresh::Session]);
        assert_eq!(
            for_koje_session(vec![Refresh::Hyprland, Refresh::Waybar]),
            [Refresh::Hyprland]
        );
    }

    #[test]
    fn an_upgrade_reloads_the_whole_koje_session() {
        assert_eq!(
            for_koje_session(vec![Refresh::Restart("koje-shell")]),
            [Refresh::Hyprland, Refresh::Session]
        );
        assert_eq!(for_koje_session(vec![Refresh::Waybar]), []);
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
        // The title bar snippet next to it: a reload too.
        assert_eq!(
            refresh(
                &[write("/d/koje/hyprland/hyprbars.conf")],
                snippet,
                waybar,
                &all
            ),
            [Refresh::Hyprland]
        );

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
            Some("a = b\n"),
            &system_keyboard(None),
        );
        assert_eq!(
            refresh(&[source], snippet, waybar, &all),
            [Refresh::Hyprland]
        );
    }
}
