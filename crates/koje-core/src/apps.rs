//! The programs of a context for koje's application menu (M8.17): the
//! `.desktop` entries in the context's data directories. Pure: the CLI runs
//! [`LIST_SCRIPT`] in the context and hands its output to [`parse_listing`].
//!
//! koje runs an entry's `Exec` command itself. `gio launch` asks the bus to
//! start D-Bus activatable programs (Nautilus) instead, which did not open
//! anything in a context.

use std::collections::{BTreeMap, BTreeSet};

/// Separates the files in [`LIST_SCRIPT`]'s output (ASCII record separator).
const RECORD: char = '\u{1e}';

/// Prints every `*.desktop` file of the XDG data directories, user's first:
/// a record separator, the path, a newline, the file.
pub const LIST_SCRIPT: &str = r#"IFS=:
for d in "${XDG_DATA_HOME:-$HOME/.local/share}" ${XDG_DATA_DIRS:-/usr/local/share:/usr/share}; do
  for f in "$d"/applications/*.desktop; do
    [ -f "$f" ] && printf '\036%s\n' "$f" && cat "$f"
  done
done
exit 0"#;

/// koje's desktop for `OnlyShowIn` / `NotShowIn`.
const DESKTOP: &str = "Hyprland";

/// A program in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// What the menu shows; unique (the id is added to equal names).
    pub name: String,
    /// Desktop file id, e.g. `firefox.desktop`.
    pub id: String,
    /// The file in the context.
    pub path: String,
    /// `Exec` as a command line, field codes resolved (no files or URLs).
    pub command: Vec<String>,
    /// `Terminal=true`: runs inside a terminal.
    pub terminal: bool,
}

/// What koje uses of a menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub terminal: bool,
}

/// The `[Desktop Entry]` group of a desktop file, if it belongs in a menu:
/// an application with a name and a command that is not hidden, not
/// `NoDisplay` and shown on koje's desktop.
pub fn menu_entry(text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let mut keys = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim()).or_insert(v.trim());
        }
    }
    let is = |k: &str| keys.get(k).is_some_and(|v| *v == "true");
    let lists_us = |k: &str| keys.get(k).map(|v| v.split(';').any(|d| d == DESKTOP));
    let shown = lists_us("OnlyShowIn").unwrap_or(true) && !lists_us("NotShowIn").unwrap_or(false);
    let name = keys.get("Name").filter(|n| !n.is_empty())?;
    let exec = keys.get("Exec").filter(|e| !e.is_empty())?;
    (keys.get("Type") == Some(&"Application") && !is("NoDisplay") && !is("Hidden") && shown).then(
        || Entry {
            name: (*name).to_owned(),
            exec: (*exec).to_owned(),
            icon: keys.get("Icon").map(|i| (*i).to_owned()),
            terminal: is("Terminal"),
        },
    )
}

/// `Exec` as a command line (Desktop Entry spec): the string escapes, then
/// quoting, then field codes. Nothing is opened, so file and URL codes go;
/// `%i`, `%c`, `%k` become the icon, name and file. `None` for a broken line.
pub fn exec_command(entry: &Entry, path: &str) -> Option<Vec<String>> {
    // String escapes of the file format.
    let mut text = String::new();
    let mut chars = entry.exec.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => text.push(' '),
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('r') => text.push('\r'),
            Some('\\') | None => text.push('\\'),
            Some(o) => {
                text.push('\\');
                text.push(o);
            }
        }
    }
    // Arguments: split at spaces; "…" quotes, with \" \` \$ \\ inside.
    let mut args: Vec<(String, bool)> = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else { break };
        let mut arg = String::new();
        if first == '"' {
            chars.next();
            loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => arg.push(chars.next()?),
                    c => arg.push(c),
                }
            }
            args.push((arg, true));
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                arg.push(c);
            }
            args.push((arg, false));
        }
    }
    let mut out = Vec::new();
    for (arg, quoted) in args {
        match (arg.as_str(), quoted) {
            ("%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m", false) => {}
            ("%i", false) => {
                if let Some(icon) = &entry.icon {
                    out.extend(["--icon".to_owned(), icon.clone()]);
                }
            }
            ("%c", false) => out.push(entry.name.clone()),
            ("%k", false) => out.push(path.to_owned()),
            _ => {
                // Codes inside an argument: %% is a percent sign, others go.
                let mut a = String::new();
                let mut cs = arg.chars();
                while let Some(c) = cs.next() {
                    if c != '%' {
                        a.push(c);
                    } else if let Some(n) = cs.next()
                        && n == '%'
                    {
                        a.push('%');
                    }
                }
                out.push(a);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The menu from [`LIST_SCRIPT`]'s output, sorted by name. An id seen first
/// (the user's directory) hides later files with the same id, also when the
/// first one is hidden – that is how users remove an entry.
pub fn parse_listing(out: &str) -> Vec<App> {
    let mut seen = BTreeSet::new();
    let mut apps: Vec<App> = Vec::new();
    for record in out.split(RECORD).filter(|r| !r.is_empty()) {
        let (path, text) = record.split_once('\n').unwrap_or((record, ""));
        let id = path.rsplit('/').next().unwrap_or(path);
        if !seen.insert(id.to_owned()) {
            continue;
        }
        if let Some(entry) = menu_entry(text)
            && let Some(command) = exec_command(&entry, path)
        {
            apps.push(App {
                name: entry.name,
                id: id.to_owned(),
                path: path.to_owned(),
                command,
                terminal: entry.terminal,
            });
        }
    }
    apps.sort_by_key(|a| (a.name.to_lowercase(), a.id.clone()));
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for a in &apps {
        *count.entry(a.name.clone()).or_default() += 1;
    }
    for a in &mut apps {
        if count[&a.name] > 1 {
            a.name = format!("{} ({})", a.name, a.id.trim_end_matches(".desktop"));
        }
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\n{extra}")
    }

    fn menu_name(text: &str) -> Option<String> {
        menu_entry(text).map(|e| e.name)
    }

    #[test]
    fn menu_entries() {
        assert_eq!(menu_name(&entry("")).as_deref(), Some("Files"));
        assert_eq!(
            menu_entry(&entry("Terminal=true\nIcon=nautilus")),
            Some(Entry {
                name: "Files".into(),
                exec: "nautilus %U".into(),
                icon: Some("nautilus".into()),
                terminal: true
            })
        );
        for hidden in [
            "NoDisplay=true",
            "Hidden=true",
            "OnlyShowIn=GNOME;",
            "NotShowIn=GNOME;Hyprland;",
        ] {
            assert_eq!(menu_name(&entry(hidden)), None, "{hidden}");
        }
        assert_eq!(
            menu_name(&entry("OnlyShowIn=GNOME;Hyprland;")).as_deref(),
            Some("Files")
        );
        assert_eq!(
            menu_name("[Desktop Entry]\nType=Link\nName=x\nURL=y\n"),
            None
        );
        // Only the main group counts; localized names are not the plain one.
        let actions = format!(
            "{}\nName[de]=Dateien\n[Desktop Action new]\nName=New Window\nNoDisplay=true\n",
            entry("")
        );
        assert_eq!(menu_name(&actions).as_deref(), Some("Files"));
    }

    #[test]
    fn listing() {
        let rec = |path: &str, text: &str| format!("{RECORD}{path}\n{text}");
        let out = [
            rec(
                "/h/.local/share/applications/hidden.desktop",
                &entry("Hidden=true"),
            ),
            rec("/usr/share/applications/hidden.desktop", &entry("")),
            rec(
                "/usr/share/applications/org.gnome.Nautilus.desktop",
                &entry(""),
            ),
            rec(
                "/usr/share/applications/firefox.desktop",
                "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n",
            ),
            rec(
                "/usr/share/applications/foot.desktop",
                &entry("").replace("Files", "Foot"),
            ),
            rec(
                "/usr/share/applications/footclient.desktop",
                &entry("").replace("Files", "Foot"),
            ),
        ]
        .concat();
        let names: Vec<String> = parse_listing(&out).into_iter().map(|a| a.name).collect();
        assert_eq!(
            names,
            ["Files", "Firefox", "Foot (foot)", "Foot (footclient)"],
            "sorted, the user's hidden entry hides the system's, equal names get the id"
        );
        let firefox = &parse_listing(&out)[1];
        assert_eq!(firefox.id, "firefox.desktop");
        assert_eq!(firefox.path, "/usr/share/applications/firefox.desktop");
        assert_eq!(firefox.command, ["firefox"]);
        assert_eq!(parse_listing(""), []);
    }

    #[test]
    fn exec_lines() {
        let cmd = |exec: &str| {
            let e = Entry {
                name: "My App".into(),
                exec: exec.into(),
                icon: Some("my-icon".into()),
                terminal: false,
            };
            exec_command(&e, "/usr/share/applications/my.desktop")
        };
        let v = |a: &[&str]| Some(a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        // Nautilus, as Fedora ships it.
        assert_eq!(
            cmd("nautilus --new-window %U"),
            v(&["nautilus", "--new-window"])
        );
        assert_eq!(
            cmd(r#"sh -c "echo \"\$HOME\" 100%%" %f"#),
            v(&["sh", "-c", r#"echo "$HOME" 100%"#]),
            "quotes, escapes, %%"
        );
        assert_eq!(
            cmd(r#""app name" --x=%u %i %c %k"#),
            v(&[
                "app name",
                "--x=",
                "--icon",
                "my-icon",
                "My App",
                "/usr/share/applications/my.desktop"
            ])
        );
        assert_eq!(cmd(r#"broken "quote"#), None);
        assert_eq!(cmd("%U"), None, "nothing left to run");
    }
}
