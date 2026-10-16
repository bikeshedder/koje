#!/usr/bin/env bash
# Remove a spike context container and its home volume. Only touches
# containers/volumes named koje-spike-<name> that carry io.koje.spike=true.
set -euo pipefail
name=${1:?usage: rm-context.sh <name>}
c="koje-spike-$name"
if [[ "$(podman inspect -f '{{index .Config.Labels "io.koje.spike"}}' "$c" 2>/dev/null)" == "true" ]]; then
  podman rm -f "$c"
fi
podman volume rm "$c-home" 2>/dev/null || true
