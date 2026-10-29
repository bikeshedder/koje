# G.3 – Freezing a context under Mutter

- Date:
- Host: Ubuntu 26.04, kernel, GNOME Shell / Mutter version, Podman version

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

| Frozen for | "Not responding" dialog | Windows still there after resume | Journal |
|---|---|---|---|
| 1 min | | | |
| 10 min | | | |
| 30 min | | | |

## Conclusion

Can koje's sleep stay as it is on GNOME (G.11)? If windows are lost: what
would have to change (Mutter's buffer size, hiding windows first, …)?
