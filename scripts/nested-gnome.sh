#!/usr/bin/env bash
# A nested GNOME Shell in a window, with its own koje (branch gnome, ADR 0059):
# the installed extension loaded fresh, a second kojed on the nested session's
# bus, and a test context in it. For trying new extension code without logging
# out. Run on the host from your GNOME session:
#
#   scripts/nested-gnome.sh [context]     # default context: nested-test
#
# The nested koje has its own config and state (in $XDG_RUNTIME_DIR/koje-nested,
# gone at the next boot); states still go to ~/.local/share/koje/<context>
# (Podman's storage shares XDG_DATA_HOME). Use a context of its own: two kojeds
# on one context would both write its recovery file and watch its limits.
# Closing the window stops the context and the nested kojed;
# `koje rm <context>` removes the test context for good.
set -euo pipefail

context=${1:-nested-test}
: "${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR is not set}"
[[ -x /usr/libexec/mutter-devkit ]] || {
  echo "the devkit is missing: sudo apt install mutter-dev-bin" >&2
  exit 1
}
for c in gnome-shell kojed koje podman dbus-run-session; do
  command -v "$c" >/dev/null || { echo "$c not found" >&2; exit 1; }
done

home=$XDG_RUNTIME_DIR/koje-nested
mkdir -p "$home/bin"
# Podman reaches systemd (cgroups of the containers) over the session bus:
# it keeps the desktop's, while kojed and koje talk on the nested one.
cat > "$home/bin/podman" <<EOF
#!/bin/sh
DBUS_SESSION_BUS_ADDRESS='${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}' exec $(command -v podman) "\$@"
EOF
chmod +x "$home/bin/podman"
export NESTED_CONTEXT=$context NESTED_HOME=$home
echo "Starting a nested GNOME Shell (its session's messages: $home/session.log)"

# The session's services log to its stderr: into a file. What the script
# itself says goes to the terminal (fd 3).
exec 3>&2
exec dbus-run-session -- bash -euo pipefail -c '
  say() { echo "$@" >&3; }
  log=$NESTED_HOME
  before=$(ls "$XDG_RUNTIME_DIR" | grep -E "^wayland-[0-9]+$" || true)
  gnome-shell --devkit --wayland > "$log/shell.log" 2>&1 &
  shell=$!
  # The nested Shell'\''s Wayland socket: the one that was not there before.
  socket=
  for _ in $(seq 60); do
    socket=$(ls "$XDG_RUNTIME_DIR" | grep -E "^wayland-[0-9]+$" | grep -vxF "$before" | head -1 || true)
    [[ -n "$socket" ]] && break
    kill -0 "$shell" 2>/dev/null || { say "GNOME Shell ended, see $log/shell.log"; exit 1; }
    sleep 0.5
  done
  [[ -n "$socket" ]] || { say "no Wayland socket from the nested Shell, see $log/shell.log"; exit 1; }

  export PATH=$NESTED_HOME/bin:$PATH
  export WAYLAND_DISPLAY=$socket KOJE_COMPOSITOR=gnome
  export XDG_CONFIG_HOME=$NESTED_HOME/config XDG_STATE_HOME=$NESTED_HOME/state
  unset HYPRLAND_INSTANCE_SIGNATURE
  kojed > "$log/kojed.log" 2>&1 &
  kojed=$!
  cleanup() {
    koje down "$NESTED_CONTEXT" --no-save >&3 2>&3 || true
    kill "$kojed" 2>/dev/null || true
  }
  trap cleanup EXIT
  # kojed on the bus before koje asks it.
  for _ in $(seq 20); do
    gdbus introspect --session -d io.koje.Daemon -o /io/koje/Daemon >/dev/null 2>&1 && break
    sleep 0.5
  done

  {
    koje list | grep -q "^$NESTED_CONTEXT\b" || koje new "$NESTED_CONTEXT" --color "#e01b24"
    koje up "$NESTED_CONTEXT"
    koje run "$NESTED_CONTEXT" foot || true
  } >&3 2>&3
  say
  say "Nested GNOME Shell on $socket with context $NESTED_CONTEXT."
  say "Logs: $log/shell.log, $log/kojed.log. Close its window to end it."
  wait "$shell" || true
' 2>"$home/session.log"
