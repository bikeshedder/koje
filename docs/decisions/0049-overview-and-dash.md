# 0049. An overview and a dash like GNOME's

- Status: Accepted (the windows: ADR 0051 – a koje Hyprland plugin; the dash
  per context and the workspace shortcuts: ADR 0053)
- Date: 2026-10-26
- Source: discussion after the task list was dropped (ADR 0047); amends the
  Super key of ADR 0043

## Context

With the task list gone (ADR 0047), the bar shows no running programs. Window
switching depends on Alt+Tab and the fuzzel window picker (Super+Tab). GNOME
answers this with the Activities overview: Super shows the workspace's windows
side by side, the workspaces above them, the dash (favorites and running apps)
below, and search; the application grid is one step further. koje follows GNOME
where it has no reason to differ (ADR 0043), but GNOME has no contexts, layouts
or stash.

ADR 0043 bound Super alone to the launcher.

## Decision

- **The overview** is a full-screen surface of the shell, as GNOME's:
  - the **current workspace's windows** as thumbnails – in a context that is
    its active layout; click focuses a window, closing works as in GNOME;
  - a **workspace strip** on top with all workspaces: contexts in their color,
    host workspaces; click switches, dropping a window there moves it;
  - the context's **stashed windows** in a small row of their own; click brings
    one back into the active layout;
  - **search**: typing searches apps (later windows and GNOME's search
    providers, M8.50).
- **The dash is GNOME's dash**: favorites and running apps at the bottom of the
  overview only. There is no permanent dock.
- **Super alone opens and closes the overview**, as in GNOME (amends ADR 0043,
  which bound it to the launcher). The grid button in the dash and Super+A show
  all apps.
- **An app in several contexts is one entry per (app, context)** in the dash
  and in Alt+Tab, marked with the context color. Favorites launch in the active
  context.

## Consequences

+ Running programs are visible again, the GNOME way, without taking screen
  space.
+ One surface replaces the fuzzel window picker and, with the grid, the fuzzel
  application menu (M8.18, M8.40).
- Window thumbnails need a capture protocol (Hyprland's toplevel export) inside
  the GTK shell, including windows on hidden workspaces and of paused contexts:
  a host spike first; until then the overview shows icons and titles.
- Super alone still depends on Hyprland's release binding not firing after a
  combination (ADR 0043).
- The same app twice in the dash costs room when many contexts run it.
- Follow-ups: M8.67–M8.71; M8.18, M8.40, M8.41 and M8.49 adjusted.
