# 0050. koje's own screen sharing picker

- Status: Accepted
- Date: 2026-10-26
- Source: M8.65 ([report](../spikes/M8.65-share-picker.md)), M8.66

## Context

Screen sharing in the koje session goes through Hyprland's portal backend,
xdph (ADR 0048, `koje-portals.conf`). Its picker, `hyprland-share-picker`, is a
Qt list of window classes and titles: with several contexts running the same
programs, the entries say little, and it looks nothing like the rest of the
session (ADR 0043). GNOME's picker needs Mutter.

xdph 1.3.11 runs `screencopy:custom_picker_binary` instead when set. The picker
gets every window in `XDPH_WINDOW_SHARING_LIST` – xdph's handle, class, title
and Hyprland's window address –, `--allow-token` when
`screencopy:allow_token_by_default` is on, nothing about who asks; it answers
`[SELECTION]<flags>/window:<handle>`, `/screen:<output>` or
`/region:<output>@x,y,w,h` on stdout (`r` grants a restore token), anything
else cancels.

xdph reads its config only from `$XDG_CONFIG_HOME/hypr/xdph.conf`, else
`~/.config/hypr/xdph.conf`; koje never touches `~/.config/hypr/` (ADR 0048).

## Decision

- **koje ships its own picker**: `koje-shell share-picker`, a short-lived
  process of the shell binary (like the settings app, ADR 0042), started by
  xdph through `/usr/lib/koje/koje-share-picker`. It is a card on a layer-shell
  surface like the shell's other dialogs, laid out as GNOME's: *Windows* and
  *Screens*, *Remember this selection*, Cancel and Share.
- **Windows are grouped by context** – the asking context's guess (the focused
  window's) first, then the other contexts, then the host's windows –, each
  group under the context's name in its color; a window shows its thumbnail
  (the overview's capture, M8.67), its app's icon and name and its title;
  stashed windows are marked. Screens show their name and model. No region
  (GNOME has none either).
- **What it shows is a pure function** of xdph's list, Hyprland's windows
  (the context from kojed's window tag), kojed's context colors and the
  monitors; the reply is formatted by one too. Without kojed it still works,
  without colors.
- **xdph's config in the koje session is koje's**: a drop-in for
  `xdg-desktop-portal-hyprland.service` reads `$XDG_RUNTIME_DIR/koje/xdph.env`
  if it exists. `koje session start` writes it (`XDG_CONFIG_HOME` pointing at
  `$XDG_RUNTIME_DIR/koje/xdph`, and the user's own `XDG_CONFIG_HOME`) and
  `xdph/hypr/xdph.conf` there: koje's picker, then the user's
  `~/.config/koje/xdph.conf`; both are removed when the session ends. A plain
  Hyprland session keeps the user's own `xdph.conf`.
- The wrapper gives the picker the user's `XDG_CONFIG_HOME` back and falls back
  to `hyprland-share-picker` when koje's picker fails to start, so sharing
  never depends on the shell working.

## Consequences

+ Shared windows are recognizable by context, app and picture, in GNOME's look.
+ No change to xdph or the portal; the stock picker remains the fallback.
- Who asks is unknown: the focused window's context is only a guess, so it
  orders the list but selects nothing.
- `XDG_CONFIG_HOME` is changed for xdph only; anything xdph starts besides the
  picker would see it.
- The picker depends on xdph's undocumented picker interface; a new xdph
  version may need changes (`koje doctor` shows the version later).
- Follow-ups: screen thumbnails; programs' icons from inside contexts (M8.18).
