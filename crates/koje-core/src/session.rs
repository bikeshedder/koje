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

/// Hyprland's watchdog (since 0.53): starts it, restarts it in safe mode
/// after a crash; Hyprland warns when started without it.
pub const WATCHDOG: &str = "start-hyprland";

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

/// The arguments to start Hyprland with koje's config – through the
/// watchdog, which passes what follows `--` on.
pub fn hyprland_args(conf: &Path, watchdog: bool) -> Vec<String> {
    let mut args = Vec::new();
    if watchdog {
        args.push("--".to_owned());
    }
    args.extend(["--config".to_owned(), conf.display().to_string()]);
    args
}

/// koje's Hyprland plugin (ADR 0051), as the package installs it.
pub const PLUGIN: &str = "/usr/lib/koje/koje-hyprland.so";

/// The session's Hyprland config, generated at login into the runtime
/// directory (ADR 0048): koje's plugin (ADR 0051), koje's snippet, then the
/// monitors from `monitors.xml` (ADR 0046), then the user's own file – each
/// only when given; later settings win.
pub fn session_conf(
    plugin: Option<&Path>,
    snippet: &Path,
    monitors: Option<&Path>,
    user: Option<&Path>,
) -> String {
    use std::fmt::Write as _;
    let mut conf = String::from(
        "# The koje session's Hyprland config (ADR 0048), written by `koje session\n\
         # start` at every login – edits are lost. Your own settings go into\n\
         # ~/.config/koje/hyprland.conf; GNOME Settings is the place for most.\n",
    );
    if let Some(p) = plugin {
        let _ = writeln!(conf, "plugin = {}", p.display());
    }
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
    conf.push_str(
        "\n# `koje:GNOME` is intended (ADR 0048): no warning that it is not `Hyprland`.\n\
         misc:disable_xdg_env_checks = true\n\
         \n# Hand the environment to systemd and start koje's services.\n\
         exec-once = koje session ready\n",
    );
    conf
}

/// koje's screen sharing picker (ADR 0050), as xdph runs it.
pub const SHARE_PICKER: &str = "/usr/lib/koje/koje-share-picker";

/// xdph's config directory in the runtime directory: `XDG_CONFIG_HOME` for
/// xdph only (its file is `hypr/xdph.conf` below it).
pub const XDPH_DIR: &str = "xdph";

/// The environment file the package's drop-in for
/// `xdg-desktop-portal-hyprland.service` reads when it exists.
pub const XDPH_ENV: &str = "xdph.env";

/// xdph's config in the koje session (ADR 0050): koje's picker, then the
/// user's own settings (`~/.config/koje/xdph.conf`) – later ones win. Copied
/// in, not sourced: xdph's parser is not known to follow `source`.
pub fn xdph_conf(picker: &str, user: Option<&str>) -> String {
    let mut conf = format!(
        "# xdg-desktop-portal-hyprland's config in the koje session (ADR 0050),\n\
         # written by `koje session start` at every login – edits are lost. Your\n\
         # own settings go into ~/.config/koje/xdph.conf.\n\
         screencopy {{\n    custom_picker_binary = {picker}\n}}\n"
    );
    if let Some(user) = user {
        conf.push_str("\n# ~/.config/koje/xdph.conf\n");
        conf.push_str(user);
        if !user.ends_with('\n') {
            conf.push('\n');
        }
    }
    conf
}

