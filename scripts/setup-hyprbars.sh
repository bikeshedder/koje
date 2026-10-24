#!/usr/bin/env bash
# Title bars for Hyprland (ADR 0039): build the hyprbars plugin against the
# distribution's Hyprland headers (Debian/Ubuntu: hyprland-dev) and load it
# from hyprland.conf together with koje's hyprbars snippet. Run on the host,
# inside the Hyprland session, after ./install.sh.
#
#   scripts/setup-hyprbars.sh          build, install, load (asks before apt)
#   scripts/setup-hyprbars.sh --yes    do not ask
#
# A plugin only works with the Hyprland it was built for: after a Hyprland
# update run this script again and log in again. No hyprpm: Ubuntu's never
# records its headers, so `hyprpm add` always says "Headers outdated".
set -euo pipefail

yes=0
case "${1:-}" in
  --yes | -y) yes=1 ;;
  -h | --help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  "") ;;
  *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
esac

plugins=https://github.com/hyprwm/hyprland-plugins
snippet=${XDG_DATA_HOME:-$HOME/.local/share}/koje/hyprland/hyprbars.conf
lib=$HOME/.local/lib/koje
so=$lib/hyprbars.so
conf=${XDG_CONFIG_HOME:-$HOME/.config}/hypr/hyprland.conf
work=${XDG_CACHE_HOME:-$HOME/.cache}/koje/hyprbars
step() { printf '\n== %s\n' "$*"; }
ask() {
  ((yes)) && return 0
  local answer
  read -r -p "$1 [y/N] " answer || return 1
  [[ $answer == [yY]* ]]
}

[[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] || {
  echo "run this inside the Hyprland session (the plugin is built for the running Hyprland)" >&2
  exit 1
}
[[ -f $snippet ]] || {
  echo "$snippet is missing – run koje's ./install.sh first" >&2
  exit 1
}
running=$(hyprctl version -j)
installed=$(Hyprland --version-json)
commit=$(jq -r .commit <<<"$running")
if [[ $(jq -r .abiHash <<<"$running") != "$(jq -r .abiHash <<<"$installed")" ]]; then
  echo "the running Hyprland is not the installed one (updated since you logged in?)" >&2
  echo "log out and in again (or reboot), then run this script again" >&2
  exit 1
fi

step "Build dependencies (sudo apt)"
deps=(make g++ pkg-config curl jq hyprland-dev libpixman-1-dev libdrm-dev libpango1.0-dev
  libinput-dev libudev-dev libwayland-dev libxkbcommon-dev)
echo "${deps[*]}"
if ask "Install them?"; then
  sudo apt-get install -y --no-install-recommends "${deps[@]}"
fi
headers=$(pkg-config --modversion hyprland 2>/dev/null || echo none)
hl=$(jq -r .tag <<<"$running")
if [[ v$headers != "$hl" ]]; then
  echo "Hyprland's headers ($headers) do not match the running Hyprland ($hl)" >&2
  exit 1
fi

step "hyprland-plugins for Hyprland $hl"
# The repository pins the plugin commit for each Hyprland commit (hyprpm.toml).
pin=$(curl -fsSL "https://raw.githubusercontent.com/hyprwm/hyprland-plugins/main/hyprpm.toml" |
  grep -oE "\[\"$commit\", *\"[0-9a-f]{40}\"\]" | grep -oE '[0-9a-f]{40}"\]$' | tr -d '"]' || true)
if [[ -z $pin ]]; then
  echo "hyprland-plugins has no version for Hyprland $hl ($commit)" >&2
  exit 1
fi
echo "plugins commit $pin"
rm -rf "$work"
mkdir -p "$work"
curl -fL --progress-bar "$plugins/archive/$pin.tar.gz" -o "$work/plugins.tar.gz"
tar -xzf "$work/plugins.tar.gz" -C "$work" --strip-components=1

step "Build hyprbars"
make -C "$work/hyprbars" all
mkdir -p "$lib"
install -m 755 "$work/hyprbars/hyprbars.so" "$so.koje-new"
mv -f "$so.koje-new" "$so"
echo "installed $so"

step "Load it"
# The plugin first, then koje's settings for it.
if ! grep -qxF "plugin = $so" "$conf" 2>/dev/null; then
  printf '\n# added by koje scripts/setup-hyprbars.sh\nplugin = %s\nsource = %s\n' "$so" "$snippet" >>"$conf"
  echo "added to $conf: plugin = $so, source = $snippet"
fi
if hyprctl plugin list | grep -q hyprbars; then
  echo "hyprbars was loaded already: log out and in again to use the new build"
else
  hyprctl plugin load "$so"
  hyprctl reload >/dev/null
  echo "title bars on"
fi
echo
echo "after a Hyprland update: run this script again, then log in again"
