# 0057. Windows look as in GNOME; windows outside their context glow and carry a badge

- Status: Accepted (the plugin part: superseded by [0061](0061-remove-the-hyprland-path.md))
- Date: 2026-10-27
- Source: discussion ("the window borders should look exactly like GNOME –
  the current outlines just don't look right"); revises ADR 0018's visible
  part, takes part of M8.38

## Context

koje drew a 3 px border in the context's color around every context window
(ADR 0018), with 6 px corners and gaps. GNOME draws no frame: libadwaita
windows have 15 px corners, a soft layered shadow and a faint inner outline
(libadwaita 1.7 `window.csd`). The colored frame clashed with that and did
not even match the apps' corners.

The color still has a job: telling which context a window belongs to. That
matters mostly when a window is *not* where it belongs – on another
context's workspace or on a host workspace. On its own context's workspace
the workspace itself (and the bar) already says it. Color alone is also not
enough to tell contexts apart.

## Decision

- **Windows look as in GNOME:** no border, no gaps, 15 px corners, a soft
  shadow close to libadwaita's (`koje.conf`; `[desktop]` defaults, ADR 0056).
- **A window outside its context** – a context's window on another context's
  workspace or a host workspace; never on overlays or the stash, never a host
  window – is marked by koje's plugin (ADR 0051):
  - a **glow**: its shadow in the context's color, stronger when focused;
  - a **badge**: a pill in the context's color with the context's name on
    the window's top edge (inside the window where the edge is under the
    bar).
- The plugin decides by itself: a window's context is kojed's `koje-<context>`
  tag, a context's workspace is named like the context (ADR 0028), the color
  is the first color of kojed's border rule. kojed keeps setting tags and
  rules as before (ADR 0018); nothing new to send.
- `[desktop]` `foreign_glow` and `foreign_badge` switch them off;
  `border_width = 3` with `foreign_glow = false` is the old look with the
  badge on top. The Desktop page of koje Settings has all three.

## Consequences

+ Windows look as GNOME's; the context shows where it tells something.
+ The badge names the context – no need to remember colors.
- Plugin code (hooks of `CWindow::updateDecorationValues` and
  `CCompositor::moveWindowToWorkspaceSafe`, a window decoration) – checked
  against Ubuntu's Hyprland 0.53.3 headers and symbols, not yet run.
- Assumes Hyprland crops windows to their geometry, so GTK's own shadow does
  not show besides Hyprland's; to check on the host.
- Not yet GNOME's: maximized windows keep their corners (Mutter squares
  them), apps without own title bars get round corners at the bottom too.
- A window's focus blip (kojed sets its border white for a moment, M8.19)
  shows as a white glow on foreign windows.
