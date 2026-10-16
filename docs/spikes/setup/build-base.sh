#!/usr/bin/env bash
# Build the throwaway spike image `koje-spike/base` for the current user.
set -euo pipefail
cd "$(dirname "$0")/base"
podman build \
  --build-arg USER_NAME="$(id -un)" \
  --build-arg USER_UID="$(id -u)" \
  --build-arg USER_HOME="$HOME" \
  -t localhost/koje-spike/base:latest .
