#!/usr/bin/env bash
# The koje package's version (ADR 0045): the workspace version plus the
# commit, newer with every commit – 0.1.0+git20261026.1412.dc57ea6, with
# `.dirty` for uncommitted changes. The time is the commit's, in UTC.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
stamp=$(TZ=UTC git log -1 --format=%cd --date=format-local:%Y%m%d.%H%M)
commit=$(git rev-parse --short=7 HEAD)
dirty=$(git diff --quiet HEAD -- && echo "" || echo ".dirty")
echo "$version+git$stamp.$commit$dirty"
