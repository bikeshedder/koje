//! The filtered host bus of a context (ADR 0019, 0025): which names the
//! forwarder owns in the context and which `xdg-dbus-proxy` rules let them
//! through on the host. Pure; derived from the effective access. The rules are
//! those found in the spikes M2.4–M2.11 (`docs/spikes/setup/host-bus-proxy.sh`).

use crate::access::Access;

pub const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
const DESKTOP: &str = "/org/freedesktop/portal/desktop";

/// Whether the context needs the host bus at all.
pub fn needed(a: &Access) -> bool {
    a.notifications || portal_needed(a)
}

fn portal_needed(a: &Access) -> bool {
    a.file_chooser || a.settings || a.inhibit || a.screencast || a.print
}

/// The names the forwarder owns on the context bus.
pub fn forwarded_names(a: &Access) -> Vec<&'static str> {
    let mut names = Vec::new();
    if a.notifications {
        names.push(NOTIFICATIONS);
    }
    if portal_needed(a) {
        names.push(PORTAL);
    }
    names
}

/// `xdg-dbus-proxy` arguments after the bus addresses: `--filter` plus one rule
/// per allowed interface. Everything not listed is denied, in particular
/// `OpenURI` and `host.portal.Registry` (a context must not claim app ids).
pub fn proxy_rules(a: &Access) -> Vec<String> {
    let mut r = vec!["--filter".to_owned()];
    if a.notifications {
        r.push(format!("--talk={NOTIFICATIONS}"));
    }
    if !portal_needed(a) {
        return r;
    }
    let call = |what: &str, at: &str| format!("--call={PORTAL}={what}@{at}");
    let broadcast = |what: &str, at: &str| format!("--broadcast={PORTAL}={what}@{at}");
    let request = format!("{DESKTOP}/request/*");
    let session = format!("{DESKTOP}/session/*");
    // GDBusProxy clients read all properties and introspect; a denied GetAll
    // reads as "portal missing" (M2.10). Wildcards only as `Interface.*`.
    r.push(call("org.freedesktop.DBus.Properties.Get", DESKTOP));
    r.push(call("org.freedesktop.DBus.Properties.GetAll", DESKTOP));
    r.push(call(
        "org.freedesktop.DBus.Introspectable.Introspect",
        DESKTOP,
    ));
    for (on, iface) in [
        (a.file_chooser, "FileChooser"),
        (a.settings, "Settings"),
        (a.inhibit, "Inhibit"),
        (a.screencast, "ScreenCast"),
        (a.print, "Print"),
    ] {
        if on {
            r.push(call(&format!("org.freedesktop.portal.{iface}.*"), DESKTOP));
        }
    }
    if a.settings {
        r.push(broadcast(
            "org.freedesktop.portal.Settings.SettingChanged",
            DESKTOP,
        ));
    }
    r.push(call("org.freedesktop.portal.Request.*", &request));
    r.push(broadcast(
        "org.freedesktop.portal.Request.Response",
        &request,
    ));
    if a.screencast || a.inhibit {
        r.push(call("org.freedesktop.portal.Session.*", &session));
        r.push(broadcast("org.freedesktop.portal.Session.Closed", &session));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Profile;

    #[test]
    fn full_access_matches_the_spike_filter() {
        let a = Profile::Full.access();
        assert!(needed(&a));
        assert_eq!(forwarded_names(&a), [NOTIFICATIONS, PORTAL]);
        let d = "/org/freedesktop/portal/desktop";
        let p = "org.freedesktop.portal.Desktop";
        assert_eq!(
            proxy_rules(&a),
            [
                "--filter".to_owned(),
                "--talk=org.freedesktop.Notifications".to_owned(),
                format!("--call={p}=org.freedesktop.DBus.Properties.Get@{d}"),
                format!("--call={p}=org.freedesktop.DBus.Properties.GetAll@{d}"),
                format!("--call={p}=org.freedesktop.DBus.Introspectable.Introspect@{d}"),
                format!("--call={p}=org.freedesktop.portal.FileChooser.*@{d}"),
                format!("--call={p}=org.freedesktop.portal.Settings.*@{d}"),
                format!("--call={p}=org.freedesktop.portal.Inhibit.*@{d}"),
                format!("--call={p}=org.freedesktop.portal.ScreenCast.*@{d}"),
                format!("--call={p}=org.freedesktop.portal.Print.*@{d}"),
                format!("--broadcast={p}=org.freedesktop.portal.Settings.SettingChanged@{d}"),
                format!("--call={p}=org.freedesktop.portal.Request.*@{d}/request/*"),
                format!("--broadcast={p}=org.freedesktop.portal.Request.Response@{d}/request/*"),
                format!("--call={p}=org.freedesktop.portal.Session.*@{d}/session/*"),
                format!("--broadcast={p}=org.freedesktop.portal.Session.Closed@{d}/session/*"),
            ]
        );
    }

    #[test]
    fn never_open_uri_or_registry() {
        let rules = proxy_rules(&Profile::Full.access()).join(" ");
        assert!(
            !rules.contains("OpenURI") && !rules.contains("Registry"),
            "{rules}"
        );
    }

    #[test]
    fn headless_only_notifications() {
        let a = Profile::Headless.access();
        assert_eq!(forwarded_names(&a), [NOTIFICATIONS]);
        assert_eq!(
            proxy_rules(&a),
            ["--filter", "--talk=org.freedesktop.Notifications"]
        );
    }

    #[test]
    fn isolated_needs_no_host_bus() {
        let a = Profile::Isolated.access();
        assert!(!needed(&a));
        assert_eq!(forwarded_names(&a), Vec::<&str>::new());
    }

    #[test]
    fn single_switches() {
        let only = |f: fn(&mut Access)| {
            let mut a = Access::NONE;
            f(&mut a);
            proxy_rules(&a).join(" ")
        };
        let fc = only(|a| a.file_chooser = true);
        assert!(
            fc.contains("FileChooser") && !fc.contains("ScreenCast") && !fc.contains("Session.")
        );
        let sc = only(|a| a.screencast = true);
        assert!(sc.contains("ScreenCast") && sc.contains("Session.*") && !sc.contains("Settings"));
        let st = only(|a| a.settings = true);
        assert!(st.contains("SettingChanged") && !st.contains("Notifications"));
    }
}
