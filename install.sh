#!/usr/bin/env bash
# koje installer (ADR 0021). Run from a checkout on the host:
#
#   ./install.sh             build and install koje, then run `koje setup`
#   ./install.sh --dry-run   only show what would happen
#   ./install.sh --yes       do not ask
#   ./install.sh --skip-build   do not build; only run `koje setup`
#
# It builds koje, kojed and the static koje-forward and installs them below
# ~/.local. Everything else – icon font, Hyprland snippet, kojed's systemd unit,
# Waybar, the base image – is `koje setup`, which shows its plan and asks once.
# Nothing here runs sudo; `koje doctor` lists missing system packages.
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"

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

bin=$HOME/.local/bin
libdir=$HOME/.local/lib/koje
musl=x86_64-unknown-linux-musl
setup_args=()
((dry)) && setup_args+=(--dry-run)
((yes)) && setup_args+=(--yes)

if ((build)); then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is needed to build koje: https://rustup.rs (or run with --skip-build)" >&2
    exit 1
  fi
  echo "Binaries"
  for b in koje kojed; do echo "  $bin/$b"; done
  echo "  $libdir/koje-forward (static, mounted into contexts)"
  if ((dry)); then
    echo "  (dry run: not built)"
  else
    echo "building koje (release) …"
    cargo build --release --locked -p koje -p kojed
    if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "$musl"; then
      echo "adding the Rust target $musl (for the static forwarder) …"
      rustup target add "$musl"
    fi
    echo "building koje-forward (static) …"
    cargo build --release --locked -p koje-forward --target "$musl"
    mkdir -p "$bin" "$libdir"
    # Via a temp file and rename: running binaries are not overwritten in place.
    for b in koje kojed; do
      install -m 755 "target/release/$b" "$bin/$b.koje-new"
      mv -f "$bin/$b.koje-new" "$bin/$b"
    done
    install -m 755 "target/$musl/release/koje-forward" "$libdir/koje-forward.koje-new"
    mv -f "$libdir/koje-forward.koje-new" "$libdir/koje-forward"
    # A running kojed picks up the new binary only when restarted.
    if systemctl --user is-active --quiet kojed 2>/dev/null; then
      systemctl --user restart kojed && echo "  restarted kojed"
    fi
  fi
  echo
fi

# A dry run builds nothing: the plan must come from the koje this checkout
# would install, not from an older installed one (which may lack `setup`).
if ((dry && build)); then
  exec cargo run --quiet --release --locked -p koje -- setup "${setup_args[@]}"
fi
koje=$bin/koje
if [[ ! -x "$koje" ]]; then
  koje=$(command -v koje || true)
  [[ -n "$koje" ]] || { echo "koje is not installed – run without --skip-build" >&2; exit 1; }
fi
case ":$PATH:" in *":$bin:"*) ;; *) echo "note: add ${bin//$HOME/\~} to your PATH" ;; esac
exec "$koje" setup "${setup_args[@]}"