/// The environment file for xdph: its config directory, and the user's own
/// `XDG_CONFIG_HOME` (empty when unset) for the picker to restore.
pub fn xdph_env(config_home: &Path, user_config_home: Option<&str>) -> String {
    format!(
        "XDG_CONFIG_HOME={}\nKOJE_USER_CONFIG_HOME={}\n",
        config_home.display(),
        user_config_home.unwrap_or_default()
    )
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

/// `koje session restart`: koje's services again, the session goes on –
/// with their unit files as they are now (a package update changes them).
/// Waybar and mako have their own units outside the target: a restart
/// meets koje's drop-ins and stops them (koje-shell is the bar and the
/// notification server, M8.36).
pub fn restart_commands() -> Vec<Cmd> {
    let systemctl = |args: &[&str]| {
        let mut all = vec!["--user".to_owned()];
        all.extend(args.iter().map(|a| (*a).to_owned()));
        ("systemctl", all)
    };
    vec![
        systemctl(&["daemon-reload"]),
        systemctl(&["restart", TARGET]),
        systemctl(&["try-restart", "waybar.service", "mako.service"]),
    ]
}

/// koje's plugin from the file as it is now (a package update replaced it):
/// out of Hyprland if it is `loaded`, then in. Hyprland reads a plugin's
/// file only when loading it.
pub fn plugin_reload_commands(loaded: bool) -> Vec<Cmd> {
    let hyprctl = |verb: &str| {
        (
            "hyprctl",
            vec!["plugin".to_owned(), verb.to_owned(), PLUGIN.to_owned()],
        )
    };
    let mut cmds = Vec::new();
    if loaded {
        cmds.push(hyprctl("unload"));
    }
    cmds.push(hyprctl("load"));
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugin_is_loaded_again_from_its_file() {
        let line = |c: &Cmd| format!("{} {}", c.0, c.1.join(" "));
        let again: Vec<String> = plugin_reload_commands(true).iter().map(line).collect();
        assert_eq!(
            again,
            [
                "hyprctl plugin unload /usr/lib/koje/koje-hyprland.so",
                "hyprctl plugin load /usr/lib/koje/koje-hyprland.so"
            ]
        );
        let first: Vec<String> = plugin_reload_commands(false).iter().map(line).collect();
        assert_eq!(
            first,
            ["hyprctl plugin load /usr/lib/koje/koje-hyprland.so"]
        );
    }

    #[test]
    fn xdph_gets_kojes_picker_then_the_users_settings() {
        let plain = xdph_conf(SHARE_PICKER, None);
        assert!(plain.contains(
            "screencopy {\n    custom_picker_binary = /usr/lib/koje/koje-share-picker\n}\n"
        ));
        assert!(!plain.contains("koje/xdph.conf\n\n"));
        let user = xdph_conf(SHARE_PICKER, Some("screencopy {\n    max_fps = 30\n}"));
        let picker = user.find("custom_picker_binary").unwrap();
        let fps = user.find("max_fps = 30").unwrap();
        assert!(picker < fps, "{user}");
        assert!(user.ends_with("}\n"));
    }

    #[test]
    fn xdph_env_keeps_the_users_config_home() {
        let dir = Path::new("/run/user/1000/koje/xdph");
        assert_eq!(
            xdph_env(dir, None),
            "XDG_CONFIG_HOME=/run/user/1000/koje/xdph\nKOJE_USER_CONFIG_HOME=\n"
        );
        assert!(
            xdph_env(dir, Some("/home/u/cfg")).ends_with("KOJE_USER_CONFIG_HOME=/home/u/cfg\n")
        );
    }

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
            Some(Path::new(PLUGIN)),
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
        assert!(full.contains("\nplugin = /usr/lib/koje/koje-hyprland.so\n"));
        assert!(full.contains("\nexec-once = koje session ready\n"));
        assert!(full.contains("\nmisc:disable_xdg_env_checks = true\n"));
        let bare = session_conf(None, Path::new(SNIPPET), None, None);
        assert_eq!(bare.lines().filter(|l| l.starts_with("source")).count(), 1);
        assert!(!bare.contains("plugin ="));
    }

    #[test]
    fn hyprland_gets_kojes_config() {
        let conf = Path::new("/run/user/1000/koje/session.conf");
        assert_eq!(
            hyprland_args(conf, false),
            ["--config", "/run/user/1000/koje/session.conf"]
        );
        assert_eq!(
            hyprland_args(conf, true),
            ["--", "--config", "/run/user/1000/koje/session.conf"]
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
        let restart: Vec<String> = restart_commands().iter().map(|c| c.1.join(" ")).collect();
        assert_eq!(
            restart,
            [
                "--user daemon-reload",
                "--user restart koje-session.target",
                "--user try-restart waybar.service mako.service"
            ]
        );
    }
}
