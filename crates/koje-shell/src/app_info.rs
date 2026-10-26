//! A window's app, as GNOME finds it: the desktop file named after its app
//! id, else the best search match; its icon, else the icon theme's icon of
//! that name, else a generic one. Programs in contexts have their desktop
//! files inside the container: they get the generic icon until the shell
//! reads them (M8.18).

use gtk4::gio;
use gtk4::prelude::*;

pub fn find(app: &str) -> Option<gio_unix::DesktopAppInfo> {
    if app.is_empty() {
        return None;
    }
    let lower = app.to_lowercase();
    for id in [app, lower.as_str()] {
        if let Some(info) = gio_unix::DesktopAppInfo::new(&format!("{id}.desktop")) {
            return Some(info);
        }
    }
    gio_unix::DesktopAppInfo::search(app)
        .into_iter()
        .flatten()
        .find_map(|id| gio_unix::DesktopAppInfo::new(&id))
}

pub fn icon(app: &str) -> gio::Icon {
    if let Some(icon) = find(app).and_then(|i| i.icon()) {
        return icon;
    }
    let theme = gtk4::gdk::Display::default().map(|d| gtk4::IconTheme::for_display(&d));
    if !app.is_empty() && theme.is_some_and(|t| t.has_icon(app)) {
        return gio::ThemedIcon::new(app).upcast();
    }
    gio::ThemedIcon::new("application-x-executable").upcast()
}
