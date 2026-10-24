# 0044. GNOME Settings for the system, a gsettings bridge in kojed

- Status: Accepted
- Date: 2026-10-25
- Source: discussion (what of GNOME has to be rebuilt for a seamless GNOME
  feel, what can be reused); follows ADR 0043

## Context

koje looks and feels like GNOME (ADR 0043) but runs on Hyprland. GNOME is more
than its look: GNOME Shell and Mutter draw the desktop and apply settings,
gnome-settings-daemon applies more of them, and GNOME Settings
(`gnome-control-center`) edits them. Almost all of these settings live in
gsettings (dconf); the rest goes to system services (NetworkManager, BlueZ,
UPower, PipeWire, CUPS, accountsservice, timedated, …).

On Hyprland, everything that talks to a system service or only writes a value
GTK reads works unchanged. What Mutter, GNOME Shell or the parts of
gnome-settings-daemon that need them would apply has no effect: touchpad,
keyboard layouts, idle and power, night light, background, displays. Some of
GNOME Shell's duties have no replacement at all yet – most notably the
NetworkManager secret agent: nothing asks for a new Wi-Fi password.

## Decision

- **GNOME Settings is koje's settings for the system.** koje does not build
  pages for network, sound, Bluetooth, power, users, appearance and the like.
  koje's settings app (ADR 0036) holds everything that is koje's own:
  contexts, states, layouts, windows, memory, and koje's global settings
  (`config.toml`).
- **kojed applies GNOME's gsettings to Hyprland** (the gsettings bridge). For
  the keys it covers, gsettings is the source and GNOME Settings the editor;
  kojed applies them at start, on every change and after a Hyprland config
  reload. First:
  - mouse and touchpad (`org.gnome.desktop.peripherals.*`) → Hyprland's input
    settings;
  - keyboard layouts and options (`org.gnome.desktop.input-sources`) →
    `kb_layout`, `kb_variant`, `kb_options`;
  - idle, screen blank and suspend (`org.gnome.desktop.session`,
    `org.gnome.settings-daemon.plugins.power`) → hypridle;
  - night light (`org.gnome.settings-daemon.plugins.color`) → hyprsunset;
  - background (`org.gnome.desktop.background`) → a wallpaper program.
  Which key maps to which Hyprland setting is a pure function in `koje-core`.
- **The shell reads GNOME Shell's data:** favorites
  (`org.gnome.shell favorite-apps`), app folders
  (`org.gnome.desktop.app-folders`), the notification settings per app and
  "Do Not Disturb" (`org.gnome.desktop.notifications`), and the apps' search
  providers (`org.gnome.Shell.SearchProvider2`) for the launcher's search.
- **Reused as they are:** GNOME's apps; GDM as the login screen (it starts the
  Hyprland session); the system services above; gnome-keyring and gcr;
  `xdg-desktop-portal-gnome` or `-gtk` for the file chooser and settings,
  `xdg-desktop-portal-hyprland` for screenshots and screen sharing.
- **koje-shell rebuilds what GNOME Shell does,** besides what ADR 0042/0043
  already list: the NetworkManager secret agent (Wi-Fi and VPN passwords), the
  polkit agent, the end session dialog (log out, restart, shut down), the
  screenshot UI, and Alt+Tab as in GNOME (grouped by app, Alt+` within one).
  The lock screen stays hyprlock (ADR 0042), styled like GNOME's.
- **Not used:** gnome-session (the session stays systemd user units) and the
  gnome-settings-daemon plugins that need Mutter or GNOME Shell.
- A host spike settles the details first: under which `XDG_CURRENT_DESKTOP`
  GNOME Settings starts outside GNOME (e.g. `Hyprland:GNOME`), what that value
  changes in apps, and which panels work.

## Consequences

+ A GNOME user finds the system settings where and how they know them; koje
  builds no pages for them.
+ koje's settings app stays small and about contexts.
+ Settings made in GNOME Settings also reach GTK apps in contexts (color scheme,
  fonts) through the settings portal (ADR 0019).
- The **Displays** panel does not work: it uses Mutter's D-Bus API
  (`org.gnome.Mutter.DisplayConfig`). Whether kojed implements that API on
  Hyprland's monitors or koje gets its own page is an open question; until then
  monitors stay in `hyprland.conf` (ADR 0037).
- Keyboard shortcuts, multitasking, search and parts of accessibility in GNOME
  Settings have no effect or clash with koje's bindings.
- Keyboard layouts move from `hyprland.conf` (ADR 0037 writes them from
  `/etc/default/keyboard`) to gsettings once the bridge covers them.
- A `XDG_CURRENT_DESKTOP` that contains `GNOME` changes how some apps behave
  (tray icons, Qt themes); the spike checks it.
- Without gnome-session, apps cannot block logging out
  (`org.gnome.SessionManager.Inhibit`); the inhibit portal still works.
- Until the secret agent exists, nothing may ask for the password of a new
  Wi-Fi network; the spike checks whether GNOME Settings does while it is
  open.
- Follow-ups: plan tasks M8.42–M8.50; the open question on displays.
