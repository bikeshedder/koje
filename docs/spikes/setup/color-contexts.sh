#!/usr/bin/env bash
# Color every window of a spike context in the nested Hyprland – kojed's job
# later (pid → cgroup → container, tag rule per context: ADR 0018).
# Run on the host after opening windows; run again for new windows.
#
#   color-contexts.sh            koje-spike-a red, koje-spike-b blue, others green
set -euo pipefail
ipc="$(dirname "$0")/hypr-ipc/target/release/hypr-ipc"

declare -A colors=(
  [koje-spike-a]="rgb(e53935) rgb(7f1d1b)"
  [koje-spike-b]="rgb(1e88e5) rgb(0d3c66)"
)
for name in "${!colors[@]}"; do
  "$ipc" raw keyword windowrule "match:tag $name, border_color ${colors[$name]}" >/dev/null 2>&1
done

"$ipc" clients 2>/dev/null | awk '$2 == "pid" { print $1, $3 }' | while read -r addr pid; do
  id=$(grep -o 'libpod-[0-9a-f]*' "/proc/$pid/cgroup" 2>/dev/null | head -1 | cut -d- -f2 || true)
  [[ -n "$id" ]] || continue
  name=$(podman inspect -f '{{.Name}}' "$id" 2>/dev/null || true)
  [[ -n "$name" ]] || continue
  if [[ -z "${colors[$name]:-}" ]]; then
    "$ipc" raw keyword windowrule "match:tag $name, border_color rgb(2e7d5b) rgb(14402d)" >/dev/null 2>&1
  fi
  "$ipc" raw dispatch tagwindow "+$name" "address:$addr" >/dev/null 2>&1
  echo "$addr (pid $pid) → $name"
done
