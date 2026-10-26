#!/usr/bin/env bash
# Install the newest koje package from target/debian/ on the host and set up
# the current user (ADR 0045) – for testing after `just deb` in the dev
# container.
#
#   scripts/install-deb.sh          install, then `koje setup` (asks once)
#   scripts/install-deb.sh --yes    do not ask (apt and koje setup)
#
# Reinstalls when the version is the same (another build of the same commit).
# koje setup restarts kojed and koje-shell when they run an old binary and,
# in the koje session, reloads Hyprland. sudo is used for apt.
set -euo pipefail

yes=0
for arg in "$@"; do
  case "$arg" in
    --yes | -y) yes=1 ;;
    -h | --help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg (see --help)" >&2; exit 2 ;;
  esac
done

cd "$(dirname "$(readlink -f "$0")")/.."

deb=$(ls -t target/debian/koje_*_amd64.deb 2>/dev/null | head -1 || true)
if [[ -z $deb ]]; then
  echo "no package in target/debian/ – build one in the dev container: just deb" >&2
  exit 1
fi
version=$(dpkg-deb -f "$deb" Version)
echo "package: $deb"
checkout=$(scripts/deb-version.sh)
if [[ $version != "$checkout" ]]; then
  echo "warning: built from $version, the checkout is $checkout – run just deb again?" >&2
fi

apt_args=(install --reinstall)
((yes)) && apt_args+=(--yes)
sudo apt "${apt_args[@]}" "./$deb"

setup_args=()
((yes)) && setup_args+=(--yes)
echo
/usr/bin/koje setup "${setup_args[@]}"

# koje setup restarted the units; a koje-shell or kojed started by hand still
# runs the binary the package replaced.
for name in koje-shell kojed; do
  for pid in $(pgrep -x "$name" || true); do
    if [[ $(readlink "/proc/$pid/exe" 2>/dev/null) == *" (deleted)" ]]; then
      echo
      echo "note: $name (pid $pid) runs the old binary and is not koje's unit –"
      echo "      stop it and run: systemctl --user restart $name"
    fi
  done
done
