#!/usr/bin/env bash
# Build a Waybar .deb from upstream git for this Debian/Ubuntu system and
# install it. koje's bar needs Waybar after 0.15.0: its task list (the current
# workspace's windows, hyprland/workspaces in taskbar mode) takes clicks only
# since master 2026-05-21, and no release has that yet.
#
#   scripts/build-waybar-deb.sh               build master, install the .deb
#   scripts/build-waybar-deb.sh --ref <ref>   build a tag, branch or commit
#   scripts/build-waybar-deb.sh --no-install  only build; prints the .deb
#   scripts/build-waybar-deb.sh --yes         do not ask
#
# It reuses the distribution's packaging of Waybar (`apt source waybar`, its
# debian/ directory without the distribution's patches) and builds the package
# as version <distribution's upstream version>+git<date>.<commit>, which is
# newer than the distribution's, so its updates do not replace it.
# sudo is used for apt only: build dependencies, the source repositories (asked
# first) and installing the package. Work directory:
# ${XDG_CACHE_HOME:-~/.cache}/koje/waybar-deb (rebuilt from scratch every run).
set -euo pipefail

ref=master yes=0 install=1
while (($#)); do
  case "$1" in
    --ref) ref=${2:?--ref needs a value}; shift ;;
    --no-install) install=0 ;;
    --yes | -y) yes=1 ;;
    -h | --help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

repo=https://github.com/Alexays/Waybar
api=https://api.github.com/repos/Alexays/Waybar
work=${XDG_CACHE_HOME:-$HOME/.cache}/koje/waybar-deb

ask() {
  ((yes)) && return 0
  local answer
  read -r -p "$1 [y/N] " answer || return 1
  [[ $answer == [yY]* ]]
}
step() { printf '\n== %s\n' "$*"; }

command -v apt-get >/dev/null && command -v dpkg >/dev/null || {
  echo "this script is for Debian and Ubuntu (apt and dpkg)" >&2
  exit 1
}
. /etc/os-release
codename=${VERSION_CODENAME:-unstable}

step "Source repositories"
if [[ -z $(apt-cache showsrc waybar 2>/dev/null) ]]; then
  sources=/etc/apt/sources.list.d/${ID}.sources
  if [[ ! -f $sources ]]; then
    echo "apt has no source packages (deb-src) – enable them for your distribution and run again" >&2
    exit 1
  fi
  echo "apt has no source packages; the build needs Waybar's (its packaging)."
  echo "This adds deb-src to $sources (Types: deb → Types: deb deb-src)."
  ask "Enable source repositories?" || { echo "aborted" >&2; exit 1; }
  sudo sed -i 's/^Types: deb$/Types: deb deb-src/' "$sources"
  sudo apt-get update
else
  echo "enabled"
fi

step "Build dependencies (sudo apt-get build-dep waybar)"
sudo apt-get install -y --no-install-recommends curl jq ca-certificates dpkg-dev build-essential
sudo apt-get build-dep -y waybar

step "Sources in $work"
rm -rf "$work"
mkdir -p "$work"
cd "$work"
apt-get source --download-only waybar >/dev/null
# apt checked the download against the signed archive index; the uploader's
# own signature on the .dsc cannot be checked without their key.
dpkg-source --no-check -x --skip-patches ./*.dsc distro >/dev/null
upstream=$(dpkg-parsechangelog -l distro/debian/changelog -S Version | sed 's/-[^-]*$//; s/^[0-9]*://')
echo "packaging: Waybar $upstream from $ID $codename"
# The commit $ref names, then that commit as an archive – plain curl: git's
# HTTPS helper hung at 100 % CPU on Ubuntu 26.04.
info=$(curl -fsSL "$api/commits/$ref")
sha=$(jq -r .sha <<<"$info")
commit=${sha:0:10}
echo "source: $ref ($commit, $(jq -r .commit.committer.date <<<"$info" | cut -c1-10))"
curl -fL --progress-bar "$repo/archive/$sha.tar.gz" -o waybar.tar.gz
mkdir src
tar -xzf waybar.tar.gz -C src --strip-components=1
cd src

step "Packaging"
cp -r ../distro/debian .
# The distribution's patches are for its release; a native package needs no
# original tarball.
rm -rf debian/patches
echo "3.0 (native)" >debian/source/format
version="${upstream}+git$(date +%Y%m%d).${commit}"
name=$(git config user.name 2>/dev/null || true)
email=$(git config user.email 2>/dev/null || true)
{
  printf 'waybar (%s) %s; urgency=medium\n\n' "$version" "$codename"
  printf '  * Snapshot of %s %s (%s), built by koje'"'"'s scripts/build-waybar-deb.sh.\n\n' "$repo" "$ref" "$commit"
  printf ' -- %s <%s>  %s\n\n' "${name:-$USER}" "${email:-$USER@localhost}" "$(date -R)"
  cat debian/changelog
} >debian/changelog.new
mv debian/changelog.new debian/changelog
echo "version: $version"

# Waybar turns on bar blur for wayland-protocols >= 1.41, but its protocol file
# came later (missing in 1.44): without the file, build without blur (koje's
# bar does not use it).
protocols=$(pkg-config --variable=pkgdatadir wayland-protocols)
if [[ ! -f $protocols/staging/ext-background-effect/ext-background-effect-v1.xml ]]; then
  echo "no ext-background-effect protocol in wayland-protocols $(pkg-config --modversion wayland-protocols): building without bar blur"
  sed -i "s/version_compare('>=1.41')/version_compare('>=9999')/" meson.build protocol/meson.build
fi

step "Build (dpkg-buildpackage)"
DEB_BUILD_OPTIONS="parallel=$(nproc) nocheck" dpkg-buildpackage -us -uc -b
deb=$(ls "$work"/waybar_"${version}"_*.deb)
echo "built: $deb"

if ((install)); then
  step "Install (sudo apt install)"
  sudo apt-get install -y --allow-downgrades "$deb"
  if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] && pgrep -x waybar >/dev/null; then
    pkill -x waybar || true
    hyprctl dispatch exec waybar >/dev/null
    echo "restarted Waybar"
  fi
  echo
  echo "installed $version (check: apt policy waybar). Then run koje's ./install.sh"
  echo "for koje's bar config. Back to the distribution's: sudo apt install --allow-downgrades waybar=<version>"
else
  echo
  echo "install with: sudo apt install $deb"
fi
