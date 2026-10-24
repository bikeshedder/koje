# 0043. GNOME's look and feel, with libadwaita

- Status: Accepted
- Date: 2026-10-25
- Source: discussion (the look of koje's GTK4 UI, ADR 0042); answers the open
  question "Look of koje's GTK4 UI"

## Context

The user likes GNOME and its Adwaita style. koje runs on Hyprland only because
Mutter cannot be customized enough for contexts (border colors per window,
special workspaces, placing windows). GNOME's top bar cannot be used: it is
part of GNOME Shell, which is a Mutter plugin, and its extensions have no stable
API. koje's own shell (ADR 0042) has to draw the bar, the OSD and the
notifications anyway, so it can draw them the way GNOME does.

ADR 0042 left open whether koje's GTK4 UI uses libadwaita or its own CSS.

## Decision

- **koje looks and feels like GNOME.** Where koje has no reason to differ, it
  follows GNOME's design and behavior. The deliberate differences are koje's
  features: the context color (window borders, accents in the bar and the
  settings app, title bars), context workspaces, layouts, the stash and the
  overlays.
- **libadwaita** for all of koje's UI. The settings app is a GNOME app
  (`AdwApplicationWindow`, split views, preferences pages). The shell's surfaces
  use libadwaita's widgets and styles. The context color is the accent of a
  subtree through libadwaita's accent CSS variables (libadwaita 1.6 or later).
- **Icons:** the icon theme's symbolic icons, as GNOME apps use them. The
  Lucide font (ADR 0020) stays only for Waybar and the fuzzel menus until the
  shell replaces them.
- **The bar is laid out like GNOME's top bar:** on the left contexts and
  workspaces in place of "Activities"; the clock in the middle, its popover with
  the calendar and later the notification list; on the right the tray and one
  Quick Settings button whose popover holds volume, network, battery,
  brightness, keyboard layout and the dark style. koje's own parts (task list,
  active context, memory, overlays, layouts) fit into this layout; their places
  are settled in the bar tasks.
- **OSD and notifications** look and sit like GNOME's.
- **Session look:** the Adwaita icon and cursor themes and GNOME's fonts; light
  and dark through `org.gnome.desktop.interface color-scheme`, which GTK apps
  and the settings portal read – so apps in contexts follow it too (ADR 0019).
- **Windows:** no gaps, rounding and shadows close to GNOME's windows; maximize
  fills the area below the bar. The border in the context color (ADR 0018)
  stays as it is.
- **Title bars** from hyprbars (ADR 0039) look like an Adwaita header bar:
  height, colors, centered title, GNOME's buttons.
- **Later, the launcher, the window picker and the keyboard shortcuts list**
  move from fuzzel into the shell, in GNOME's style.
- **GNOME's key bindings:**
  - **Super alone** opens the launcher – today koje's application menu
    (Super+D), later the shell's – and closes it again. It fires when Super is
    released without another key or a mouse button in between, so Super+key and
    Super+drag do not open it.
  - **Super+Up** maximizes, **Super+Down** restores the window's previous size
    and place (from maximized or a half), **Super+Left** and **Super+Right** put
    it on the left or right half of the monitor, below the bar.
  - koje's other bindings stay as they are.

## Consequences

+ koje's UI and the GNOME and GTK apps people run in contexts look like one
  desktop.
+ libadwaita provides the widgets, light and dark, and the accent colors; little
  of koje's own CSS.
- GNOME Shell does not use GTK (it uses St), so its bar, Quick Settings, OSD and
  notifications are rebuilt by hand, not reused.
- libadwaita is opinionated: styling beyond its variables works against it.
- koje follows GNOME's design changes by hand, release by release.
- Building needs libadwaita's development package too.
- Super alone depends on Hyprland not firing the release binding after a
  combination; to be checked on the host.
- Restoring the previous size after a half or maximize means koje remembers
  the window's geometry.
- Further GNOME bindings and touchpad gestures are still an open question.
- Supersedes the look values of ADR 0037 (gaps, rounding, shadows) and, for
  koje's own UI, the Lucide font of ADR 0020. Follow-ups: plan tasks
  M8.38–M8.41.
