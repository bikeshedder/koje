#!/usr/bin/env bash
# Title bars for Hyprland (ADR 0039): build and enable the hyprbars plugin with
# hyprpm, then source koje's hyprbars snippet from hyprland.conf. Run on the
# host, inside the Hyprland session, after ./install.sh.
#
#   scripts/setup-hyprbars.sh          set up (asks before installing packages)
#   scripts/setup-hyprbars.sh --yes    do not ask
#
# hyprpm builds plugins against the running Hyprland's exact version: after a
# Hyprland update run `hyprpm update` (this script does the same). sudo is used
# for apt and by hyprpm itself (it installs Hyprland's headers).
set -euo pipefail

yes=0
case "${1:-}" in
  --yes | -y) yes=1 ;;
  -h | --help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  "") ;;
  *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
esac

snippet=${XDG_DATA_HOME:-$HOME/.local/share}/koje/hyprland/hyprbars.conf
conf=${XDG_CONFIG_HOME:-$HOME/.config}/hypr/hyprland.conf
step() { printf '\n== %s\n' "$*"; }
ask() {
  ((yes)) && return 0
  local answer
  read -r -p "$1 [y/N] " answer || return 1
  [[ $answer == [yY]* ]]
}

[[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] || {
  echo "run this inside the Hyprland session (hyprpm needs the running Hyprland)" >&2
  exit 1
}
command -v hyprpm >/dev/null || {
  echo "hyprpm is missing – it comes with Hyprland (Ubuntu: package hyprland)" >&2
  exit 1
}
[[ -f $snippet ]] || {
  echo "$snippet is missing – run koje's ./install.sh first" >&2
  exit 1
}

step "Build dependencies (Hyprland's headers and tools, sudo apt)"
echo "cpio cmake git meson g++ pkg-config and Hyprland's build dependencies (apt build-dep hyprland)"
if ask "Install them?"; then
  sudo apt-get install -y cpio cmake git meson g++ pkg-config
  sudo apt-get build-dep -y hyprland || {
    echo "apt build-dep failed – source repositories may be off (see scripts/build-waybar-deb.sh)" >&2
    exit 1
  }
fi

step "Hyprland headers for $(hyprctl version -j | jq -r .tag) (hyprpm update; asks for sudo)"
hyprpm update

step "hyprland-plugins (hyprpm add)"
if ! hyprpm list | grep -q hyprbars; then
  hyprpm add https://github.com/hyprwm/hyprland-plugins
fi
hyprpm enable hyprbars

step "Load it"
line="source = $snippet"
if ! grep -qxF "$line" "$conf" 2>/dev/null && ! grep -qxF "source = ${snippet/#$HOME/\~}" "$conf" 2>/dev/null; then
  printf '\n# added by koje scripts/setup-hyprbars.sh\n%s\n' "$line" >>"$conf"
  echo "added to $conf: $line"
fi
hyprpm reload -n
hyprctl reload >/dev/null
echo
echo "title bars on. After a Hyprland update: hyprpm update"
