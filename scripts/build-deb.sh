#!/usr/bin/env bash
# Build the koje package (ADR 0045) in an Ubuntu 26.04 container
# (build/Containerfile), on the host or in the dev container alike – only
# Podman and git are needed: the binaries, the musl forwarder, then cargo-deb.
# Writes target/debian/koje_<version>_amd64.deb. `just deb` runs it.
#
# Its own target directory (target/ubuntu): artifacts built against the dev
# container's Debian libraries never mix with these. Downloaded crates stay in
# the Podman volume koje-deb-cargo.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."

version=$(scripts/deb-version.sh)
image=koje-deb-build
# The image's version: the Containerfile and the toolchain it installs.
tag=$(cat build/Containerfile rust-toolchain.toml | sha256sum | cut -c1-12)
if ! podman image exists "$image:$tag"; then
  echo "building the image $image:$tag (once)"
  context=$(mktemp -d)
  trap 'rm -rf "$context"' EXIT
  cp build/Containerfile rust-toolchain.toml "$context/"
  podman build -t "$image:$tag" -f "$context/Containerfile" "$context"
fi

podman run --rm \
  -v "$PWD:/src" -w /src \
  -v koje-deb-cargo:/root/.cargo/registry \
  -e VERSION="$version" \
  -e RUSTUP_TOOLCHAIN="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)" \
  "$image:$tag" bash -euo pipefail -c '
    export CARGO_TARGET_DIR=/src/target/ubuntu
    mkdir -p target/deb-staging target/debian
    cargo build --release --locked -p koje -p kojed
    cargo build --release --locked -p koje-forward --target x86_64-unknown-linux-musl
    cp "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/koje-forward" target/deb-staging/
    cargo deb -p koje --no-build --deb-version "$VERSION" -o target/debian/
  '
ls -t target/debian/koje_*_amd64.deb | head -1
