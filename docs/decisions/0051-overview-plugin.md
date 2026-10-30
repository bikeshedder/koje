# 0051. The overview's windows come from a koje Hyprland plugin

- Status: Superseded by [0061](0061-remove-the-hyprland-path.md)
- Date: 2026-10-26
- Source: host test of M8.67 ("the screenshots are pixelated and don't look
  right"); amends ADR 0049

## Context

The overview (ADR 0049, M8.68) showed windows as pictures captured through
`hyprland_toplevel_export_v1` (M8.67): copies taken when it opens, scaled down
on the CPU, scaled up again by GTK – blurry, and never live. GNOME's overview
shows the windows themselves: Mutter scales and moves their textures, the
apps notice nothing and keep drawing.

Hyprland 0.53 has no IPC to draw a window elsewhere or smaller without
resizing it; resizing through IPC sends the app a configure event. Only code
inside the compositor can draw a window's buffer somewhere else. Hyprland's
way for that is a plugin – which must match the running Hyprland exactly
(ADR 0039).

## Decision

- **A koje plugin for Hyprland draws the overview's windows**: while the
  overview is open, each window's own buffer at the place and size the shell
  asks for, on the GPU, animated from where the window is – live and sharp,
  without copies and without telling the app (no configure, no resize).
- **The shell keeps the overview**: it arranges the windows (the pure
  `overview::arrange`), tells the plugin the places through Hyprland's
  command socket (a dispatcher of the plugin), and draws everything else –
  strip, titles, close buttons, stash, dash, search – on its transparent
  layer surface above. Clicks still go to the shell.
- **Built with the package for the distribution's Hyprland:** `just deb`
  compiles the plugin in an Ubuntu 26.04 container (nested Podman, M0.8)
  against `hyprland-dev` (`scripts/build-deb.sh`), and the package depends
  on that upstream Hyprland version (`>= 0.53.3, << 0.53.4~`; the script
  stops when Ubuntu's differs). The koje session loads it (`plugin =` in the
  generated session config, ADR 0048). No hyprpm, no build on the host.
- **Its own animation, GNOME's timing:** the windows fly in 250 ms, eased
  out (Clutter's `EASE_OUT_QUAD`), not with Hyprland's window animation;
  `plugin:koje:overview_duration` (ms, 0: at once) in the user's Hyprland
  config changes it. The plugin posts `kojeoverview>>landed,<monitor>` when
  they arrived; the shell shows the cards' rings, icons and titles then.
  When the windows change while it is open (one closes), the cards glide
  into their new places as long, eased alike: the shell's cards and the
  plugin's windows each by themselves, the shell reading the duration from
  Hyprland (M8.98).
- **Without the plugin** (not loaded, a Hyprland version it was not built
  for) the overview shows icons on its cards. The overview no longer captures
  windows; the capture code of M8.67 stays for the screen sharing picker
  (ADR 0050).

## Consequences

+ The overview looks and behaves like GNOME's: real, live windows, zoomed
  out from their places.
+ Nothing reaches the apps; no screencast is reported.
- Plugin code in C++ against Hyprland's internal API, which changes between
  releases: each Hyprland update needs the plugin checked and koje rebuilt;
  the exact version dependency makes the package manager hold Hyprland back
  until then.
- koje keeps a plugin although title bars (ADR 0039) may lose theirs later.
- Hyprland keeps a loaded plugin: `koje session restart` (run by
  `koje setup` after a package update) loads it again from its file.
- Follow-ups: M8.72 (build in the Ubuntu container, packaging, loading),
  M8.73 (drawing and animating the windows), M8.67 dropped in favour of them.
