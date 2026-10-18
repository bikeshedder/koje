#!/usr/bin/env bash
# koje installer (ADR 0021). Run from a checkout on the host:
#
#   ./install.sh             show the plan, ask once, install
#   ./install.sh --dry-run   only show the plan
#   ./install.sh --yes       do not ask
#   ./install.sh --skip-build   do not build/install koje and kojed
#
# It never runs sudo: missing system packages are listed with an install command.
# It writes only below ~/.local (binaries, icon font, koje's Hyprland snippet),
# adds one `source` line to ~/.config/hypr/hyprland.conf if missing, and installs
# koje's Waybar config only if you have none. Everything is shown before it happens.
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"
repo=$PWD

dry=0 yes=0 build=1
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry=1 ;;
    --yes | -y) yes=1 ;;
    --skip-build) build=0 ;;
    -h | --help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg (see --help)" >&2; exit 2 ;;
  esac
done

data=${XDG_DATA_HOME:-$HOME/.local/share}
config=${XDG_CONFIG_HOME:-$HOME/.config}
bin=$HOME/.local/bin
snippet=$data/koje/hyprland/koje.conf
hyprconf=$config/hypr/hyprland.conf
source_line="source = $snippet"

bold=$(tput bold 2>/dev/null || true) reset=$(tput sgr0 2>/dev/null || true)

