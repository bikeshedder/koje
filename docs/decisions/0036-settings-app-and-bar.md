# 0036. A settings app built with Kiesel; the bar stays on Waybar

- Status: Accepted; bar and toolkit superseded by ADR 0042 (own bar, GTK4)
- Date: 2026-10-22
- Source: plan task M8.3, discussion (the concept's panel overlay revisited)

## Context

The concept planned three UI stages: a launcher menu, a Waybar module and an own
panel as a layer-shell overlay, plus a layout overview (Super+L), overlays on
sleeping and hanging windows (ADR 0024) and a beacon window per running context
(ADR 0028). The toolkit for the panel was open (iced, egui or Slint).

Looking at what each part is for, koje's UI splits into two kinds:

- **Viewing and changing things** – contexts, states, layouts, windows, colors,
  limits. This is occasional work and does not need to float over the desktop.
- **Glancing and switching** – the active context, its layouts, stashed windows,
  memory, other running contexts. This must always be visible.

Kiesel is the user's own UI toolkit (a component library with a desktop port in
progress). Using it gives koje components that are designed together, and koje
is the first application of the desktop port.

## Decision

- **Settings app** (`koje-gui`): a normal application window (xdg toplevel),
  built with **Kiesel Desktop**. It holds everything that is viewed and
  changed: contexts (create, edit, start, stop, pause, recreate), states,
  layouts (the windows × layouts matrix, thumbnails, dragging windows between
  layouts), windows, memory, later global settings and integrations. It is a
  host program and a client of kojed (ADR 0027), like the CLI.
- The **layout overview** (Super+L) as an overlay over the desktop is dropped;
  its content moves into the settings app's layouts page.
- The **panel overlay** is dropped; the settings app replaces it.
- **Bar:** stays on **Waybar** with koje's custom modules (`koje bar` and
  successors) and the fuzzel pickers. Anything a custom module cannot do (rich
  menus, dragging) opens the settings app or a picker. An own bar is only built
  when Waybar's limits get in the way; it would then also use Kiesel Desktop.
- **Window overlays** on sleeping and hanging windows (ADR 0024) stay planned,
  as layer-shell surfaces built with Kiesel Desktop.
- The **beacon window** (ADR 0028) stays planned for now, built with Kiesel
  Desktop, but may be dropped: `koje bar` already keeps running contexts
  visible without it.

## Consequences

+ The first GUI is an ordinary window: no layer-shell, no placement over other
  windows, no keyboard grabbing. Kiesel Desktop can start with what every
  application needs.
+ One place for all settings; the CLI and the settings app are equal clients of
  kojed.
+ The bar keeps working on any setup with Waybar; no own bar to maintain.
- Layout editing is no longer "over the desktop"; the user opens the settings
  app for it.
- Window overlays still need layer-shell support in Kiesel Desktop, later.
- koje depends on Kiesel Desktop's progress for its GUI.
- Follow-ups: plan tasks for the settings app (M8.9–M8.15); the component
  priority list for Kiesel Desktop (`docs/upstream/kiesel-desktop-components.md`).
