# 0054. The shell draws the background from GNOME's settings

- Status: Accepted
- Date: 2026-10-27
- Source: plan tasks M8.82, M8.83 (requested 2026-10-26: the background
  changeable with GNOME Settings, GNOME's menu on a right click)

## Context

Under Hyprland something has to draw the desktop's background; with
`disable_hyprland_logo` koje's session showed a plain color. GNOME keeps the
background in `org.gnome.desktop.background` (picture, dark-style picture,
how it is fitted, colors), and GNOME Settings' Appearance panel writes
there. GNOME also opens a menu on a right click on the background (Change
Background…, Display Settings, Settings). A wallpaper program (hyprpaper,
swaybg) can draw a picture, but it knows nothing of GNOME's settings and its
surface takes no clicks koje could answer.

## Decision

- **koje-shell draws the background**: one layer-shell surface per monitor
  on the `background` layer, under the bar too, kept with the monitor's bar.
- It shows what `org.gnome.desktop.background` says – `picture-uri`, or
  `picture-uri-dark` in the dark style, fitted as `picture-options` says
  (GNOME's seven options; `spanned` per monitor as `zoom`), on
  `primary-color`/`secondary-color` with `color-shading-type`, at
  `picture-opacity` – and follows changes. GNOME's XML slideshows show their
  first picture. The picture is composed for the monitor's pixel size off
  the GTK main loop.
- A right click opens GNOME Shell's background menu; its entries open the
  same desktop files GNOME Shell does (`gnome-background-panel.desktop`,
  `gnome-display-panel.desktop`, `org.gnome.Settings.desktop`), with
  `XDG_CURRENT_DESKTOP` naming GNOME as the Quick Settings button does. koje
  has no picker of its own.
- No wallpaper program in the session.

## Consequences

+ GNOME Settings changes the background, light and dark, as on GNOME.
+ The menu works because the surface is koje's.
- A user's own wallpaper program would be covered by the shell's surface;
  there is no switch to turn the shell's background off yet.
- Slideshows do not change pictures over time.
- koje's overview plugin (ADR 0051) draws over the background layer – to be
  checked on the host.
