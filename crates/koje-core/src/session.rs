//! The koje session (ADR 0048): what `koje session start` hands to Hyprland,
//! what `koje session ready` passes to systemd once Hyprland runs, and what is
//! undone when it exits. Pure: the CLI runs the commands.

use std::path::Path;

/// The session's own systemd target (bound to `graphical-session.target`).
pub const TARGET: &str = "koje-session.target";

/// koje's Hyprland config for the session (package, ADR 0045).
pub const SESSION_CONF: &str = "/usr/share/koje/hyprland/session.conf";

/// The compositor.
pub const HYPRLAND: &str = "Hyprland";

/// What programs started by systemd and D-Bus need from the session: the
/// display, the desktop's identity, Hyprland's instance.
pub const ENVIRONMENT: [&str; 6] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_DESKTOP",
    "HYPRLAND_INSTANCE_SIGNATURE",
];

/// `XDG_CURRENT_DESKTOP` for the session: `koje:GNOME` as the display manager
/// sets it from `koje.desktop`; a value from elsewhere (a start from a TTY,
/// a nested test) keeps its names and gets the missing ones.
pub fn current_desktop(given: Option<&str>) -> String {
    let mut names: Vec<&str> = given
        .unwrap_or_default()
        .split(':')
        .filter(|n| !n.is_empty() && *n != "koje" && *n != "GNOME")
        .collect();
    names.insert(0, "GNOME");
    names.insert(0, "koje");
    names.join(":")
}

/// Variables `koje session start` sets for Hyprland and everything in it.
pub fn session_environment(given_desktop: Option<&str>) -> Vec<(&'static str, String)> {
    vec![
        ("XDG_CURRENT_DESKTOP", current_desktop(given_desktop)),
        ("XDG_SESSION_DESKTOP", "koje".to_owned()),
        ("XDG_SESSION_TYPE", "wayland".to_owned()),
    ]
}

/// The arguments to start Hyprland with koje's config.
pub fn hyprland_args(conf: &Path) -> Vec<String> {
    vec!["--config".to_owned(), conf.display().to_string()]
}

/// The user's own settings, sourced last by `session.conf`: created empty
/// with this text when missing, so Hyprland finds the file.
pub const USER_CONF_TEMPLATE: &str = "\
# Your settings for the koje session (ADR 0048): this machine's screens,
# keyboard, your own bindings. Read after koje's – later settings win;
# `unbind = MODS, key` removes one of koje's bindings.
# Screens and modes: hyprctl monitors all. Example:
# monitor = DP-1, 5120x2160@120, 0x0, 1
";

/// One command: program and arguments.
pub type Cmd = (&'static str, Vec<String>);

/// `koje session ready` (from Hyprland's `exec-once`): hand the environment to
/// systemd and D-Bus, then start the session's services.
pub fn ready_commands() -> Vec<Cmd> {
    let mut import = vec!["--systemd".to_owned()];
    import.extend(ENVIRONMENT.iter().map(|v| (*v).to_owned()));
    vec![
        ("dbus-update-activation-environment", import),
        (
            "systemctl",
            ["--user", "start", "--no-block", TARGET]
                .map(str::to_owned)
                .to_vec(),
        ),
    ]
}

/// After Hyprland exited: stop the services and take back the environment,
/// so nothing started later believes the session still runs.
pub fn end_commands() -> Vec<Cmd> {
    let mut unset = vec!["--user".to_owned(), "unset-environment".to_owned()];
    unset.extend(ENVIRONMENT.iter().map(|v| (*v).to_owned()));
    vec![
        (
            "systemctl",
            ["--user", "stop", TARGET].map(str::to_owned).to_vec(),
        ),
        ("systemctl", unset),
    ]
}

/// `koje session restart`: koje's services again, the session goes on.
pub fn restart_commands() -> Vec<Cmd> {
    vec![(
        "systemctl",
        ["--user", "restart", TARGET].map(str::to_owned).to_vec(),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_is_koje_and_gnome() {
        assert_eq!(current_desktop(None), "koje:GNOME");
        assert_eq!(current_desktop(Some("koje:GNOME")), "koje:GNOME");
        assert_eq!(current_desktop(Some("Hyprland")), "koje:GNOME:Hyprland");
        assert_eq!(current_desktop(Some("GNOME:koje")), "koje:GNOME");
        assert_eq!(current_desktop(Some("")), "koje:GNOME");
    }

    #[test]
    fn hyprland_gets_kojes_config() {
        assert_eq!(
            hyprland_args(Path::new(SESSION_CONF)),
            ["--config", "/usr/share/koje/hyprland/session.conf"]
        );
        let env = session_environment(Some("koje:GNOME"));
        assert!(env.contains(&("XDG_SESSION_DESKTOP", "koje".to_owned())));
    }

    #[test]
    fn ready_imports_then_starts_and_the_end_undoes_it() {
        let ready = ready_commands();
        assert_eq!(ready[0].0, "dbus-update-activation-environment");
        assert!(
            ready[0]
                .1
                .contains(&"HYPRLAND_INSTANCE_SIGNATURE".to_owned())
        );
        assert_eq!(
            ready[1].1.join(" "),
            "--user start --no-block koje-session.target"
        );
        let end = end_commands();
        assert_eq!(end[0].1.join(" "), "--user stop koje-session.target");
        assert_eq!(
            end[1].1[..3].join(" "),
            "--user unset-environment WAYLAND_DISPLAY"
        );
        assert_eq!(end[1].1.len(), 2 + ENVIRONMENT.len());
        assert_eq!(
            restart_commands()[0].1.join(" "),
            "--user restart koje-session.target"
        );
    }
}
