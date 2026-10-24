# 0041. Volume, brightness and media keys through `koje media`, with an OSD

- Status: Accepted (the OSD toolkit revised by ADR 0042: GTK4 instead of Kiesel Desktop)
- Date: 2026-10-24
- Source: discussion (after `koje setup` replaced Hyprland's default config, the
  laptop's brightness and volume keys were dead); extends ADR 0037

## Context

Hyprland's generated default config binds the volume, brightness and media keys
to `wpctl`, `brightnessctl` and `playerctl`. koje replaces that config
(ADR 0037), so its snippet has to bind them itself. Three things do not work
with those commands alone:

- **Brightness:** Ubuntu builds `brightnessctl` without logind. It writes
  `/sys/class/backlight/*/brightness` itself, which needs the group `video`.
  GNOME needs no group: it asks systemd-logind (`Session.SetBrightness`), which
  lets the owner of the active session on the seat set the backlight. Tested on
  the host: the call works in the Hyprland session without the group.
- **Media keys:** `playerctl` sees the MPRIS players on the host's session bus
  only. Every context has its own bus (ADR 0015), so a browser or music player
  in a context never got the keys.
- **Feedback:** GNOME shows an on-screen display (OSD) with the new level;
  Hyprland shows nothing.

Volume needs no change: the contexts play through the host's PipeWire
(ADR 0017, 0029), so `wpctl` on the default sink sets the volume for all of them.

## Decision

- **One command for all these keys:** `koje media <action>` – `volume-up`,
  `volume-down`, `mute`, `mic-mute`, `brightness-up`, `brightness-down`,
  `play-pause`, `next`, `previous`. koje's snippet binds the `XF86…` keys to it
  (repeat when held for volume and brightness; also on the lock screen).
- **Volume and microphone:** `wpctl` on `@DEFAULT_AUDIO_SINK@` /
  `@DEFAULT_AUDIO_SOURCE@`, 5 % steps, at most 100 %.
- **Brightness:** koje sets it through logind (`SetBrightness` on the session
  `auto`, system bus) – no `brightnessctl`, no group `video`. The device is the
  first backlight in `/sys/class/backlight` by kind (`firmware`, `platform`,
  `raw`, as GNOME). Steps of 5 % on a perceptual scale (exponent 4, as
  `brightnessctl -e4`), never fully dark.
- **Media keys go to the most recently active player**, as in GNOME: the MPRIS
  player that last started playing, on the host or in any running context.
  kojed tracks it: it watches `PlaybackStatus` on the host's session bus and on
  each running context's bus. It reaches a context's bus from the host at
  `/proc/<container pid>/root/run/user/<uid>/bus` – same user (`keep-id`), no
  process in the container, no recreate (tested on the host). Without a record
  (kojed just started) it takes a playing player, then a paused one, then any;
  a player that does not answer is skipped for the next. The choice is a pure
  function in `koje_core::media` (`player_order`). The media keys need kojed
  (D-Bus `MediaPlayer`); without it they do nothing.
- **OSD:** after a volume or brightness change `koje media` shows the level as a
  notification with a progress value (`value` hint) that disappears after a
  moment – shown by mako for now. It replaces the previous one through the hint
  `x-canonical-private-synchronous`, not `replaces_id`: mako 1.10 crashes when
  a notification has both (tested on the host). The display lives
  in one place, so a GTK4 layer-shell OSD (ADR 0042) can replace it
  later without touching the bindings. Media keys show no OSD.

## Consequences

+ Brightness works after `./install.sh` without a group or a new login, as in
  GNOME; `brightnessctl` and `playerctl` are not needed.
+ Play/pause reaches players in contexts.
+ All key logic is in one command – testable, and the OSD is replaceable.
- kojed holds one bus connection per running context. It is a client there and
  treats everything it reads from a context's bus as untrusted (no names or
  metadata into commands).
- A paused context's bus does not answer: kojed connects only to running
  contexts (it checks every few seconds) and calls into context buses with a
  timeout.
- The OSD looks like a notification until koje's own OSD exists (M8.26).
