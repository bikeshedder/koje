# Spike setup (host)

Material for the M2 spikes. Everything here runs on the **host**, not in the dev
container. Host: Ubuntu 26.04 with GNOME (Wayland). Reports go to
`docs/spikes/M2.x-*.md` (skeletons are there already).

| File | Purpose |
|---|---|
| `base/Containerfile`, `build-base.sh` | throwaway spike image `localhost/koje-spike/base` (Fedora: Firefox as a real package, not a Snap) |
| `run-context.sh <name> [socket]` | hand-made context container `koje-spike-<name>` (prints the exact `podman run`) |
| `rm-context.sh <name>` | remove it and its home volume |
| `mem.sh <pattern> [container]` | RSS/PSS/shared MiB per process (M2.6) |
| `cgroup-mem.sh <container>` | cgroup memory of a context (M2.6) |
| `lib-pss.sh <pattern> <container> <lib>` | Rss/Pss of one library's mappings (M2.6, cross-context sharing) |
| `hyprland-nested.conf` | Hyprland nested in GNOME, ALT as modifier (step 2) |
| `color-contexts.sh` | colors windows by context in the nested Hyprland (A red, B blue) – kojed in miniature |
| `hyprland-session.conf` | starter config for a real Hyprland session (M2.1, M2.9): personal/machine settings; koje's part comes from `install.sh` |
| `wl-secure-socket <path> <context>` | Wayland socket via `security-context-v1` for one context (M2.9, Rust, build in the dev container) |
| `host-bus-proxy.sh <name>` | filtered view of the host's session bus for a context (M2.4) |
| `portal-check.py inhibit <s> \| watch-settings` | portal clients that stay connected (M2.11, runs in the context) |
| `bus-forward.py <name> <address>` | forwards one D-Bus name from the context bus to the host proxy (M2.4, runs in the context) |

Everything the scripts create carries the label `io.koje.spike=true` and the
name prefix `koje-spike-`, so cleanup never touches your other containers:

```sh
podman ps -a --filter label=io.koje.spike=true
```

## 0. Prepare (once)

```sh
sudo apt install podman passt fuse-overlayfs uidmap smem
podman info --format '{{.Host.Security.Rootless}} {{.Version.Version}}'   # true 5.x
grep "^$(id -un):" /etc/subuid /etc/subgid       # must show a range, e.g. 100000:65536
sysctl kernel.apparmor_restrict_unprivileged_userns   # note the value for M2.5
docs/spikes/setup/build-base.sh
```

After changing `base/Containerfile`, rebuild and recreate:
`build-base.sh && rm-context.sh a && run-context.sh a wayland-0`.

## 1. Under plain GNOME

**M2.3 – first pass** (`M2.3-context-container.md`)

```sh
docs/spikes/setup/run-context.sh a wayland-0          # copy the printed command into the report
podman exec -d koje-spike-a foot                      # terminal from the context
podman exec -d koje-spike-a firefox
podman exec koje-spike-a pw-cli info 0 | head -5    # PipeWire reachable? Then play a video in Firefox
podman exec koje-spike-a sh -c 'ls -l /dev/dri; eglinfo -B 2>/dev/null | head -20 || glxinfo -B'
# In the context's terminal: xdg-open https://example.org  → must open the context's Firefox
```

Check `about:support` in that Firefox for "Compositing: WebRender" (GPU) and
"Window Protocol: wayland".

**GPU with NVIDIA.** Mesa in the image only drives AMD/Intel, e.g. the Ryzen's
iGPU. To use an NVIDIA card with the proprietary driver:

```sh
# which card drives the monitor? (0x10de = NVIDIA, 0x1002 = AMD)
for c in /sys/class/drm/card?; do echo "$c $(cat $c/device/vendor)"; done
grep -l '^connected' /sys/class/drm/card*-*/status
# CDI for Podman (nvidia-container-toolkit from NVIDIA's apt repository)
sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml && nvidia-ctk cdi list
docs/spikes/setup/rm-context.sh a
KOJE_SPIKE_GPU=nvidia docs/spikes/setup/run-context.sh a wayland-0
podman exec koje-spike-a nvidia-smi
podman exec koje-spike-a sh -c 'eglinfo -B 2>/dev/null | grep -i renderer'
```

