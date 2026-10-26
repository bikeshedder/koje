# 0045. koje ships as one Debian package, built in the dev container

- Status: Accepted (amended 2026-10-26: built in an Ubuntu 26.04 container, on
  the host or in the dev container – see the decision)
- Date: 2026-10-26
- Source: discussion (the host cannot build koje-shell without GTK's
  development packages, M8.27); revises ADR 0021

## Context

`install.sh` (ADR 0021) builds koje on the host with cargo and installs the
binaries below `~/.local`. Since koje-shell (ADR 0042) the host would also need
the development packages of GTK4, libadwaita and gtk4-layer-shell, a Rust
toolchain and the musl target for `koje-forward` – just to install koje.

The dev container already has all of that. Binaries built there (Debian
trixie) run on the host (Ubuntu 26.04): its glibc is newer, and GTK4,
libadwaita and gtk4-layer-shell keep their sonames. Debian's packaging works
out the runtime libraries a binary needs.

## Decision

- **One package `koje`** holds everything that is not per user:

  | Path | What |
  |---|---|
  | `/usr/bin/koje`, `kojed`, `koje-shell` | the binaries |
  | `/usr/lib/koje/koje-forward` | the static forwarder mounted into contexts (ADR 0025) |
  | `/usr/lib/koje/koje-window-picker`, `koje-context-picker`, `koje-app-menu`, `koje-keys` | the fuzzel helpers |
  | `/usr/lib/systemd/user/kojed.service`, `koje-shell.service` | the user units |
  | `/usr/share/fonts/truetype/koje/lucide.ttf` | the icon font (ADR 0020) |

- **Built in an Ubuntu 26.04 container** (amended): `just deb`
  (`scripts/build-deb.sh`) builds the release binaries (`koje-forward` for
  musl), the Hyprland plugin (ADR 0051) and the package with `cargo-deb` in a
  Podman container of `build/Containerfile` – against the libraries of the
  distribution koje runs on, the same on the host and in the dev container
  (only Podman and git needed). It writes `target/debian/`; the container's
  own target directory is `target/ubuntu`. (First version: built directly in
  the dev container, against Debian trixie's libraries – the host could not
  run `just deb`, and the plugin had to match Ubuntu's Hyprland anyway.) The workspace is shared with the host, which installs it
  with `sudo apt install ./target/debian/koje_<version>_amd64.deb`. The version
  is the workspace version plus the commit (`0.1.0+git<date>.<commit>`), so
  every build is newer than the last.
- **Dependencies are declared:** the shared libraries are found by
  `dpkg-shlibdeps`; Podman and what contexts need (`podman`, `uidmap`,
  `passt`, `fuse-overlayfs`, `xdg-dbus-proxy`) and the helpers' tools
  (`fuzzel`, `jq`, `fontconfig`) are `Depends`; the session (Hyprland, Waybar,
  foot, mako, hypridle, hyprlock, grim, slurp, wl-clipboard, wireplumber, the
  portals) is `Recommends`, since Hyprland may be built by hand. `koje doctor`
  keeps checking commands.
- **`koje setup` stays per user** and is run after every install or upgrade:
  koje's Hyprland and hyprbars snippets in `$XDG_DATA_HOME/koje/hyprland/`,
  the `source` line in `hyprland.conf`, the Waybar config, the base image,
  restarting a running kojed. It no longer installs binaries, the font, the
  helpers or the unit.
- **The old layout is cleaned up:** `koje setup` removes what `install.sh` put
  into `~/.local` (`~/.local/bin/koje` and `kojed`, the forwarder and helpers
  in `~/.local/lib/koje/`, the font in `$XDG_DATA_HOME/fonts/koje/`) and the
  user unit `~/.config/systemd/user/kojed.service`, which would hide the
  package's. Only koje's own files, shown in the plan first. `hyprbars.so`
  stays: `scripts/setup-hyprbars.sh` builds it per user for the running
  Hyprland.
- Commands in the snippet, the Waybar config and the units name `koje` and
  `/usr/lib/koje/…` instead of `~/.local/…`.
- **`install.sh` goes away**, with `just install`.
- Later: a CI job attaches the package to a GitHub release on tags.

## Consequences

+ The host needs no Rust toolchain and no development packages; installing and
  upgrading is one `apt install`, removing one `apt remove`.
+ apt pulls in the runtime libraries and Podman.
+ One build for every machine; the dev container, which runs `just check`,
  also builds what is installed.
- Installing needs `sudo`; ADR 0021's installer never did.
- Two steps after an upgrade: `apt install`, then `koje setup` – a package
  cannot change the user's files or restart user services.
- Packages for amd64 and Debian/Ubuntu only; other distributions build from
  source with cargo by hand.
- The build image (Ubuntu 26.04, Rust, cargo-deb) is built once per change of
  its Containerfile or the toolchain; crates are cached in a Podman volume.
- Follow-ups: plan tasks M0.9 (the package, `just deb`, `koje setup`'s
  cleanup) and M0.10 (CI release); M8.32 adds `koje-shell` to the session.
