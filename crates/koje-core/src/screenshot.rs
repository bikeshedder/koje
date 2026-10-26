//! `koje screenshot` as GNOME takes them: a file in the Pictures folder's
//! `Screenshots` named by date and time, the image on the clipboard, a
//! notification. Pure: the CLI runs grim, slurp and wl-copy.

use std::path::{Path, PathBuf};

/// What to capture: GNOME's Print (interactive – an area until koje's own
/// screenshot UI, M8.48), Shift+Print (the screen), Alt+Print (the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Area,
    Screen,
    Window,
}

/// The Pictures folder from `~/.config/user-dirs.dirs`
/// (`XDG_PICTURES_DIR="$HOME/Pictures"`), else `~/Pictures`.
pub fn pictures_dir(user_dirs: Option<&str>, home: &Path) -> PathBuf {
    let configured = user_dirs.and_then(|text| {
        text.lines().find_map(|l| {
            let value = l.trim().strip_prefix("XDG_PICTURES_DIR=")?;
            let value = value.trim().trim_matches('"');
            Some(match value.strip_prefix("$HOME") {
                Some(rest) => home.join(rest.trim_start_matches('/')),
                None => PathBuf::from(value),
            })
        })
    });
    configured
        .filter(|p| p.is_absolute() && p != home)
        .unwrap_or_else(|| home.join("Pictures"))
}

/// GNOME's file name: `Screenshot From 2026-10-26 15-40-12.png`; `stamp` is
/// the local date and time as `%Y-%m-%d %H-%M-%S`.
pub fn file_name(stamp: &str) -> String {
    format!("Screenshot From {}.png", stamp.trim())
}

/// grim's geometry for a window at `x,y` of `w`×`h`.
pub fn geometry(x: i32, y: i32, w: i32, h: i32) -> String {
    format!("{x},{y} {w}x{h}")
}

/// GNOME Shell's notification.
pub const SUMMARY: &str = "Screenshot captured";
pub const BODY: &str = "You can paste the image from the clipboard.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pictures_folder() {
        let home = Path::new("/h");
        let dirs =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\n";
        assert_eq!(pictures_dir(Some(dirs), home), PathBuf::from("/h/Bilder"));
        assert_eq!(
            pictures_dir(Some("XDG_PICTURES_DIR=\"/data/pics\""), home),
            PathBuf::from("/data/pics")
        );
        // `$HOME/` alone means "no Pictures folder": GNOME falls back too.
        assert_eq!(
            pictures_dir(Some("XDG_PICTURES_DIR=\"$HOME/\""), home),
            PathBuf::from("/h/Pictures")
        );
        assert_eq!(pictures_dir(None, home), PathBuf::from("/h/Pictures"));
    }

    #[test]
    fn names_and_geometry() {
        assert_eq!(
            file_name("2026-10-26 15-40-12\n"),
            "Screenshot From 2026-10-26 15-40-12.png"
        );
        assert_eq!(geometry(10, -20, 800, 600), "10,-20 800x600");
    }
}
