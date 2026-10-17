#!/usr/bin/env bash
# M2.6: Rss/Pss of one shared library's mappings, summed over the processes of
# a container that match a pattern (run on the HOST).
#
#   lib-pss.sh firefox koje-spike-a libxul.so
#
# If another context maps the same physical pages (same image layer), the Pss of
# these mappings drops while that context runs: Pss splits a page among all
# processes that map it, system-wide.
set -euo pipefail
pattern=${1:?usage: lib-pss.sh <pattern> <container> <library>}
container=${2:?usage: lib-pss.sh <pattern> <container> <library>}
lib=${3:?usage: lib-pss.sh <pattern> <container> <library>}

cg=$(podman inspect -f '{{.State.CgroupPath}}' "$container")
for p in $(pgrep -f -- "$pattern" || true); do
  [[ "$(cat "/proc/$p/comm" 2>/dev/null)" != lib-pss.sh ]] || continue
  if grep -qF -- "$cg" "/proc/$p/cgroup" 2>/dev/null && [[ -r /proc/$p/smaps ]]; then
    cat "/proc/$p/smaps"
  fi
done | awk -v lib="$lib" '
  /^[0-9a-f]+-[0-9a-f]+ / { inlib = index($0, lib) > 0; next }
  inlib && /^Rss:/ { rss += $2 }
  inlib && /^Pss:/ { pss += $2 }
  END { printf "%s in %s processes: Rss %.1f MiB, Pss %.1f MiB\n", lib, "'"$container"'", rss/1024, pss/1024 }'
