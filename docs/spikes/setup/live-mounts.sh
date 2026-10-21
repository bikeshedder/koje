#!/usr/bin/env bash
# M2.12 spike: can a host folder be mounted into a running rootless container?
#
#   docs/spikes/setup/live-mounts.sh
#
# Idea: in rootless Podman's own user+mount namespace (`podman unshare`) a
# per-context directory is made a shared mount point *before* the container
# starts; the container mounts it with `rslave`. Folders bind-mounted into that
# directory later (again inside Podman's namespace, no root) should appear in
# the running container.
#
# Touches only its own test container `koje-spike-mounts` (label
# io.koje.test=true) and its own folders; cleans up on exit. Uses koje's base
# image with koje's user-namespace flag (--userns=keep-id).
set -u

NAME=koje-spike-mounts
IMAGE=localhost/koje/base:latest
LIVE=${XDG_RUNTIME_DIR:?}/koje-spike-mounts/live
SRC=$HOME/koje-spike-src
IN=/run/koje-live

step() { printf '\n== %s\n' "$*"; }
check() { # check <label> <command…>: runs it, prints its output and ok/FAIL
  local label=$1; shift
  local out
  if out=$("$@" 2>&1); then
    printf -- '-> ok    %s\n' "$label"
  else
    printf -- '-> FAIL  %s\n' "$label"
  fi
  [[ -n "$out" ]] && printf '%s\n' "$out" | sed 's/^/     /'
}
in_ns() { podman unshare sh -c "$1"; }
in_ctr() { podman exec "$NAME" sh -c "$1"; }

cleanup() {
  podman rm -f "$NAME" >/dev/null 2>&1
  podman volume rm "$NAME-home" >/dev/null 2>&1
  # The shared dir and the staging dir: nothing of the spike stays mounted
  # in Podman's namespace. Lazily, in case something still uses them.
  in_ns "umount -R -l '$LIVE' 2>/dev/null; umount -R -l '${LIVE%/live}/stage/b' 2>/dev/null; true"
  rm -rf "$SRC" "${LIVE%/live}"
}
trap cleanup EXIT

step "host"
podman --version
uname -r
podman image exists "$IMAGE" || { echo "no $IMAGE – run: koje image build"; exit 1; }

step "1. folders"
mkdir -p "$LIVE" "$SRC/a" "$SRC/b"
echo "hello a" >"$SRC/a/hello.txt"
echo "hello b" >"$SRC/b/hello.txt"
ls -ld "$LIVE" "$SRC/a" "$SRC/b"

step "2. shared mount point in Podman's namespace"
check "bind + make-rshared" in_ns "mount --bind '$LIVE' '$LIVE' && mount --make-rshared '$LIVE'"
check "a new podman unshare still sees it (persistent)" in_ns "findmnt -no TARGET,PROPAGATION '$LIVE'"

step "3. container with the directory as rslave (koje's flags)"
# Round 2: a home volume like a real context's (step 8).
check "podman run" podman run -d --name "$NAME" --label io.koje.test=true \
  --userns=keep-id --security-opt label=disable \
  --volume "$NAME-home:$HOME:U" \
  --volume "$LIVE:$IN:rslave" "$IMAGE" sleep infinity
check "propagation inside" in_ctr "findmnt -no TARGET,PROPAGATION $IN"

step "4. mount folder a while it runs"
check "mount --bind a" in_ns "mkdir -p '$LIVE/a' && mount --bind '$SRC/a' '$LIVE/a'"
check "container sees a/hello.txt" in_ctr "cat $IN/a/hello.txt"
check "container writes a/back.txt" in_ctr "echo from-container > $IN/a/back.txt"
check "host sees it, owned by the user" ls -ln "$SRC/a/back.txt"

step "5. second folder b, read-only (round 2: ro in a private staging dir, then bound)"
# Round 1: `mount --bind` then `remount,ro` left the container's copy writable
# (flags do not propagate). A bind copies its source's flags, so make the ro
# mount where nothing propagates, then bind that into the shared directory.
STAGE=${LIVE%/live}/stage
check "ro bind in staging, then into the shared dir" in_ns "mkdir -p '$STAGE/b' '$LIVE/b' && mount --bind '$SRC/b' '$STAGE/b' && mount --make-private '$STAGE/b' && mount -o remount,bind,ro '$STAGE/b' && mount --bind '$STAGE/b' '$LIVE/b'"
check "container sees b/hello.txt" in_ctr "cat $IN/b/hello.txt"
if in_ctr "echo x > $IN/b/nope.txt" 2>/dev/null; then
  echo "-> FAIL  b is writable in the container"
else
  echo "-> ok    b is read-only in the container"
fi

step "6. unmount a while a process in the container uses it"
podman exec -d "$NAME" sh -c "cd $IN/a && exec sleep 600"
sleep 1
check "umount a (Podman's namespace)" in_ns "umount '$LIVE/a'"
check "container after umount: ls a" in_ctr "ls -A $IN/a; findmnt -no TARGET $IN/a || echo '(not a mount point any more)'"
check "busy process still runs" in_ctr "pgrep -fa 'sleep 600'"
check "lazy umount if still mounted inside" in_ns "umount -l '$LIVE/a' 2>&1 || true"
check "container after lazy umount: ls a" in_ctr "ls -A $IN/a; findmnt -no TARGET $IN/a || echo '(not a mount point any more)'"

step "7. restart the container: b still there?"
check "podman restart" podman restart -t 1 "$NAME"
check "b after restart" in_ctr "cat $IN/b/hello.txt"

step "8. own path inside the container (home is a volume, as in a context)"
# The folder appears below $IN only. A symlink at its own path in the
# context's home shows it at its usual path:
check "symlink at the folder's own path" in_ctr "mkdir -p '$(dirname "$SRC")' && ln -sfn $IN/b '$SRC' && cat '$SRC/hello.txt'"
check "what programs see: pwd vs pwd -P" in_ctr "cd '$SRC' && pwd && pwd -P"

step "9. host side"
check "host does not see Podman-namespace mounts in $LIVE/b (expected: empty)" ls -A "$LIVE/b"

echo
echo "done – cleaning up"
