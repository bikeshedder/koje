#!/usr/bin/env bash
# M2.6: memory.current / memory.stat of a spike context's cgroup (run on the HOST).
# Use it before and after starting a nested container inside the context to see
# whether nested containers are counted.
#
#   cgroup-mem.sh koje-spike-a
set -euo pipefail
c=${1:?usage: cgroup-mem.sh <container>}
path=/sys/fs/cgroup$(podman inspect -f '{{.State.CgroupPath}}' "$c")
echo "cgroup: $path"
awk '{printf "memory.current: %.1f MiB\n", $1/1048576}' "$path/memory.current"
grep -E '^(anon|file|shmem|kernel) ' "$path/memory.stat" | awk '{printf "  %-8s %.1f MiB\n", $1, $2/1048576}'
echo "sub-cgroups:"
find "$path" -mindepth 2 -maxdepth 4 -name memory.current -printf '%h\n' | while read -r d; do
  awk -v d="${d#"$path"}" '{printf "  %-40s %.1f MiB\n", d, $1/1048576}' "$d/memory.current"
done
