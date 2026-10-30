#!/bin/sh
# Entry point of the GNOME lab container: a stand-in system bus (GNOME Shell
# needs one), the session bus with PipeWire (the devkit window gets the
# screen through it), then GNOME Shell with its devkit window on the dev
# container's Wayland (the socket VS Code forwards) – or, with LAB_HEADLESS,
# headless on a virtual monitor.
set -eu
export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/system_bus_socket"
export GTK_A11Y=none
# As in a GNOME session on the host: GNOME's defaults for Mutter
# (dynamic workspaces, workspaces on the primary monitor only) apply only then.
export XDG_CURRENT_DESKTOP=GNOME
dbus-daemon --session --fork --address="$DBUS_SYSTEM_BUS_ADDRESS"
# The session bus in the runtime directory, which the dev container shares:
# kojed and the koje-gnome tests reach the extension from there.
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
dbus-daemon --session --fork --address="$DBUS_SESSION_BUS_ADDRESS"
exec sh -c '
  set -eu
  wait_for() { for _ in $(seq 50); do eval "$1" && return 0; sleep 0.2; done; echo "lab: timed out: $1" >&2; exit 1; }
  # WirePlumber started together with PipeWire spins at full CPU and never
  # offers nodes; start it once PipeWire listens.
  pipewire &
  wait_for "[ -S \$XDG_RUNTIME_DIR/pipewire-0 ]"
  wireplumber &
  wait_for "timeout 2 pw-cli ls Node 2>/dev/null | grep -q Dummy-Driver"
  # lab.sh finds the session bus here (the dev container shares its /proc
  # with nested containers, so looking for the process is ambiguous).
  echo "$DBUS_SESSION_BUS_ADDRESS" > "$XDG_RUNTIME_DIR/lab-session-bus"
  gsettings set org.gnome.shell enabled-extensions "[\"lab@koje.io\", \"koje@koje.io\"]"
  gsettings set org.gnome.shell welcome-dialog-last-shown-version "999"
  if [ -n "${LAB_HEADLESS:-}" ]; then
    exec gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 "$@"
  fi
  exec gnome-shell --devkit --wayland --no-x11 "$@"' lab-session "$@"
