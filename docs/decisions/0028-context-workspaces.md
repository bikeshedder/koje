# 0028. One named workspace per context

- Status: Accepted
- Date: 2026-10-19
- Source: plan task M4.6, concept "Workspaces"

## Context

The concept gives each context one Hyprland workspace ("Ein Kontext belegt genau
einen Hyprland-Workspace pro Monitor"); `koje focus` brings it to the front.
Programs in a context open their windows wherever Hyprland puts them, usually
the current workspace, and a link clicked in context A may open a window while
the user looks at context B. How several monitors share a context is still open
(`docs/open-questions.md`).

## Decision

- A context's workspace is the **named workspace `<context>`**. Context names
  start with a letter, so they never clash with numbered or `special:`
  workspaces.
- kojed **moves every context window** there when it maps it (on `openwindow`
  and when it syncs at start or after a reconnect), silently (the user's view
  stays where it is). Windows on special workspaces (overlays, stash) are left
  alone. Host windows are never moved.
- `koje focus <context>` asks kojed (`Focus`) to switch to that workspace.
  `koje up` does the same after starting a context, so its new windows are in
  view. Without kojed, `focus` fails with the start hint, `up` skips it.
- Single monitor for now: Hyprland places the workspace on the focused monitor.

## Consequences

+ A context's windows stay together without the user sorting them.
+ Waybar's workspace module shows contexts by name.
- A window opened from another context appears on its own workspace, not in
  front of the user; a notification or bar hint may be needed (M8).
- Multi-monitor (one workspace per context per monitor) still needs a decision;
  the workspace name will then need a monitor part.
- The `global` context's windows (M7) would be moved to a workspace `global`;
  M7 decides whether that context is exempt.
- Update (M5.15): an empty context workspace disappears from Hyprland (and
  its workspace list in Waybar). Keeping it persistent was tried and dropped:
  Hyprland clears a workspace's persistent flag only on a config reload, so
  every `koje down` would have reloaded the config. Instead the bar shows
  *contexts*: `koje bar`, a Waybar module fed by kojed (`RunningContexts`,
  signal `ContextsChanged`), lists the running contexts in their colors with
  the active one marked – the context of the focused workspace, also when it
  has no windows. A click opens a picker that runs `koje focus`.
- Planned (M8.8): a **beacon window** per running context on its workspace,
  koje's own small window with the context's controls, keeps the workspace
  alive. (Renaming a workspace also removes it, which would make persistent
  rules workable without reloads; not pursued in favor of the beacon.)
