# 0056. Everything in koje.conf is a setting: the look in koje Settings, shortcuts in GNOME Settings

- Status: Superseded by [0061](0061-remove-the-hyprland-path.md)
- Date: 2026-10-27
- Source: discussion after M8.91 ("make the overlay configurable in the koje
  settings – everything in koje.conf should be editable with the koje
  settings app"); builds on ADR 0044 (GNOME Settings for the system) and
  ADR 0055 (`koje config --stdin`)

## Context

koje's Hyprland snippet (`/usr/share/koje/hyprland/koje.conf`, ADR 0037)
sets the windows' look, overlays' look, the behaviour koje copies from
GNOME, and about 70 key bindings. Changing any of it meant writing Hyprland
syntax into `~/.config/koje/hyprland.conf`. The user wants all of it
editable from the settings apps.

The snippet holds four kinds of lines:

1. **Look and animations** – borders, gaps, corners, shadows, snapping, how
   an open overlay darkens, blurs and zooms (M8.86, M8.91), the overview's
   animation (M8.73). koje's own taste; nothing in GNOME covers them.
2. **Key bindings** – GNOME's own shortcuts (Alt+Tab, Super+arrows,
   screenshots, workspaces, media keys) and koje's (stash, layouts, the
   terminal in the context).
3. **Input and focus** – natural scrolling, focus on click. GNOME Settings
   owns input (ADR 0044, the gsettings bridge).
4. **Fixed behaviour** – everything floats, resize from borders, focus on
   activate, no ANR dialog, no Hyprland logo, the cursor workaround (M8.62).
   They are what makes koje behave as GNOME or work at all.

## Decision

- **Look and animations are `[desktop]` in `config.toml`**, edited on koje
  Settings' **Desktop** page (`koje-shell settings`): `border_width`,
  `gaps_inside`, `gaps_outside`, `corner_radius`, `shadows`, `snap`,
  `foreign_glow`, `foreign_badge` (ADR 0057),
  `overlay_dim` (percent), `overlay_blur` (0: none), `overlay_zoom`,
  `overview_duration` (ms). Ranges are checked as for the rest of the file.
  **kojed applies them with `keyword`** at start, when the file changes and
  after Hyprland reloads – as the overlays' keys (M7.1). `koje.conf` keeps
  the same values as defaults. Only keys that are set are applied, so the
  user's `hyprland.conf` still decides the others; the settings app writes
  only values that differ from the default. A key removed goes back to the
  default. The logic is `koje_core::desktop`.
- **Key bindings are GNOME Settings' keyboard shortcuts.** kojed reads
  GNOME's keybinding gsettings (`org.gnome.desktop.wm.keybindings`,
  `org.gnome.shell.keybindings`, `org.gnome.mutter.keybindings`,
  `org.gnome.settings-daemon.plugins.media-keys`) and binds koje's action
  for each – the same bridge as for input (ADR 0044). koje's own actions
  (stash, layouts, terminal in the context, …) become GNOME custom shortcuts
  (M8.63). koje Settings has no shortcuts page. Plan task M8.93.
- **Input and focus follow GNOME Settings 100 %**, as ADR 0044 says; there
  is no focus-follows-pointer option in koje.
- **Fixed behaviour stays fixed** in `koje.conf`; `hyprland.conf` remains
  the escape hatch for anything Hyprland can do.

## Consequences

+ The overlay's tint, blur and zoom, and the windows' look, change from a
  page with live effect; no Hyprland syntax needed.
+ Shortcuts are edited where a GNOME user looks for them.
- Two writers for the same Hyprland settings: a value in `hyprland.conf`
  wins until the key is set in `[desktop]`; the Desktop page shows koje's
  default then, not the `hyprland.conf` value.
- `[desktop]` keys set while koje's plugin is not loaded are skipped for the
  plugin's values (`overlay_zoom`, `overview_duration`) and logged.
- Follow-up: M8.93 (shortcuts from GNOME Settings, which takes in M8.63).
