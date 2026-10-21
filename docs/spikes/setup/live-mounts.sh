#!/usr/bin/env bash
# M2.12 spike, round 3: the context's home as the propagation point.
#
#   docs/spikes/setup/live-mounts.sh
#
# The home volume's directory on the host is made a shared mount point inside
# rootless Podman's namespace (`podman unshare`) and mounted at $HOME in the
# container with `rslave` – instead of the named volume. A folder bound into
# that directory later (again inside Podman's namespace, no root) should appear
# in the running container at its own path below $HOME, as a real mount.
#
# (Rounds 1 and 2 used a separate directory plus symlinks; see the report.)
#
# Touches only its own test container `koje-spike-mounts` (label
# io.koje.test=true), its volume `koje-spike-mounts-home` and its own folders;
# cleans up on exit. koje's base image, koje's flags (--userns=keep-id).
set -u

NAME=koje-spike-mounts
VOLUME=$NAME-home
IMAGE=localhost/koje/base:latest
SRC=$HOME/koje-spike-src
REL=work/project # where the folder appears below $HOME in the container

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

DATA=""
cleanup() {
  podman rm -f "$NAME" >/dev/null 2>&1
  [[ -n "$DATA" ]] && in_ns "umount -R -l '$DATA' 2>/dev/null; true"
  podman volume rm "$VOLUME" >/dev/null 2>&1
  rm -rf "$SRC"
}
trap cleanup EXIT

step "host"
podman --version
uname -r
podman image exists "$IMAGE" || { echo "no $IMAGE – run: koje image build"; exit 1; }

step "1. folders and the home volume"
mkdir -p "$SRC"
echo "hello project" >"$SRC/hello.txt"
check "volume create" podman volume create "$VOLUME"
DATA=$(podman volume inspect --format '{{.Mountpoint}}' "$VOLUME")
echo "volume directory: $DATA"
check "owner of the volume directory on the host" ls -ldn "$DATA"

step "2. the volume directory as a shared mount point in Podman's namespace"
check "bind + make-rshared" in_ns "mount --bind '$DATA' '$DATA' && mount --make-rshared '$DATA'"
check "still there in a new podman unshare" in_ns "findmnt -no TARGET,PROPAGATION '$DATA'"

step "3. container with it at \$HOME (rslave), koje's flags"
check "podman run" podman run -d --name "$NAME" --label io.koje.test=true \
  --userns=keep-id --security-opt label=disable \
  --env HOME="$HOME" --workdir "$HOME" \
  --volume "$DATA:$HOME:rslave" "$IMAGE" sleep infinity
check "propagation of \$HOME inside" in_ctr "findmnt -no TARGET,PROPAGATION '$HOME'"
check "home is the user's and writable" in_ctr "id -u; stat -c '%u %a' '$HOME'; touch '$HOME/.probe' && echo writable"

step "4. mount the folder while it runs, at its own path below \$HOME"
check "mount --bind into the volume directory" in_ns "mkdir -p '$DATA/$REL' && mount --bind '$SRC' '$DATA/$REL'"
check "container sees it at ~/$REL" in_ctr "cat '$HOME/$REL/hello.txt'"
check "a real mount there (no symlink): pwd -P, findmnt" in_ctr "cd '$HOME/$REL' && pwd -P && findmnt -no TARGET '$HOME/$REL'"
check "container writes back" in_ctr "echo from-container > '$HOME/$REL/back.txt'"
check "host sees it, owned by the user" ls -ln "$SRC/back.txt"
check "the rest of the home is untouched" in_ctr "ls -A '$HOME'"

step "5. unmount while a process uses it (lazy)"
podman exec -d "$NAME" sh -c "cd '$HOME/$REL' && exec sleep 600"
sleep 1
check "umount -l" in_ns "umount -l '$DATA/$REL' && rmdir '$DATA/$REL'"
check "container after umount: gone" in_ctr "ls -A '$HOME/work' ; findmnt -no TARGET '$HOME/$REL' || echo '(not mounted)'"
check "busy process still runs" in_ctr "pgrep -fa 'sleep 600'"

step "6. mount again, restart the container: still there?"
check "mount again" in_ns "mkdir -p '$DATA/$REL' && mount --bind '$SRC' '$DATA/$REL'"
check "podman restart" podman restart -t 1 "$NAME"
check "after restart" in_ctr "cat '$HOME/$REL/hello.txt'"

step "7. host side"
check "host's own view of the volume directory: the folder is not mounted there (expected: empty)" ls -A "$DATA/$REL"

echo
echo "done – cleaning up"
