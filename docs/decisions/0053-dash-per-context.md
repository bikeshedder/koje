# 0053. The dash and the workspace shortcuts follow the context

- Status: Superseded by [0061](0061-remove-the-hyprland-path.md)
- Date: 2026-10-26
- Source: host test of the overview (M8.69); M8.75, M8.76

## Context

ADR 0049 made the dash GNOME's: the favorites (`org.gnome.shell
favorite-apps`) launch in the active context, and every running app of the
desktop follows, one entry per (app, context). On the host this was odd:

- A context is like a machine of its own. Only the bar and the desktop's
  behaviour are shared between contexts; the apps of the others in the dash
  of one are noise.
- GNOME's favorites are the host's desktop file ids. A context has other
  programs, or the same ones under other ids (Fedora's
  `org.mozilla.firefox.desktop` for the host's `firefox.desktop`), so a
  click on a favorite in a context did nothing.

Workspace shortcuts had the same blind spot: Super+1…9 reached only the
numbered host workspaces, never a context's named one.

## Decision

- **The dash shows the active context only** – its favorites and its
  running apps (the stash included); on a host workspace the host's. Apps of
  other contexts are reached through their workspace, the search or the
  app grid.
- **Favorites per context**: `favorites` in `context.toml` – desktop file
  ids of the context's programs, in order, pinned and unpinned from the
  dash's menu (`koje favorites [-c <context>] [--set <id>… | --reset]`).
  Unset, they are GNOME's favorites as far as the context has them: the
  same id, the same last part of the id, the same name or a program of
  that name; the others are left out. The first pin in a context writes
  its list, from then on it is the context's own. On the host, pinning
  stays GNOME's `favorite-apps`.
- **Running apps are the context's programs**: a window belongs to the
  program whose `StartupWMClass`, id (or its last part) or program name is
  its app id (`koje apps --json` now gives `wm_class` and `binary`); the
  dash shows that program's name and icon and starts it again by its id.
- **Workspace shortcuts as GNOME's, through the bar's order** (numbered host
  workspaces, then the contexts, ADR 0028): Super+1…9 the n-th, Super+Page
  Up/Down and Ctrl+Alt+Left/Right the one before or after, no wrapping; a
  running context without a workspace opens. With Shift the focused window
  goes along – only where it belongs: a host window to host workspaces, a
  context's window stays in its context (as M8.71's dragging). It stays on
  screen and focused while the workspace changes behind it, as in GNOME
  (added 2026-10-27; Hyprland pins it for the switch – a maximized or
  fullscreen window, which cannot be pinned, moves and is focused again).

## Consequences

+ A click in a context's dash starts what is installed there.
+ Each context gets a dash of its own without setting anything up.
- Matching GNOME's favorites and windows to a context's programs is a
  heuristic: an app with an unusual id, name and program is left out of the
  favorites until pinned, and its window gets the generic icon.
- Shift with a workspace shortcut does nothing for a context's window
  unless the target is its own workspace – unlike GNOME.
- Follow-ups: M8.75 (done), M8.76.
