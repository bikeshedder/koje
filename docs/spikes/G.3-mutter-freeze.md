# G.3 – Freezing a context under Mutter

- Date: 2026-10-29
- Host: **GNOME lab** in the dev container ([gnome-lab](setup/gnome-lab/README.md)): GNOME Shell / Mutter 50.1 (Ubuntu 26.04 packages), software rendering; context from `localhost/koje-spike/base` (Fedora 44) with foot and Files (Nautilus); frozen with `lab.sh freeze` (SIGSTOP to all 16 processes of the context – for Mutter the same as the cgroup freezer: the client stops reading its socket)

## Question

`koje sleep` freezes a context's cgroup (M2.8 on Hyprland: windows survived).
Under Mutter: does GNOME show "not responding" for every frozen window? Does
Mutter disconnect frozen clients when their socket buffer fills, so windows
are lost? Decides L8 (ADR 0059).

## Setup

In the lab instead (dev container, [gnome-lab](setup/gnome-lab/README.md)): `./lab.sh start`, then use
`./lab.sh context a` for `run-context.sh a wayland-0`, and `./lab.sh freeze a` / `./lab.sh thaw a` for `podman pause` / `unpause` (SIGSTOP instead of the cgroup freezer). For the journal, use `podman logs koje-gnome-lab`. Note in the
report that it ran in the lab.

```sh
docs/spikes/setup/run-context.sh a wayland-0
podman exec -d koje-spike-a foot
podman exec -d koje-spike-a firefox
podman pause koje-spike-a
# While frozen: hover, click, type into the windows, move and resize them,
# switch workspaces, open the overview. Note what GNOME shows and when.
# Resume after 1 min, again after 10 min, again after 30 min:
podman unpause koje-spike-a
journalctl --user -b -g 'wayland|client|disconnect' --since '-35 min' | tail -50
```

## Result

Run by the agent in the lab. Input came from a driver inside GNOME Shell
(`lab.sh eval`): a virtual pointer and keyboard, `Meta.Window.activate`,
`move_resize_frame`. The state was read from `Meta.Window.is_alive` and the
shell's mapped dialogs.

| Run | While frozen | Frozen for | "Not responding" | After thaw | Mutter log |
|---|---|---|---|---|---|
| 1 | flood: every 200 ms each context window focused, resized by 20 px, pointer moved over it, a key typed | 1 min | after < 10 s, one dialog per window (`close-dialog`), `is_alive = false` | windows there, dialogs gone by themselves, typed keys arrive (Files' search held the "a"s) | – |
| 2 | the same flood | 10 min | as run 1 | **all context windows gone** | `WL: Data too big for buffer (1048564 + 20 > 1048576)` about 3 min in; at the thaw `WL: error in client communication` for both programs |
| 3 | windows parked on their own workspace; the user on another, looking at theirs for 10 s every 5 min, no input to them | 30 min | **never**: `is_alive` stayed `true` (no focus, no ping) | windows there, nothing lost | – |
| 4 | pointer moved over a frozen window 500 times/s, no click, no focus | 201 s (97,666 motion events) | never | windows there | – |

- Mutter queues at most **1 MiB per client**. When a frozen client's queue
  is full, Mutter disconnects it once it can write again, and every window of
  that program is gone. Hyprland kept such windows (M2.8).
- Focus is what triggers the ping, and so the dialog. Moving and resizing a
  window, typing into it and focusing it fill the queue. Pointer motion fills
  it slowly: Mutter sends at most one motion event per frame, so run 4 did not
  fill it in 200 s.

## Conclusion

- **koje's sleep works on GNOME if sleeping windows get no input.** Untouched
  frozen windows survive (run 3), with no dialog.
- **G.11 must keep input away from sleeping windows**, not only draw the
  overlay:
  - the overlay is a reactive actor over each sleeping window and catches
    pointer input ("Wake up" is the only way through);
  - the extension refuses to focus sleeping windows (`activate`, Alt+Tab,
    the dash, clicks), so Mutter does not ping them and shows no dialog;
  - no moves or resizes of sleeping windows (also not from layout/state
    restore), and GNOME's close dialog stays suppressed for them in case a
    ping still happens;
  - kojed: `koje wake` before any action that sends events to a context's windows.
- Optional extra safety: before freezing, move a context's windows to its own
  workspace if they are elsewhere (on Hyprland: the stash, M7.4).
- Upstream ask stays: a per-window "suspended" state (no pings, events
  dropped or coalesced instead of queued). `docs/upstream/gnome-apis.md`.
- Recheck on the host with `podman pause` and a real GPU once G.11 exists.
