# 0001. Hyprland is the only compositor

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

koje needs to list windows with PID and geometry, place them pixel-exact, create
workspaces and color borders per window. Under Wayland only the compositor can do
this. GNOME would need a self-maintained Shell extension, KDE Plasma cannot color
borders per window, niri's column model does not fit a free-floating workflow on a
5K monitor, X11 desktops (XFCE) break isolation between contexts.

## Decision

Support Hyprland only, configured fully floating (`windowrule = float, class:.*`),
Super+LMB move, Super+MMB resize. Talk to it via its command and event sockets.
No compositor abstraction beyond the `Compositor` port needed for testing.

## Consequences

+ Open IPC, per-window border colors, special workspaces for stash and overlays.
- Hyprland's config syntax and IPC change between versions → pin tested versions,
  keep the adapter small (open question).
- Users of other desktops cannot use koje.
