# 0052. New windows open as in GNOME

- Status: Superseded by [0061](0061-remove-the-hyprland-path.md)
- Date: 2026-10-26
- Source: host report "every Firefox and Thunderbird window opens way too
  small" (a `WAYLAND_DEBUG` trace); ADR 0043 (GNOME look and feel), ADR 0051
  (koje's Hyprland plugin)

## Context

All windows float in koje (concept "Desktop: Hyprland"), so a new window gets
the size its program asks for, centred. Firefox and Thunderbird remember being
maximized and call `xdg_toplevel.set_maximized` before their first frame.
Hyprland 0.53 acts on that request only for windows already mapped
(`CWindow::onUpdateState`) and drops it otherwise – unlike a fullscreen
request, which it keeps for the map. Firefox then gets a configure without a
size and stays at a placeholder (526×304 on a 5120×2160 screen):

```
-> xdg_toplevel.set_maximized()
<- xdg_toplevel.configure(0, 0, …)
-> xdg_surface.set_window_geometry(26, 23, 252, 304)
```

GNOME opens these windows maximized. Mutter also maximizes a window shown for
the first time whose area is more than 80 % of the monitor's work area
(`auto-maximize`, `place.c`); it does not shrink other windows, only moves
them onto the screen.

Hyprland has no IPC for a request it dropped, and kojed sees a window only
after it is mapped. Inside the compositor, Hyprland's `openWindow` hook runs
after the map has set up fullscreen states and before the first frame.

## Decision

- **A window that asks to be maximized before it is shown opens maximized**:
  koje's plugin remembers the request (hooking `CWindow::onUpdateState` while
  the window is unmapped) and, on `openWindow`, maximizes the window the way
  Hyprland does for a mapped window's request – the program is told it is
  maximized. `windowrule … suppress_event maximize` still stops it.
- **Auto-maximize as Mutter's**: a new floating window larger than 80 % of the
  work area (by area; the monitor below the bar) opens maximized, unless its
  size is fixed (minimum = maximum). `plugin:koje:auto_maximize = 0` in
  `~/.config/koje/hyprland.conf` turns it off.
- **Other new windows keep their own size**; Hyprland centres them as before.
- Maximized is Hyprland's maximized state (as Super+Shift+F and the title
  bar's button), filling the monitor below the bar.

## Consequences

+ Firefox, Thunderbird and other programs that remember being maximized open
  as they do in GNOME; nothing is configured per program.
+ It happens before the first frame: no window jumps from small to large.
- The plugin's scope grows beyond the overview (ADR 0051); without the plugin
  (another Hyprland version) windows open as Hyprland places them.
- Unmaximizing returns to the size the window had when it opened – for
  Firefox the placeholder. To check on the host; Mutter caps the size it
  restores to 80 % of the work area.
- Super+Up/Down (`koje snap`, M8.41) move and size the window but do not set
  Hyprland's maximized state; a window maximized here is restored by the
  program's own unmaximize or Super+Shift+F. To align later.
- Follow-up: M8.74.
