# 0024. One koje overlay for sleeping and hanging windows

- Status: Accepted
- Date: 2026-10-19
- Source: M2.8 recheck in a real session, discussion

## Context

`koje sleep` freezes a context (cgroup freezer). Hyprland then shows its
"Application Not Responding" dialog for every window of that context – also for
windows in the stash. Hyprland detects hangs with the Wayland `xdg_wm_base`
ping/pong (X11: `_NET_WM_PING`); only the compositor can ping a client, so koje
cannot detect hangs itself. Hyprland exposes no per-window "not responding"
state over IPC, has no per-window rule to suppress the dialog, and with
`misc:enable_anr_dialog = false` leaves no trace at all. Hiding the dialog by a
window rule and reading the helper's command line was rejected as a hack (brief
mapping/focus, depends on dialog text).

## Decision

- koje's Hyprland snippet turns the dialog off: `misc:enable_anr_dialog = false`.
- **Sleeping windows:** kojed knows which context sleeps and which windows
  belong to it (pid → cgroup); koje's UI layer shows an overlay on each of them,
  "<context> is asleep – Wake up" (M8.6).
- **Hanging windows:** the same overlay will show "not responding – Wait / Quit"
  once Hyprland reports unresponsive windows over IPC. koje proposes that
  upstream (`docs/upstream/hyprland-anr-ipc.md`): events
  `windowunresponsive>>ADDRESS` / `windowresponsive>>ADDRESS`, independent of the
  dialog setting. Fallback if declined: a small Hyprland plugin.

## Consequences

+ No dialog storm for sleeping contexts; one consistent overlay, with a direct
  "Wake up".
- Until Hyprland offers the event, real hangs are not marked at all (as with the
  dialog switched off).
- Rejected: a Wayland proxy in front of every context (koje sending its own
  pings) – a full protocol proxy for one feature.
