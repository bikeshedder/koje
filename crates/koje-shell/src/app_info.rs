//! A window's app, as GNOME finds it (`app` is an app id or a desktop file
//! id): the desktop file named after its app
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
    let info = gio_unix::DesktopAppInfo::new(app).or_else(|| find(app));
    if let Some(icon) = info.and_then(|i| i.icon()) {
        return icon;
    }
    let theme = gtk4::gdk::Display::default().map(|d| gtk4::IconTheme::for_display(&d));
    if !app.is_empty() && theme.is_some_and(|t| t.has_icon(app)) {
        return gio::ThemedIcon::new(app).upcast();
    }
    gio::ThemedIcon::new("application-x-executable").upcast()
}

thread_local! {
    /// App id → desktop file id: looked up once per app id.
    static IDS: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> =
        std::cell::RefCell::default();
}

/// The desktop file id (`firefox.desktop`) of an app id, if the host has one.
pub fn desktop_id(app: &str) -> Option<String> {
    IDS.with(|ids| {
        ids.borrow_mut()
            .entry(app.to_owned())
            .or_insert_with(|| find(app).and_then(|i| i.id()).map(String::from))
            .clone()
    })
}

/// A desktop file's name, else the app id.
pub fn name(app: &str) -> String {
    gio_unix::DesktopAppInfo::new(app)
        .or_else(|| find(app))
        .map_or_else(|| app.to_owned(), |i| i.name().to_string())
}

/// GNOME Shell's settings, if its schema is installed.
pub fn shell_settings() -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup("org.gnome.shell", true)?;
    schema
        .has_key("favorite-apps")
        .then(|| gio::Settings::new("org.gnome.shell"))
}

/// Add a desktop file id to GNOME's favorites, or remove it.
pub fn set_favorite(id: &str, favorite: bool) {
    let Some(settings) = shell_settings() else {
        return;
    };
    let mut list: Vec<String> = settings
        .strv("favorite-apps")
        .iter()
        .map(ToString::to_string)
        .collect();
    list.retain(|f| f != id);
    if favorite {
        list.push(id.to_owned());
    }
    if let Err(e) = settings.set_strv("favorite-apps", list) {
        tracing::warn!("changing GNOME's favorites: {e}");
    }
}

/// The host's programs as GNOME lists them (`should_show`: not hidden, for
/// this desktop).
pub fn host_apps() -> Vec<crate::search::AppItem> {
    gio::AppInfo::all()
        .into_iter()
        .filter(gio::prelude::AppInfoExt::should_show)
        .filter_map(|a| a.downcast::<gio_unix::DesktopAppInfo>().ok())
        .filter_map(|d| {
            Some(crate::search::AppItem {
                name: d.name().to_string(),
                id: d.id()?.to_string(),
                icon: d
                    .icon()
                    .and_then(|i| gio::prelude::IconExt::to_string(&i))
                    .map(String::from),
                keywords: d
                    .generic_name()
                    .into_iter()
                    .map(String::from)
                    .chain(d.keywords().iter().map(ToString::to_string))
                    .collect(),
                terminal: false,
            })
        })
        .collect()
}
