# GNOME lab (dev container)

GNOME Shell 50.1 (Ubuntu 26.04, as on the host) nested in the dev container,
for the G spikes (ADR 0059). Its session opens as a window on your desktop
(Mutter's devkit, through the Wayland socket VS Code forwards into the dev
container). Spike contexts are containers on the lab's Wayland socket, made
by the usual `run-context.sh`. Nothing touches your desktop session, your
containers or your GNOME settings.

```sh
cd docs/spikes/setup/gnome-lab
./lab.sh build                 # once: localhost/koje-gnome-lab (Ubuntu 26.04 + gnome-shell)
../build-base.sh               # once: the spike context image localhost/koje-spike/base
./lab.sh start                 # the "Mutter Development Kit" window opens
./lab.sh context a             # context container koje-spike-a on the lab
podman exec -d koje-spike-a foot
./lab.sh run foot              # a host client of the lab, for comparison
./lab.sh eval 'global.get_window_actors().map(a => a.meta_window.get_wm_class() + ":" + a.meta_window.get_pid()).join(" ")'
./lab.sh freeze a; ./lab.sh thaw a
./lab.sh stop                  # the lab and every spike container
```

koje's extension as `just ui` builds it (`gnome/build/extension/koje@koje.io`) is mounted from the
repository and enabled; after changing it, `just ui`, then `./lab.sh stop && ./lab.sh start`.
The lab's session bus is `$XDG_RUNTIME_DIR/koje-gnome-lab/run/bus`, reachable
from the dev container: `just test-extension` runs the adapter against the
extension, and `DBUS_SESSION_BUS_ADDRESS=unix:path=… gdbus call --session -d
io.koje.Extension …` calls it.

Run `lab.sh` from a VS Code terminal of the dev container (it needs
`WAYLAND_DISPLAY` from VS Code). `./lab.sh` without arguments lists the
commands.

Without it (an agent's shell), `./lab.sh start --headless` runs GNOME Shell on
a virtual monitor instead of in a window; `eval` and the tests work the same.

## How it is built

| Part | What |
|---|---|
| `Containerfile` | Ubuntu 26.04 with gnome-shell, mutter-dev-bin (the devkit), PipeWire, the portals, foot, wayland-utils |
| `lab-session.sh` | the container's entry point: `XDG_CURRENT_DESKTOP=GNOME` (GNOME's Mutter defaults, such as dynamic workspaces, apply only then), a stand-in system bus, the session bus in the runtime directory, PipeWire, then WirePlumber (it spins at full CPU if started together with PipeWire), then `gnome-shell --devkit --wayland --no-x11` |
| `lab@koje.io/` | lab-only extension: switches on unsafe mode, so `lab.sh eval` (`org.gnome.Shell.Eval`) works from the dev container |
| `lab.sh` | start, stop, eval, run, exec, context, freeze, thaw; mounts koje's extension |

The lab runs with `--pid=host` (the dev container's PID namespace; Podman needs
`--cgroups=enabled` for that here). Contexts keep their own PID namespaces, so
Mutter sees their real PIDs as on the host (`Meta.Window.get_pid()`, window →
context, L2). In a sibling namespace it would see 0. Its processes outlive
the container's main process, so `lab.sh stop` kills everything in the lab's
mount namespace.

## Differences from the host

| | Host | Lab |
|---|---|---|
| Rendering | GPU | software (llvmpipe), no `/dev/dri`; Firefox works but is slow |
| Pausing a context | `podman pause` (cgroup freezer) | `lab.sh freeze` (SIGSTOP to every process but the init); for Mutter the same: the client stops reading its socket |
| System services | logind, polkit, NetworkManager, UPower, GDM | none (a stand-in system bus); GNOME logs warnings and goes on |
| Sound | the host's PipeWire | the lab's own PipeWire (no devices); enough for the screen-cast portal |
| Context files from the shell | GNOME Shell reads `/proc/<pid>/root/…` of a context (its app icons) | denied: the lab runs in a sibling user namespace; context apps show without icons |
| Screenshots | GNOME's | `org.gnome.Shell.Screenshot` never returns on this software-rendered virtual monitor; look at the devkit window instead |