# --- 1. dependencies ----------------------------------------------------------
echo "${bold}Checking dependencies${reset}"
# command → Debian/Ubuntu package
deps=(
  "podman:podman" "newuidmap:uidmap" "pasta:passt" "fuse-overlayfs:fuse-overlayfs"
  "Hyprland:hyprland" "waybar:waybar" "fc-cache:fontconfig" "xdg-dbus-proxy:xdg-dbus-proxy"
)
portals=("xdg-desktop-portal-hyprland" "xdg-desktop-portal-gtk")
missing=()
for d in "${deps[@]}"; do
  cmd=${d%%:*} pkg=${d##*:}
  command -v "$cmd" >/dev/null 2>&1 || missing+=("$pkg")
done
if command -v dpkg-query >/dev/null 2>&1; then
  for p in "${portals[@]}"; do
    dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed" || missing+=("$p")
  done
fi
if ((${#missing[@]})); then
  echo "  missing: ${missing[*]}"
  if grep -qiE '^(ID|ID_LIKE)=.*(debian|ubuntu)' /etc/os-release 2>/dev/null; then
    echo "  install with:  sudo apt install ${missing[*]}"
  else
    echo "  (package names are Debian/Ubuntu's; use your distribution's equivalents)"
  fi
else
  echo "  all found"
fi
if ((build)) && ! command -v cargo >/dev/null 2>&1; then
  echo "  cargo is needed to build koje: https://rustup.rs (or run with --skip-build)" >&2
  exit 1
fi

# --- 2. plan --------------------------------------------------------------------
# Each entry: "<mark>|<what>|<action function>|<args…>"; marks:
#   + new   ! overwrite (differs)   = unchanged   ~ append   - skipped
plan=()
add() { plan+=("$1|$2|$3"); }

plan_copy() { # plan_copy <src> <dst> <label>
  local src=$1 dst=$2 label=$3
  if [[ -e "$dst" ]]; then
    if cmp -s "$src" "$dst"; then add "=" "$label: $dst" ""; else add "!" "$label: $dst" "copy $src $dst"; fi
  else
    add "+" "$label: $dst" "copy $src $dst"
  fi
}

if ((build)); then
  for b in koje kojed; do
    if [[ -e "$bin/$b" ]]; then add "!" "binary: $bin/$b (rebuilt)" "binary $b"; else add "+" "binary: $bin/$b" "binary $b"; fi
  done
fi

plan_copy assets/fonts/lucide/lucide.ttf "$data/fonts/koje/lucide.ttf" "icon font"
plan_copy assets/fonts/lucide/LICENSE "$data/fonts/koje/LICENSE" "icon font license"
plan_copy assets/hyprland/koje.conf "$snippet" "Hyprland snippet"

if [[ ! -e "$hyprconf" ]]; then
  add "+" "Hyprland config: $hyprconf (new, sources koje's snippet)" "hyprconf new"
elif grep -qF "$source_line" "$hyprconf" || grep -qF "source = ~/${snippet#"$HOME"/}" "$hyprconf"; then
  add "=" "Hyprland config: $hyprconf already sources koje's snippet" ""
else
  add "~" "Hyprland config: $hyprconf (append: $source_line)" "hyprconf append"
fi

waybar_dir=$config/waybar
# Which Waybar files are koje's: links to the old spike draft, or files starting
# with koje's header (installed earlier). Anything else is the user's own config.
is_koje_waybar() { # is_koje_waybar <file>
  [[ -L "$1" && "$(readlink "$1")" == */docs/spikes/setup/waybar/* ]] && return 0
  [[ -f "$1" ]] && head -1 "$1" | grep -qE "^(//|/\*) koje's Waybar"
}
own_waybar=1
for f in config config.jsonc style.css; do
  t=$waybar_dir/$f
  if [[ -e "$t" || -L "$t" ]] && ! is_koje_waybar "$t"; then own_waybar=0; fi
done
if ((own_waybar)); then
  for f in config.jsonc style.css; do
    t=$waybar_dir/$f
    if [[ -L "$t" ]]; then add "!" "Waybar: $t (replaces link to the old spike draft)" "copy assets/waybar/$f $t"
    elif [[ -e "$t" ]]; then plan_copy "assets/waybar/$f" "$t" "Waybar"
    else add "+" "Waybar: $t" "copy assets/waybar/$f $t"; fi
  done
else
  add "-" "Waybar: you have your own config in $waybar_dir – left alone" ""
fi

echo
echo "${bold}Plan${reset}  (+ new  ! overwrite  ~ append  = unchanged  - skipped)"
changes=0 warnings=0
for p in "${plan[@]}"; do
  IFS='|' read -r mark what _ <<<"$p"
  echo "  $mark ${what//$HOME/\~}"
  case "$mark" in
    "+") changes=$((changes + 1)) ;;
    "!" | "~") changes=$((changes + 1)) warnings=$((warnings + 1)) ;;
  esac
done
if ((warnings)); then
  echo
  echo "  ${bold}$warnings existing file(s) will be overwritten or changed (marked ! and ~).${reset}"
fi
if ((!own_waybar)); then
  echo "  To use koje's bar instead: move ${waybar_dir//$HOME/\~} away and run ./install.sh again."
fi

if ((dry)); then echo; echo "dry run – nothing changed"; exit 0; fi
if ((!changes)); then echo; echo "nothing to do"; exit 0; fi
if ((!yes)); then
  echo
  read -r -p "Continue? [y/N] " answer || answer=""
  [[ "$answer" =~ ^[yY] ]] || { echo "aborted – nothing changed"; exit 1; }
fi

# --- 3. execute -----------------------------------------------------------------
copy() { mkdir -p "$(dirname "$2")"; rm -f "$2"; cp "$1" "$2"; }

built=0
binary() {
  if ((!built)); then
    echo "building koje (release) …"
    cargo build --release --locked -p koje -p kojed
    built=1
  fi
  mkdir -p "$bin"
  install -m 755 "target/release/$1" "$bin/$1"
}

hyprconf() {
  mkdir -p "$(dirname "$hyprconf")"
  if [[ "$1" == new ]]; then
    printf '# Hyprland config – your own settings go here.\n\n# added by koje install.sh\n%s\n' "$source_line" >"$hyprconf"
  else
    printf '\n# added by koje install.sh\n%s\n' "$source_line" >>"$hyprconf"
  fi
}

echo
for p in "${plan[@]}"; do
  IFS='|' read -r mark what action <<<"$p"
  [[ -n "$action" ]] || continue
  # shellcheck disable=SC2086 # action is "<function> <args…>" without spaces in args
  $action
  echo "  done: ${what//$HOME/\~}"
done
command -v fc-cache >/dev/null 2>&1 && fc-cache -f "$data/fonts/koje" >/dev/null 2>&1 || true

echo
echo "${bold}Next${reset}"
case ":$PATH:" in *":$bin:"*) ;; *) echo "  add ${bin//$HOME/\~} to your PATH" ;; esac
echo "  in Hyprland: hyprctl reload; restart Waybar: pkill waybar; waybar &"
