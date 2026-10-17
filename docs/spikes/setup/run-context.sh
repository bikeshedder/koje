#!/usr/bin/env bash
# Start a hand-made context container for the M2 spikes (M2.3, M2.5, M2.6).
#
#   run-context.sh <name> [wayland-socket]     e.g.  run-context.sh a wayland-0
#
# Wayland socket: `wayland-0` is usually GNOME, `wayland-1` a nested Hyprland
# (check: ls "$XDG_RUNTIME_DIR"). The container runs a D-Bus session bus; start
# programs with:  podman exec -d koje-spike-<name> firefox
# Remove with:     rm-context.sh <name>
set -euo pipefail

name=${1:?usage: run-context.sh <name> [wayland-socket]}
wl=${2:-${WAYLAND_DISPLAY:-wayland-0}}
run=${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR not set}
uid=$(id -u)

[[ -S "$run/$wl" ]] || { echo "no Wayland socket $run/$wl" >&2; exit 1; }

args=(
  --name "koje-spike-$name"
  --hostname "$name"
  # A real PID 1 (catatonit): forwards SIGTERM and reaps orphans. Without it,
  # `sleep infinity` ignores SIGTERM and every stop waits 10 s for SIGKILL.
  --init
  --label io.koje.spike=true
  --label "io.koje.context=spike-$name"
  # Same uid inside as on the host; files in mounts keep their owner.
  --userns=keep-id
  # SELinux hosts only (no effect on Ubuntu/AppArmor): the Wayland/PipeWire
  # sockets are not labelled for containers.
  --security-opt label=disable
  # Home volume at the host's home path (concept: same paths as on the host).
  # :U chowns the fresh volume to the user.
  -v "koje-spike-$name-home:$HOME:U"
  -e HOME="$HOME" -w "$HOME"
  # Runtime dir with only the sockets the context may use.
  # U=true chowns it to the container user (podman's --tmpfs has no uid=/gid=).
  --mount "type=tmpfs,destination=/run/user/$uid,tmpfs-mode=0700,U=true"
  -e XDG_RUNTIME_DIR="/run/user/$uid"
  # The context's own D-Bus session bus (main process, see below). Firefox finds
  # its running instance over D-Bus on Wayland; without it, xdg-open starts a
  # second Firefox ("already running, but not responding").
  -e DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus"
  -v "$run/$wl:/run/user/$uid/$wl"
  -e WAYLAND_DISPLAY="$wl"
  -e MOZ_ENABLE_WAYLAND=1 -e GDK_BACKEND=wayland -e QT_QPA_PLATFORM=wayland
  # Chromium/Electron (VS Code, Chrome) pass frames and IPC through /dev/shm;
  # Podman's 64 MB default crashes their renderers ("reason: 'crashed', code 4").
  # A limit, not reserved memory. Override: KOJE_SPIKE_SHM=2g
  --shm-size "${KOJE_SPIKE_SHM:-1g}"
  # GPU (Mesa drivers in the image: AMD/Intel). NVIDIA: see KOJE_SPIKE_GPU below.
  --device /dev/dri
  # Nested Podman (M2.5): fuse-overlayfs for storage, tun for pasta networking
  # of the inner containers (without it: "Failed to open() /dev/net/tun").
  --device /dev/fuse
  --device /dev/net/tun
)

# Sound via PipeWire (and its PulseAudio compatibility socket, if present).
[[ -S "$run/pipewire-0" ]] && args+=(-v "$run/pipewire-0:/run/user/$uid/pipewire-0")
# The PulseAudio socket goes to its own path: under /run/user/<uid>/pulse/ the
# mount point directory would belong to root, and libpulse refuses it ("Failed
# to create secure directory"). PULSE_SERVER points clients (Firefox) at it.
[[ -S "$run/pulse/native" ]] && args+=(
  -v "$run/pulse/native:/run/user/$uid/pulse-native"
  -e PULSE_SERVER="unix:/run/user/$uid/pulse-native"
)

# NVIDIA's proprietary driver needs its host libraries inside the container,
# injected via CDI (nvidia-container-toolkit): KOJE_SPIKE_GPU=nvidia
[[ "${KOJE_SPIKE_GPU:-}" == nvidia ]] && args+=(--device nvidia.com/gpu=all)

# Spike-only escape hatch for M2.5 on Ubuntu: if nested Podman fails with
# "permission denied" on user namespaces, retry with KOJE_SPIKE_APPARMOR=unconfined
# and record it in the report (it must not end up in koje unexamined).
[[ -n "${KOJE_SPIKE_APPARMOR:-}" ]] && args+=(--security-opt "apparmor=$KOJE_SPIKE_APPARMOR")

# M2.5: paths of /proc to unmask in the context, e.g. KOJE_SPIKE_UNMASK=/proc/sys
# (compose bridge networks: netavark writes per-netns sysctls) or ALL.
[[ -n "${KOJE_SPIKE_UNMASK:-}" ]] && args+=(--security-opt "unmask=$KOJE_SPIKE_UNMASK")

# Extra folders at the same path, e.g. KOJE_SPIKE_MOUNTS="$HOME/Austausch"
# A `:ro` suffix mounts read-only: KOJE_SPIKE_MOUNTS="$HOME/koje-spike-images:ro"
for m in ${KOJE_SPIKE_MOUNTS:-}; do
  case "$m" in
    *:ro) args+=(-v "${m%:ro}:${m%:ro}:ro") ;;
    *) args+=(-v "$m:$m") ;;
  esac
done

# Main process: the context's session bus (it keeps the container running).
cmd=(dbus-daemon --session --nofork --nopidfile --address="unix:path=/run/user/$uid/bus")

# Print the exact command for the report, then run it.
printf 'podman run -d'; printf ' %q' "${args[@]}" localhost/koje-spike/base:latest "${cmd[@]}"; echo
podman run -d "${args[@]}" localhost/koje-spike/base:latest "${cmd[@]}"
