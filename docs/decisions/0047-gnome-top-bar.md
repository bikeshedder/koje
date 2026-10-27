# 0047. koje's bar is a clone of GNOME's top bar

- Status: Accepted
- Date: 2026-10-26
- Source: discussion after M8.30 ("clone the GNOME bar, not the Waybar
  design"); revises the bar of ADR 0042 and ADR 0043

## Context

ADR 0043 lays the bar out "like GNOME's top bar" but kept what koje's Waybar
config had: a task list and workspace buttons with names, in libadwaita's
window colors. The result looked like a restyled Waybar. koje follows GNOME's
guidelines as closely as possible, where it makes sense; GNOME's top bar has
no task list, shows workspaces as dots, is black in light and dark style and
has no tray (most users add the AppIndicator extension).

## Decision

- **The bar clones GNOME's top bar**: black in both styles, white bold text,
  panel buttons that are transparent pills and light up on hover and while
  their menu is open, the clock in the middle. Dropdowns follow light and
  dark like GNOME's menus.
- **No task list.** Windows are switched as in GNOME: Alt+Tab (M8.49), the
  launcher and window picker (M8.40); until those exist the fuzzel window
  picker (Super+Tab) stays. Dragging windows onto workspaces in the bar
  (ADR 0042) goes with it.
- **Workspaces on the left, in a configurable style** – `[bar] workspaces` in
  `config.toml`, later a page of the settings app:
  - `colored-dots` (default): GNOME's workspace indicator, one dot per
    workspace and the active one stretched into a pill; a context's dot in
    its color, host workspaces white.
  - `dots`: GNOME's indicator unchanged, white.
  - `names`: GNOME-styled panel buttons with the workspace names.
  Each dot is a button (click switches, the name on hover). Running contexts
  without a workspace (ADR 0028) show as dots too.
  Update (ADR 0055): the default is now `labels` – each workspace's icon and
  label; the dot styles stay selectable.
- **Right side, as GNOME's**: tray icons as panel buttons (as the
  AppIndicator extension, M8.31), the koje menu (M8.55), and one Quick
  Settings pill with the system icons (M8.30).
- The koje menu stays: it is koje's (contexts, memory, limits); GNOME has
  nothing in its place.

## Consequences

+ The bar looks and behaves like the GNOME users know; less of koje's own
  design to maintain.
+ More room: no task list competes with the centred clock.
- Window switching depends on Alt+Tab and the launcher, which are not built
  yet (M8.40, M8.49); the fuzzel picker bridges the gap.
- Workspace names are visible only on hover with the dot styles.
- Dragging windows between workspaces in the bar (one of the three wishes of
  ADR 0042) is dropped; the overview or launcher may bring it back.
- Plan: M8.28's task list and M8.33 dropped; a task for the workspace styles
  and the GNOME look (M8.57).
