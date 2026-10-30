# 0061. koje runs on GNOME only; the Hyprland path is removed

- Status: Accepted
- Date: 2026-10-30
- Source: decision of the user after daily use of the GNOME prototype
  (plan [G](../plan/G-gnome.md), G.28); ADR 0059 asks for this ADR

## Context

ADR 0059 tried koje inside a stock GNOME session, with a GNOME Shell extension
for what is koje's own. The prototype is in daily use: contexts, states,
memory limits, sleep with its overlay, the glow and badge, context apps in
GNOME's grid and the top-bar indicator all work on GNOME. The user uses GNOME
from now on.

Keeping the Hyprland path means keeping koje-shell (a copy of GNOME's bar,
overview, OSD, notifications and more), the Hyprland plugin, the session, the
bridges from GNOME's settings to Hyprland, and two adapters behind every
compositor call. Nobody uses it any more.

## Decision

- **koje supports GNOME Shell only**, through koje's extension (ADR 0059).
  kojed has one compositor adapter, `koje-gnome`.
- **Removed:** `koje-shell`, `koje-hyprland`, the Hyprland plugin, the koje
  session and its units, the Hyprland snippet, Waybar, hyprbars, hypridle and
  hyprlock files, the pickers in `assets/bin`, kojed's gsettings bridge,
  DisplayConfig service, `monitors.xml` writer and media players, and the CLI
  commands that only work on Hyprland (`layout`, `stash`, `unstash`, `snap`,
  `overlay`, `pin`, `unpin`, `session`, `media`, `screenshot`, `bar`).
- **Kept as data:** layouts stay part of a state's file format (ADR 0004).
  On GNOME a state restores one geometry per window (ADR 0059). Layouts, the
  stash and overlays may come back as GNOME features later, with new ADRs.
- **The settings app** is rebuilt in TypeScript (ADR 0060). Until then,
  `koje settings` has no app to open.
- **Superseded:** 0001, 0018, 0022 (Mutter has no `security-context-v1`, 0059
  L1), 0036, 0037, 0039, 0041–0044, 0046–0054, 0056, 0058, and the plugin part of
  0057. 0059 is accepted: it describes the path koje takes.

## Consequences

+ About half of the code goes. Every compositor call has one implementation.
+ GNOME provides the bar, overview, settings, displays, input, OSD,
  notifications, lock screen and session.
- The limitations in ADR 0059 are now koje's (L1–L14), including weaker
  Wayland isolation and one app per program across contexts.
- Layouts, stash, overlays, pin and snap are gone until they are designed for
  GNOME.
- No settings app until G.27.
