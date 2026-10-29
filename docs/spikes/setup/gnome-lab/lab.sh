#!/usr/bin/env bash
# GNOME lab: GNOME Shell 50 (Ubuntu 26.04, as on the host) nested in the dev
# container, for the G spikes (ADR 0059). See README.md here.
#
#   lab.sh build                 build the image localhost/koje-gnome-lab
#   lab.sh start                 start GNOME Shell; its window opens on your desktop
#   lab.sh stop                  stop it and its contexts
#   lab.sh eval '<js>'           run JavaScript in the shell (unsafe mode, lab only),
#                                e.g. lab.sh eval 'global.get_window_actors().map(a => a.meta_window.get_wm_class()).join()'
#   lab.sh run <program> [args]  start a program as a host client of the nested shell
#   lab.sh exec <cmd> [args]     run a command in the lab with its session bus and Wayland
#   lab.sh context <name> [image] a context container (koje-spike-<name>) on the nested shell
#   lab.sh freeze|thaw <name>    stop / continue every process of a context (SIGSTOP:
#                                the dev container has no cgroup freezer)
#
# Everything carries the label io.koje.spike=true; containers are named
# koje-gnome-lab and koje-spike-<name>.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
image=localhost/koje-gnome-lab
lab=koje-gnome-lab
run=${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR not set}/koje-gnome-lab/run
uid=$(id -u)
# The lab's mount namespace, kept outside $run (start wipes that).
lab_ns_file=$run/../lab-mnt-ns

# Kill every process of the lab. With --pid=host they outlive the container's
# main process, so podman cannot stop them; they all share its mount namespace.
kill_lab() {
  [[ -f "$lab_ns_file" ]] || return 0
  local ns p
  ns=$(cat "$lab_ns_file")
  [[ -n "$ns" && "$ns" != "$(readlink /proc/self/ns/mnt)" ]] || return 0
  for p in /proc/[0-9]*; do
    [[ $(readlink "$p/ns/mnt" 2>/dev/null) == "$ns" ]] && kill -KILL "${p#/proc/}" 2>/dev/null
  done
  rm -f "$lab_ns_file"
}

# Run in the lab with GNOME Shell's session bus and the nested Wayland.
# IN_LAB_OPT=-d detaches.
in_lab() {
  podman exec "${IN_LAB_OPT:--i}" "$lab" sh -c '
    [ -S "$XDG_RUNTIME_DIR/wayland-0" ] || { echo "GNOME Shell is not running (lab.sh start)" >&2; exit 1; }
    DBUS_SESSION_BUS_ADDRESS=$(cat "$XDG_RUNTIME_DIR/lab-session-bus")
    export DBUS_SESSION_BUS_ADDRESS
    export WAYLAND_DISPLAY=wayland-0
    exec "$@"' lab "$@"
}

case ${1:-} in
  build)
    podman build -t "$image" "$here"
    ;;
  start)
    # VS Code forwards the desktop's Wayland into the dev container; the
    # devkit window of the nested shell opens there.
    parent=$(readlink -f "$run/../../${WAYLAND_DISPLAY:?no WAYLAND_DISPLAY: start from a VS Code terminal}")
    [[ -S "$parent" ]] || { echo "no Wayland socket $parent" >&2; exit 1; }
    kill_lab
    podman rm -f "$lab" >/dev/null 2>&1 || true
    rm -rf "$run" && mkdir -p "$run" && chmod 700 "$run"
    # The dev container's PID namespace (--pid=host; Podman needs
    # --cgroups=enabled for it here): Mutter must see context PIDs (window →
    # context, L2), as on the host where it is in an ancestor namespace.
    podman run -d --name "$lab" --label io.koje.spike=true \
      --userns=keep-id --security-opt label=disable \
      --cgroups=enabled --pid=host \
      -v "$run:/run/user/$uid" -e XDG_RUNTIME_DIR="/run/user/$uid" \
      -v "$parent:/run/host-wayland/wayland-0" -e WAYLAND_DISPLAY=/run/host-wayland/wayland-0 \
      -v "$here/lab-session.sh:/usr/local/bin/lab-session:ro" \
      -v "$here/lab@koje.io:/usr/share/gnome-shell/extensions/lab@koje.io:ro" \
      --shm-size 1g "$image" lab-session >/dev/null
    readlink "/proc/$(podman inspect --format '{{.State.Pid}}' "$lab")/ns/mnt" > "$lab_ns_file"
    for _ in $(seq 30); do [[ -S "$run/wayland-0" ]] && break; sleep 1; done
    [[ -S "$run/wayland-0" ]] || { podman logs "$lab" | tail -20; exit 1; }
    echo "GNOME Shell runs; Wayland socket for contexts: $run/wayland-0"
    ;;
  stop)
    kill_lab
    for c in $(podman ps -a --filter label=io.koje.spike=true --format '{{.Names}}'); do
      podman rm -f -t 2 "$c" >/dev/null || true
    done
    ;;
  eval)
    in_lab gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell \
      -m org.gnome.Shell.Eval "${2:?usage: lab.sh eval '<js>'}"
    ;;
  run)
    shift
    IN_LAB_OPT=-d in_lab "$@" >/dev/null
    ;;
  exec)
    shift
    in_lab "$@"
    ;;
  context)
    name=${2:?usage: lab.sh context <name> [image]}
    KOJE_SPIKE_IMAGE=${3:-${KOJE_SPIKE_IMAGE:-}} \
      "$here/../run-context.sh" "$name" "$run/wayland-0"
    ;;
  freeze|thaw)
    name=${2:?usage: lab.sh $1 <name>}
    sig=STOP; [[ $1 == thaw ]] && sig=CONT
    # Every process in the context's PID namespace but its init (catatonit),
    # so the container stays manageable. Selected by namespace: nested
    # containers here see the dev container's whole /proc.
    init=$(podman inspect --format '{{.State.Pid}}' "koje-spike-$name")
    ns=$(readlink "/proc/$init/ns/pid")
    n=0
    for p in /proc/[0-9]*; do
      p=${p#/proc/}
      [[ $p != "$init" && $(readlink "/proc/$p/ns/pid" 2>/dev/null) == "$ns" ]] || continue
      kill -"$sig" "$p" 2>/dev/null && n=$((n + 1))
    done
    echo "$1: $n processes in koje-spike-$name"
    ;;
  *)
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
