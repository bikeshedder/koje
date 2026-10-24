//! The programs of a context for koje's application menu (M8.17): the
//! `.desktop` entries in the context's data directories. Pure: the CLI runs
//! [`LIST_SCRIPT`] in the context and hands its output to [`parse_listing`].

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
    /// The file in the context, for `gio launch`.
    pub path: String,
}

/// The `[Desktop Entry]` group of a desktop file, if it belongs in a menu:
/// an application with a name that is not hidden, not `NoDisplay` and shown
/// on koje's desktop. Returns its name.
pub fn menu_name(text: &str) -> Option<String> {
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
    (keys.get("Type") == Some(&"Application")
        && !is("NoDisplay")
        && !is("Hidden")
        && keys.contains_key("Exec")
        && shown)
        .then(|| (*name).to_owned())
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
        if let Some(name) = menu_name(text) {
            apps.push(App {
                name,
                id: id.to_owned(),
                path: path.to_owned(),
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

    #[test]
    fn menu_entries() {
        assert_eq!(menu_name(&entry("")).as_deref(), Some("Files"));
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
        assert_eq!(parse_listing(""), []);
    }
}