**M2.5 – nested Podman** (`M2.5-nested-podman.md`)

```sh
# 1. A dev container (works since the inner containers.conf, see report)
podman exec -it koje-spike-a podman run --rm docker.io/library/alpine \
  sh -c 'echo hello; ip addr; wget -qO- https://example.org | head -c 100'
podman exec koje-spike-a podman info --format '{{.Store.GraphDriverName}} {{.Host.IDMappings}}'

# 2. Compose: two services on their own network (repo mounted at the same path)
docs/spikes/setup/rm-context.sh a
KOJE_SPIKE_UNMASK=/proc/sys KOJE_SPIKE_MOUNTS="$PWD/docs/spikes/setup" docs/spikes/setup/run-context.sh a wayland-0
time podman exec -it koje-spike-a podman-compose -f "$PWD/docs/spikes/setup/compose/compose.yaml" \
  up --abort-on-container-exit        # expect: <title>Welcome to nginx!</title>

# 3. Startup time and disk usage of an inner container
podman exec koje-spike-a sh -c 'time podman run --rm docker.io/library/alpine true'
podman exec koje-spike-a podman unshare du -sh ~/.local/share/containers

# 4. Shared read-only image store: context a fills it, context b only reads it
mkdir -p ~/koje-spike-images
docs/spikes/setup/rm-context.sh a
KOJE_SPIKE_UNMASK=/proc/sys KOJE_SPIKE_MOUNTS="$HOME/koje-spike-images $PWD/docs/spikes/setup" docs/spikes/setup/run-context.sh a wayland-0
podman exec koje-spike-a podman --root "$HOME/koje-spike-images" pull docker.io/library/alpine
KOJE_SPIKE_UNMASK=/proc/sys KOJE_SPIKE_MOUNTS="$HOME/koje-spike-images:ro $PWD/docs/spikes/setup" docs/spikes/setup/run-context.sh b wayland-0
podman exec koje-spike-b sh -c "mkdir -p ~/.config/containers && sed 's|SHARED_STORE|$HOME/koje-spike-images|' \
  $PWD/docs/spikes/setup/storage-shared.conf > ~/.config/containers/storage.conf"
podman exec koje-spike-b podman images          # alpine listed, marked R/O
podman exec koje-spike-b podman run --rm docker.io/library/alpine echo from the shared store
podman exec koje-spike-b podman unshare du -sh ~/.local/share/containers   # should stay small
```

Then a dev container that VS Code *inside the context* attaches to:

```sh
# install VS Code into the running context (lost when the context is recreated)
podman exec -u 0 koje-spike-b sh -c 'rpm --import https://packages.microsoft.com/keys/microsoft.asc &&
  printf "[code]\nname=VS Code\nbaseurl=https://packages.microsoft.com/yumrepos/vscode\nenabled=1\ngpgcheck=1\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\n" \
  > /etc/yum.repos.d/vscode.repo && dnf -y install code'
podman exec -d koje-spike-b code --ozone-platform=wayland "$PWD/docs/spikes/setup/devcontainer-test"
```

In that VS Code: install the *Dev Containers* extension, set
`"dev.containers.dockerPath": "podman"` in the user settings, then
*Dev Containers: Reopen in Container*. Record time to first open, and whether a
file created in the dev container's terminal is owned by you (`ls -ln` in the
context). Cleanup of step 4: `rm-context.sh b`, `rm -rf ~/koje-spike-images`
(if `rm` is refused because of sub-uid owned files: `podman unshare rm -rf ~/koje-spike-images`).

**M2.6 – memory sharing** (`M2.6-memory-sharing.md`)

```sh
docs/spikes/setup/run-context.sh a && docs/spikes/setup/run-context.sh b
podman exec -d koje-spike-a firefox; podman exec -d koje-spike-b firefox
# open the same pages in both, wait a minute
docs/spikes/setup/mem.sh firefox koje-spike-a
docs/spikes/setup/mem.sh firefox koje-spike-b
docs/spikes/setup/cgroup-mem.sh koje-spike-a
# inside a: start a nested container that allocates memory, measure cgroup again
podman exec koje-spike-a podman run -d --name hog docker.io/library/alpine \
  sh -c 'head -c 500m /dev/zero | tail > /dev/null; sleep 600'
docs/spikes/setup/cgroup-mem.sh koje-spike-a
```

