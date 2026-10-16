#!/usr/bin/env bash
# M2.6: memory of processes whose command line matches a pattern, from
# /proc/<pid>/smaps_rollup (run on the HOST; container processes are visible
# there too). Prints per-process and total RSS, PSS and shared pages in MiB.
#
#   mem.sh firefox                 all Firefox processes
#   mem.sh firefox koje-spike-a    only those inside container koje-spike-a
set -euo pipefail
pattern=${1:?usage: mem.sh <pattern> [container]}
container=${2:-}

pids=$(pgrep -f -- "$pattern" || true)
if [[ -n "$container" ]]; then
  # Keep only PIDs in the container's cgroup.
  cg=$(podman inspect -f '{{.State.CgroupPath}}' "$container")
  pids=$(for p in $pids; do grep -q -- "$cg" "/proc/$p/cgroup" 2>/dev/null && echo "$p"; done)
fi

printf '%8s %10s %10s %12s  %s\n' PID RSS_MiB PSS_MiB SHARED_MiB CMD
for p in $pids; do
  [[ -r /proc/$p/smaps_rollup ]] || continue
  awk -v pid="$p" -v cmd="$(tr '\0' ' ' < /proc/$p/cmdline | cut -c1-60)" '
    /^Rss:/ {rss=$2} /^Pss:/ {pss=$2}
    /^Shared_Clean:/ {sh+=$2} /^Shared_Dirty:/ {sh+=$2}
    END {printf "%8s %10.1f %10.1f %12.1f  %s\n", pid, rss/1024, pss/1024, sh/1024, cmd}
  ' "/proc/$p/smaps_rollup"
done | tee /dev/stderr | awk '{r+=$2; p+=$3; s+=$4} END {printf "%8s %10.1f %10.1f %12.1f\n", "TOTAL", r, p, s}'
