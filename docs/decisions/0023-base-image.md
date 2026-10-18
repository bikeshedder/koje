# 0023. The base image `koje/base`

- Status: Accepted
- Date: 2026-10-18
- Source: plan task M3.3, spikes M2.3–M2.10, discussion

## Context

Every context runs from an image. The spikes worked out what it needs (nested
Podman, session bus, graphics, sound, printing) on a Fedora base, because
Firefox is a real package there (on Ubuntu it is a Snap, which does not run in a
container). Contexts run with `--userns=keep-id` and mount the home at the
host's path, so the image must contain the host user with the same name, uid
and home.

## Decision

- **Built locally, per user,** from `assets/image/Containerfile` (compiled into
  koje): `koje image build [--no-cache]`; `install.sh` builds it if missing;
  `koje up` will build it on first use (M3.4). No registry, no published image.
- **Batteries included:** Firefox, foot, Nautilus, xdg-utils, git and basic
  tools besides what every context needs: D-Bus session bus, gnome-keyring (own
  keyring per context, ADR 0019), Mesa (AMD/Intel; NVIDIA comes via CDI at
  runtime, ADR 0016), PipeWire/PulseAudio and CUPS clients, fonts, rootless
  Podman with fuse-overlayfs and the inner `containers.conf` (ADR 0014).
- **Fedora 44** as the base, tagged `localhost/koje/base:latest`, labelled
  `io.koje.image=base` and `io.koje.image-version`.
- The image's own `xdg-desktop-portal` is **not D-Bus activatable**: koje's
  forwarder owns the portal name on the context bus (ADR 0019).
- A context can use another image (`image = …` in `context.toml`), e.g. one
  built `FROM koje/base` with more programs.

## Consequences

+ No registry or CI publishing; the image always matches the user.
+ A new context is usable at once (browser, terminal, file manager).
- The first build takes a few minutes and needs network.
- Updates are rebuilds (`koje image build --no-cache`); koje does not notice
  outdated images yet (follow-up: compare `io.koje.image-version`).
- All contexts share the image's layers, which also saves memory (M2.6).