For the baseline: run the Fedora image's Firefox twice from *one* container
(`podman exec -d koje-spike-a firefox -P second --no-remote`) and compare.

## 2. With Hyprland nested in GNOME

```sh
sudo apt install hyprland foot
# XDG_CURRENT_DESKTOP: otherwise inherited from GNOME ("managed externally" notice).
# "started without start-hyprland" and "hyprland-guiutils" notices are harmless here.
XDG_CURRENT_DESKTOP=Hyprland Hyprland -c "$PWD/docs/spikes/setup/hyprland-nested.conf"   # opens as a window
ls "$XDG_RUNTIME_DIR"                                       # its socket, e.g. wayland-1
ls "$XDG_RUNTIME_DIR/hypr/"                                 # its IPC instance
```

Nested limits: the keyboard layout comes from the config (`input:kb_layout`),
and the clipboard is not shared with GNOME (`wl-paste | WAYLAND_DISPLAY=wayland-1 wl-copy`
copies GNOME's clipboard into the nested Hyprland). A real session has neither problem.

If Hyprland rejects lines of the config (the syntax changes between releases),
it shows the errors at the top of its window: note them in the M2.2 report.

**M2.2 – IPC from Rust** (`M2.2-hyprland-ipc.md`). Build in the dev container,
run on the host:

```sh
# dev container
cargo build --release --manifest-path docs/spikes/setup/hypr-ipc/Cargo.toml
# host (with the nested Hyprland running)
ipc=docs/spikes/setup/hypr-ipc/target/release/hypr-ipc
docs/spikes/setup/run-context.sh a wayland-1                 # a context on Hyprland
podman exec -d koje-spike-a foot; podman exec -d koje-spike-a firefox
$ipc clients                                                 # note addresses and pids
$ipc events &                                                # then open/close/move windows
$ipc move <address> 100 100 900 600
$ipc color <address> 2e7d5b
$ipc bench 5                                                 # open ≥ 5 windows first
kill %1
```

To check the pid: `grep libpod /proc/<pid>/cgroup` should name the context's
container.

**M2.4 – filtered D-Bus** (`M2.4-filtered-dbus.md`). Each context has its own
session bus (ADR 0015); a forwarder in the context hands selected names to a
filtered proxy of the host bus.

```sh
sudo apt install xdg-dbus-proxy
docs/spikes/setup/build-base.sh                     # image now has python3-gobject, notify-send

# terminal 1 (host): the filtered host bus for context a – leave it running
docs/spikes/setup/host-bus-proxy.sh a

# terminal 2 (host)
docs/spikes/setup/rm-context.sh a
KOJE_SPIKE_HOSTBUS=1 KOJE_SPIKE_MOUNTS="$PWD/docs/spikes/setup" docs/spikes/setup/run-context.sh a wayland-1
# the image's own xdg-desktop-portal may be activated on the context bus: stop it
podman exec koje-spike-a pkill -f xdg-desktop-portal
# in a loop: a forwarder exits when the proxy restarts and comes back.
# To restart forwarders: `podman exec koje-spike-a pkill -x python3` – not
# `pkill -f bus-forward.py`, which also kills the loops (same command line).
podman exec -d koje-spike-a sh -c "while :; do python3 $PWD/docs/spikes/setup/bus-forward.py org.freedesktop.Notifications \"\$KOJE_HOST_BUS\"; sleep 1; done >> /tmp/fwd-notify.log 2>&1"
podman exec -d koje-spike-a sh -c "while :; do python3 $PWD/docs/spikes/setup/bus-forward.py org.freedesktop.portal.Desktop \"\$KOJE_HOST_BUS\"; sleep 1; done >> /tmp/fwd-portal.log 2>&1"

# 1. notification from the context → host desktop
podman exec koje-spike-a notify-send koje "hello from context a"

# 2. OpenURI on the host is denied (straight to the proxy, no forwarder)
podman exec koje-spike-a sh -c 'gdbus call --address "$KOJE_HOST_BUS" -d org.freedesktop.portal.Desktop \
  -o /org/freedesktop/portal/desktop -m org.freedesktop.portal.OpenURI.OpenURI "" https://example.org {}'

# 3. file chooser portal through the forwarder
podman exec koje-spike-a gdbus call --session -d org.freedesktop.portal.Desktop \
  -o /org/freedesktop/portal/desktop -m org.freedesktop.portal.FileChooser.OpenFile "" "koje test" {}
podman exec koje-spike-a cat /tmp/fwd-notify.log /tmp/fwd-portal.log
```

For 3, a file dialog should open on the host; pick a file. In the context's
Firefox, `widget.use-xdg-desktop-portal.file-picker = 1` in `about:config` makes
File → Open use the portal too.

**M2.10 – more host interfaces** (`M2.10-host-interfaces.md`). Same setup as
M2.4 (proxy for `a` running in its own terminal, restart it after pulling, both
forwarders started in the context). Calls straight from the context bus:

```sh
call() { podman exec koje-spike-a gdbus call --session -d org.freedesktop.portal.Desktop \
  -o /org/freedesktop/portal/desktop -m "$@"; }

# settings: dark mode etc. (0 = default, 1 = dark, 2 = light)
call org.freedesktop.portal.Settings.ReadOne org.freedesktop.appearance color-scheme
# toggle dark mode in GNOME Settings now and look for "signal …SettingChanged" in the log:
podman exec koje-spike-a cat /tmp/fwd-portal.log | tail -3

# inhibit: flags 4 = suspend, 8 = idle; check on the host while it is held
call org.freedesktop.portal.Inhibit.Inhibit "" "uint32 12" "@a{sv} {'reason': <'koje test'>, 'handle_token': <'inh1'>}"
busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager GetInhibitors

# print: the host's print dialog should open
call org.freedesktop.portal.Print.PreparePrint "" "koje test" "@a{sv} {}" "@a{sv} {}" "@a{sv} {'handle_token': <'pr1'>}"
```

Screen sharing (fd passing through the forwarder): start Firefox in the context
*after* the forwarders, open
`https://mozilla.github.io/webrtc-landing/gum_test.html`, choose *Screen*, pick a
window in the host's dialog. The preview should show it.

```sh
podman exec -d koje-spike-a firefox
podman exec koje-spike-a cat /tmp/fwd-portal.log | grep -E "ScreenCast|OpenPipeWire"
```

File chooser in Firefox: `about:config` → `widget.use-xdg-desktop-portal.file-picker = 1`,
then *File → Open File*: the host's dialog should open.

**M2.7 – files between contexts** (`M2.7-files-between-contexts.md`). Two
contexts with a shared exchange folder at the same path:

```sh
mkdir -p ~/Austausch && echo "shared" > ~/Austausch/shared.txt
docs/spikes/setup/rm-context.sh a; docs/spikes/setup/rm-context.sh b
KOJE_SPIKE_MOUNTS="$HOME/Austausch" docs/spikes/setup/run-context.sh a wayland-1
KOJE_SPIKE_MOUNTS="$HOME/Austausch" docs/spikes/setup/run-context.sh b wayland-1
podman exec koje-spike-a sh -c 'echo "only in a" > ~/only-in-a.txt'
podman exec -d koje-spike-a nautilus "$HOME"
podman exec -d koje-spike-b firefox
podman exec -d koje-spike-a foot; podman exec -d koje-spike-b foot
```

1. Drag `Austausch/shared.txt` from a's Nautilus onto b's Firefox (tab bar) → opens?
2. Drag `only-in-a.txt` the same way → expected: fails ("file not found").
3. Drag a file onto b's `foot` → what is pasted (path)?
4. Clipboard: select text in a's `foot`, Ctrl+Shift+C; paste in b's `foot` with
   Ctrl+Shift+V. Same with an image copied in a's Firefox (right click → *Copy Image*)
   pasted into b's Firefox (e.g. a new mail/web editor).

**M2.8 – pausing** (later, same setup): `podman pause koje-spike-a`, wait a minute
(does Hyprland show "not responding"?), `podman unpause koje-spike-a`; then the
same with the windows moved to `special:stash` (ALT+H) first.

## 3. Real Hyprland session (M2.1, M2.9)

**Set up once** (GNOME stays available as a session at the login screen):

```sh
# installs what exists; apt would cancel everything on one unknown name.
# hyprland-guiutils (new name) / hyprland-qtutils (old name): Hyprland's own dialogs.
want="hyprland hyprland-guiutils hyprland-qtutils hyprlock hypridle waybar mako-notifier fuzzel \
  foot grim slurp wl-clipboard xdg-desktop-portal-hyprland xdg-desktop-portal-gtk policykit-1-gnome \
  fonts-font-awesome"
have=""; missing=""
for p in $want; do
  if apt-cache show "$p" >/dev/null 2>&1; then have="$have $p"; else missing="$missing $p"; fi
done
echo "missing:$missing"
sudo apt install $have
mkdir -p ~/.config/hypr
[ -e ~/.config/hypr/hyprland.conf ] || printf 'source = %s\n\n# your own additions below\n' \
  "$PWD/docs/spikes/setup/hyprland-session.conf" > ~/.config/hypr/hyprland.conf
docs/spikes/setup/build-base.sh                                  # image now has grim
# dev container:
cargo build --release --manifest-path docs/spikes/setup/hypr-ipc/Cargo.toml
cargo build --release --manifest-path docs/spikes/setup/wl-secure-socket/Cargo.toml
```

koje's own Hyprland part (floating, Super+mouse, snapping, overlays, stash), the
icon font and koje's Waybar config come from the installer (ADR 0021):

```sh
./install.sh --dry-run && ./install.sh
```

Log out, choose **Hyprland** at the login screen. With the spike config:
Super+Return terminal, Super+D launcher, Super+Escape lock, Super+Shift+E leave.
(Since ADR 0037 koje's snippet itself carries the session except Super+D.) Missing packages or
config errors (shown at the top of the screen): note them in the M2.1 report.

**M2.1 – daily use** (`M2.1-hyprland-daily.md`): use it for a week; note what
works, what annoys. Plus the three checks collected during the nested spikes:

```sh
docs/spikes/setup/run-context.sh a wayland-1      # the session's socket: ls $XDG_RUNTIME_DIR | grep wayland
podman exec -d koje-spike-a nautilus "$HOME"      # GTK 4 without GSK_RENDERER=gl: black?
podman exec -d koje-spike-a foot
podman pause koje-spike-a; sleep 60; podman unpause koje-spike-a   # "not responding" dialog now?
```

**M2.9 – screen capture** (`M2.9-screen-capture.md`):

```sh
# baseline: host and context can both take a screenshot
grim /tmp/host.png && echo host ok
podman exec koje-spike-a grim /tmp/ctx.png && echo context ok

# 1. what path does Hyprland see for a context program?
podman exec -d koje-spike-a sleep 300
ls -l /proc/$(pgrep -n -f "sleep 300")/exe       # host view of the context's binary

# 2. permission rules: uncomment the M2.9 block in hyprland-session.conf, `hyprctl reload`,
#    then repeat the baseline – which grim is denied?

# 3. security context: a context-only Wayland socket
docs/spikes/setup/wl-secure-socket/target/release/wl-secure-socket "$XDG_RUNTIME_DIR/koje-a-wayland" a &
docs/spikes/setup/rm-context.sh a
docs/spikes/setup/run-context.sh a "$XDG_RUNTIME_DIR/koje-a-wayland"
podman exec -d koje-spike-a foot                 # still opens a window?
podman exec koje-spike-a grim /tmp/ctx.png       # screencopy denied for sandboxed clients?
```

Then screen sharing through the portal from that context (M2.10 setup: proxy,
forwarders, Firefox gum_test) – the portal (`xdg-desktop-portal-hyprland`) must
still be able to capture.

## Cleanup

```sh
docs/spikes/setup/rm-context.sh a; docs/spikes/setup/rm-context.sh b
podman rmi localhost/koje-spike/base:latest
```
