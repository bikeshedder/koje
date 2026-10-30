# 0048. koje is its own session; Hyprland is the compositor it starts

- Status: Superseded by [0061](0061-remove-the-hyprland-path.md)
- Date: 2026-10-26
- Source: discussion (GNOME Settings refuses to start under
  `XDG_CURRENT_DESKTOP=Hyprland`, M8.30; a lost `hyprland.conf` took the
  monitor settings with it); revises ADR 0037 and ADR 0021

## Context

Until now the login screen starts the *Hyprland* session; koje lives inside
the user's `~/.config/hypr/hyprland.conf`, which sources koje's snippet
(ADR 0037), and starts its services with `exec-once`. That has three costs:

- **Desktop identity.** Every program sees `XDG_CURRENT_DESKTOP=Hyprland`.
  GNOME Settings exits at once, `OnlyShowIn=GNOME` entries are hidden, Qt
  programs do not use the GNOME platform theme, and xdg-desktop-portal picks
  its backends from Hyprland's `hyprland-portals.conf`. koje means to look and
  work like GNOME (ADR 0043, 0044, 0047).
- **koje edits the user's file.** `koje setup` appends a `source` line to
  `hyprland.conf` or writes the whole file (ADR 0021, 0037); machine settings
  (monitor, keyboard) live in the same file and get lost with it.
- **Services without supervision.** kojed, the shell, mako, hypridle and the
  polkit agent start unordered from `exec-once`; nothing restarts them and
  nothing stops them at logout.

GNOME itself is a session (gnome-session) that starts its compositor (GNOME
Shell) and services as systemd user units. uwsm does the same generically for
any Wayland compositor but is a Python dependency that Debian trixie does not
ship.

## Decision

- **koje is a session of its own.** The package installs
  `/usr/share/wayland-sessions/koje.desktop` with `Exec=koje session start` and
  `DesktopNames=koje;GNOME`: the display manager sets
  `XDG_CURRENT_DESKTOP=koje:GNOME` for everything in it. "koje" is what the
  user picks on the login screen.
- **`koje session start` (called `koje-session` below) starts Hyprland with
  a config it generates at login**, `$XDG_RUNTIME_DIR/koje/session.conf`,
  which sources in this order: koje's snippet (the session as of ADR 0037),
  the monitor config generated from `monitors.xml` (ADR 0046), and – only if
  it exists – the user's `~/.config/koje/hyprland.conf`. Later settings win,
  `unbind` removes a koje binding. koje creates no file in the home for
  Hyprland: GNOME's settings are the source of truth (ADR 0044, 0046) and
  reach Hyprland through kojed; the user's file is an escape hatch for
  Hyprland-specific tweaks GNOME has no setting for. The session's environment
  comes from systemd's `environment.d` (e.g. GPU variables), not from
  Hyprland's `env =`. koje never reads or writes `~/.config/hypr/`; a plain
  Hyprland session stays the user's own.
- **koje's own systemd user units, not uwsm** – as gnome-session does it:
  `koje-session` passes the session's environment to systemd once Hyprland is
  up and starts `koje-session.target` (bound to `graphical-session.target`),
  which pulls in kojed, koje-shell, the notification daemon, hypridle and the
  polkit agent, each restarted on failure; XDG autostart entries run through
  systemd's `xdg-desktop-autostart.target`. When Hyprland exits, `koje-session`
  stops the target and clears the environment it set. No more `exec-once` for
  services.
- **Portals:** the package ships `koje-portals.conf`: Hyprland's backend for
  screen sharing, screenshots and global shortcuts, GNOME's and GTK's for the
  rest (file chooser, settings, …).
- **One way to run koje:** sourcing koje's snippet from a plain Hyprland
  session is no longer supported. `koje setup` stops editing
  `~/.config/hypr/hyprland.conf`; for an existing install it offers to move the
  user's machine settings into `~/.config/koje/hyprland.conf` and to remove its
  own `source` line (shown in the plan first, ADR 0021).
- `koje-session` is part of the CLI (`koje session start`, and `koje session
  ready` from Hyprland's `exec-once` for the systemd part); how exactly it starts
  Hyprland (directly or through Hyprland's `start-hyprland` watchdog) is
  settled by the host spike.

## Consequences

+ Programs see a GNOME desktop: GNOME Settings starts, `OnlyShowIn=GNOME`
  entries appear, Qt follows GNOME's theme, portals are koje's choice.
+ koje's files and the user's are apart: no edits to `hyprland.conf`, machine
  settings survive koje updates, a plain Hyprland session is untouched.
+ Services are supervised: ordered start, restart on failure, clean stop at
  logout.
- Programs that see `GNOME` may expect GNOME Shell's own D-Bus services
  (`org.gnome.Shell.Screenshot`, `org.gnome.SessionManager` for logout and
  inhibit); most fail gracefully. Where it matters, the shell provides them –
  the end session dialog (M8.47) is the natural home for a minimal session
  manager.
- koje maintains the session plumbing (environment, failed starts) that uwsm
  would solve.
- Users switch once: pick "koje" on the login screen and move machine
  settings (setup offers it).
- Revises ADR 0037 (where the session's config lives, `exec-once` services,
  `setup` writing `hyprland.conf`) and ADR 0021 (the `source` line);
  `XDG_CURRENT_DESKTOP` of ADR 0044's spike is decided: `koje:GNOME`.
- Follow-ups: plan tasks M8.58–M8.64.
