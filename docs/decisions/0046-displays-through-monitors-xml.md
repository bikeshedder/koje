# 0046. Displays: GNOME Settings through Mutter's API, `monitors.xml` as the source

- Status: Accepted
- Date: 2026-10-26
- Source: discussion (a lost `hyprland.conf` left the screen at a wrong mode;
  configuring screens in GNOME Settings); answers the open question
  "Displays" of ADR 0044

## Context

GNOME Settings is koje's system settings (ADR 0044), but its Displays panel
does not write gsettings: it talks to Mutter over D-Bus
(`org.gnome.Mutter.DisplayConfig`) and Mutter keeps the result in
`~/.config/monitors.xml`. On Hyprland nothing answers, so the panel is empty and
screens are configured by hand with `monitor = …` lines in `hyprland.conf`.
When that file is lost, Hyprland falls back to the screen's "preferred" mode –
for a 5120×2160 screen that is 30 Hz instead of 120 Hz.

`monitors.xml` alone is read too late: kojed starts after Hyprland, so every
login would first show Hyprland's default mode. A Hyprland config alone cannot be
shared with GNOME and has no notion of monitor sets.

## Decision

- **kojed implements `org.gnome.Mutter.DisplayConfig`** on the session bus, from
  Hyprland's monitors (`hyprctl monitors all`): screens, modes, scale, position,
  rotation, primary. `ApplyMonitorsConfig` from GNOME Settings is applied to
  Hyprland and, when persistent, written to `monitors.xml`.
- **`~/.config/monitors.xml` is the single source of truth**, in Mutter's format
  and with its rules: one configuration per set of connected monitors, monitors
  identified by connector, vendor, product and serial. A real GNOME session on
  the same account reads and writes the same file.
- **kojed generates Hyprland's monitor config from it**:
  `$XDG_CONFIG_HOME/koje/hyprland/monitors.conf`, sourced by koje's snippet, for
  the configuration that matches the connected monitors. Hyprland reads it at
  login before kojed runs, so the screen starts in the right mode. kojed writes
  it again when `monitors.xml` changes (also when edited by hand or by GNOME) and
  on hotplug, and applies it. The file is generated: hand edits are lost.
- **The "Keep these display settings?" dialog** – in GNOME it is GNOME Shell's,
  not GNOME Settings' – is koje-shell's: after a change it counts down and
  reverts unless confirmed.
- `koje setup` stops writing a `monitor =` line into a new `hyprland.conf`; an
  own `monitor` line there still wins over the generated file (sourced before).
- **When:** after the bar (M8.27–M8.35) and after the host spike on GNOME
  Settings under Hyprland (M8.42).

## Consequences

+ Screens are configured in GNOME Settings like on GNOME; settings survive a
  lost `hyprland.conf` and are shared with a GNOME session.
+ The right mode from the first frame of a login.
- kojed implements a large, unstable Mutter API (versioned by GNOME release,
  not documented as public); koje follows GNOME's changes by hand.
- Two files describe the screens; only one (`monitors.xml`) may be edited, the
  other is generated – an own `monitor` line in `hyprland.conf` overrides both.
- Until this is built, a new `hyprland.conf` from `koje setup` still has the
  generic `monitor = , preferred, auto, 1`.
- Follow-ups: plan tasks M8.51–M8.54.
