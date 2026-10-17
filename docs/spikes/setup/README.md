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
| `hyprland-nested.conf` | Hyprland nested in GNOME, ALT as modifier (step 2) |

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

## 2. With Hyprland nested in GNOME (later)

```sh
sudo apt install hyprland foot
Hyprland -c "$PWD/docs/spikes/setup/hyprland-nested.conf"   # opens as a window
ls "$XDG_RUNTIME_DIR"                                       # new socket, e.g. wayland-1
docs/spikes/setup/run-context.sh a wayland-1
```

Then M2.2, M2.4, M2.8 and M2.3 again against Hyprland.

## Cleanup

```sh
docs/spikes/setup/rm-context.sh a; docs/spikes/setup/rm-context.sh b
podman rmi localhost/koje-spike/base:latest
```
