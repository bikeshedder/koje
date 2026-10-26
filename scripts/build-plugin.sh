#!/usr/bin/env bash
# Build koje's Hyprland plugin (ADR 0051) in an Ubuntu 26.04 container against
# its hyprland-dev, into target/plugin/: koje-hyprland.so and
# hyprland-version (the Hyprland it was built for). Needs Podman (the dev
# container's nested one, M0.8). `just deb` runs it.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."

image=koje-plugin-build
# The image is built once; the Containerfile's hash names its version.
tag=$(sha256sum plugin/Containerfile | cut -c1-12)
if ! podman image exists "$image:$tag"; then
  podman build -q -t "$image:$tag" -f plugin/Containerfile plugin >/dev/null
fi
mkdir -p target/plugin
podman run --rm --network=none \
  -v "$PWD/plugin:/src:ro,Z" -v "$PWD/target/plugin:/out:Z" \
  "$image:$tag" bash -c '
    set -e
    cp -r /src /tmp/build && cd /tmp/build
    make -s OUT=/out/koje-hyprland.so
    dpkg-query -W -f "\${Version}\n" hyprland >/out/hyprland-version
  '
built=$(cat target/plugin/hyprland-version)
# The package depends on this upstream version (crates/koje/Cargo.toml):
# a newer Hyprland needs the plugin checked and both changed together.
pin=0.53.3
if [[ ${built%%+*} != "$pin" && ${built%%-*} != "$pin" ]]; then
  echo "error: Ubuntu's Hyprland is $built, the plugin and the package are for $pin" >&2
  exit 1
fi
echo "target/plugin/koje-hyprland.so for Hyprland $built"
