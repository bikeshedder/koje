//! The koje session (ADR 0048): what `koje session start` hands to Hyprland,
//! what `koje session ready` passes to systemd once Hyprland runs, and what is
//! undone when it exits. Pure: the CLI runs the commands.

use std::path::Path;

/// The session's own systemd target (bound to `graphical-session.target`).
pub const TARGET: &str = "koje-session.target";

/// koje's snippet: the session's look, bindings and workspaces (package,
/// ADR 0037, 0045).
pub const SNIPPET: &str = "/usr/share/koje/hyprland/koje.conf";

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

/// systemd's generator for `~/.config/environment.d/*.conf` (and the system's
/// `environment.d`): it prints the environment with those files applied.
pub const ENVIRONMENT_D: &str =
    "/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator";

/// The generator's `KEY=VALUE` lines; anything else is skipped.
pub fn parse_environment(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
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

/// The session's Hyprland config, generated at login into the runtime
/// directory (ADR 0048): koje's snippet, then the monitors from
/// `monitors.xml` (ADR 0046), then the user's own file – each only when
/// given; later settings win.
pub fn session_conf(snippet: &Path, monitors: Option<&Path>, user: Option<&Path>) -> String {
    use std::fmt::Write as _;
    let mut conf = String::from(
        "# The koje session's Hyprland config (ADR 0048), written by `koje session\n\
         # start` at every login – edits are lost. Your own settings go into\n\
         # ~/.config/koje/hyprland.conf; GNOME Settings is the place for most.\n",
    );
    let mut source = |p: &Path| {
        let _ = writeln!(conf, "source = {}", p.display());
    };
    source(snippet);
    if let Some(m) = monitors {
        source(m);
    }
    if let Some(u) = user {
        source(u);
    }
    conf.push_str("\n# Hand the environment to systemd and start koje's services.\nexec-once = koje session ready\n");
    conf
}

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
    fn environment_d_lines() {
        let env = parse_environment(
            "LIBVA_DRIVER_NAME=nvidia\nPATH=/a:/b\nno equals\n=x\nBAD-KEY=1\nEMPTY=\n",
        );
        assert_eq!(
            env,
            [
                ("LIBVA_DRIVER_NAME".to_owned(), "nvidia".to_owned()),
                ("PATH".to_owned(), "/a:/b".to_owned()),
                ("EMPTY".to_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn the_desktop_is_koje_and_gnome() {
        assert_eq!(current_desktop(None), "koje:GNOME");
        assert_eq!(current_desktop(Some("koje:GNOME")), "koje:GNOME");
        assert_eq!(current_desktop(Some("Hyprland")), "koje:GNOME:Hyprland");
        assert_eq!(current_desktop(Some("GNOME:koje")), "koje:GNOME");
        assert_eq!(current_desktop(Some("")), "koje:GNOME");
    }

    #[test]
    fn the_session_config_sources_what_there_is() {
        let full = session_conf(
            Path::new(SNIPPET),
            Some(Path::new("/run/user/1000/koje/monitors.conf")),
            Some(Path::new("/h/.config/koje/hyprland.conf")),
        );
        let sources: Vec<&str> = full.lines().filter(|l| l.starts_with("source")).collect();
        assert_eq!(
            sources,
            [
                "source = /usr/share/koje/hyprland/koje.conf",
                "source = /run/user/1000/koje/monitors.conf",
                "source = /h/.config/koje/hyprland.conf"
            ]
        );
        assert!(full.contains("\nexec-once = koje session ready\n"));
        let bare = session_conf(Path::new(SNIPPET), None, None);
        assert_eq!(bare.lines().filter(|l| l.starts_with("source")).count(), 1);
    }

    #[test]
    fn hyprland_gets_kojes_config() {
        assert_eq!(
            hyprland_args(Path::new("/run/user/1000/koje/session.conf")),
            ["--config", "/run/user/1000/koje/session.conf"]
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
