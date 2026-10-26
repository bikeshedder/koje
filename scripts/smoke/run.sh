#!/usr/bin/env bash
# Smoke test for koje-shell in the dev container: a headless sway as the
# compositor, fake Hyprland and kojed (fixed desktop: contexts kundenportal,
# privat, alt), the shell from target/debug. Runs STEPS, then stops it all.
#
#   scripts/smoke/run.sh [--dark] <out dir> '<steps>'
#
# Steps are bash with these helpers:
#   shot NAME         screenshot to <out dir>/NAME.png
#   overview          koje-shell overview (show or hide)
#   hover X Y         move the pointer to X,Y (logical pixels)
#   click X Y         left click at X,Y
#   rclick X Y        right click at X,Y
#   key KEY           press a key: Escape, Return, BackSpace, Tab, arrows
#   type TEXT         type letters, digits, space
#   pause SECONDS
# Afterwards <out dir> holds hypr.log (Hyprland requests), kojed.log (kojed
# calls), koje.log (CLI calls), shell.log. Example:
#   scripts/smoke/run.sh /tmp/x 'overview; pause 1; shot overview'
set -euo pipefail

dark=0
if [[ ${1:-} == --dark ]]; then dark=1; shift; fi
out=${1:?out dir}; steps=${2:?steps}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
mkdir -p "$out"; out=$(cd "$out" && pwd)
rm -f "$out"/*.png "$out"/*.log

run=$(mktemp -d); trap 'rm -rf "$run"' EXIT
export XDG_RUNTIME_DIR=$run WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export XDG_CONFIG_HOME=$run/config XDG_DATA_HOME=$run/data XDG_STATE_HOME=$run/state
sway -c "$here/sway.conf" >"$out/sway.log" 2>&1 &
sway=$!
for _ in $(seq 50); do [[ -S $run/wayland-1 ]] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=fake RUST_LOG=${RUST_LOG:-koje_shell=debug}
export SWAYSOCK=$(ls "$run"/sway-ipc.*.sock)
python3 -I "$here/fake_hypr.py" "$run/hypr/fake" "$out/hypr.log" &
hypr=$!
python3 -I "$here/input.py" "$run/input" 1920 1080 2>"$out/input.log" &
input=$!
((dark)) && export ADW_DEBUG_COLOR_SCHEME=prefer-dark
# GNOME Shell's favorites for the dash: its schema with just that key, in a
# keyfile backend below the temporary config.
mkdir -p "$run/schemas"
cat >"$run/schemas/org.gnome.shell.gschema.xml" <<'XML'
<schemalist><schema id="org.gnome.shell" path="/org/gnome/shell/">
<key name="favorite-apps" type="as"><default>['firefox.desktop', 'org.gnome.Nautilus.desktop', 'org.gnome.Calculator.desktop']</default></key>
</schema></schemalist>
XML
glib-compile-schemas "$run/schemas"
export GSETTINGS_SCHEMA_DIR=$run/schemas GSETTINGS_BACKEND=keyfile
# A fake `koje` (bin/koje): the shell runs it for `apps`, `up`, …
export PATH=$here/bin:$PATH KOJE_LOG=$out/koje.log
export SHELL_BIN=$root/target/debug/koje-shell OUT=$out LOG=$out/kojed.log INPUT=$run/input

dbus-run-session -- bash -c '
  python3 -I "'"$here"'/fake_kojed.py" &
  sleep 0.5
  "$SHELL_BIN" >"$OUT/shell.log" 2>&1 &
  shell=$!
  sleep 2
  shot() { grim "$OUT/$1.png"; }
  overview() { "$SHELL_BIN" overview; sleep 0.5; }
  hover() { echo "move $1 $2" >"$INPUT"; sleep 0.3; }
  click() { echo "move $1 $2" >"$INPUT"; sleep 0.2; echo click >"$INPUT"; sleep 0.5; }
  rclick() { echo "move $1 $2" >"$INPUT"; sleep 0.2; echo rclick >"$INPUT"; sleep 0.5; }
  key() { echo "key $1" >"$INPUT"; sleep 0.3; }
  type() { echo "type $1" >"$INPUT"; sleep 0.3; }
  pause() { sleep "$1"; }
  '"$steps"'
  kill $shell; wait $shell 2>/dev/null || true
'
kill "$hypr" "$input" "$sway" 2>/dev/null || true
wait "$sway" 2>/dev/null || true
