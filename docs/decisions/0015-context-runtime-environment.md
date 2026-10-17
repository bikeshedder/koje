# 0015. Runtime environment of a context container

- Status: Accepted (amended by ADR 0017: Wayland and sound sockets depend on the access profile)
- Date: 2026-10-17
- Source: spike [M2.3](../spikes/M2.3-context-container.md) on Ubuntu 26.04 / GNOME

## Context

Programs in a context need the desktop (Wayland), sound, IPC among themselves and
a usable `/dev/shm`. The spike found four non-obvious failures: Firefox could not
find its running instance without a session bus, libpulse rejected a root-owned
socket directory, VS Code's renderer crashed with the default 64 MB `/dev/shm`,
and `sleep infinity` as PID 1 delayed every stop by 10 s.

## Decision

- **Runtime dir:** `XDG_RUNTIME_DIR=/run/user/<uid>` is a private tmpfs
  (`--mount type=tmpfs,…,tmpfs-mode=0700,U=true`) that contains only the sockets
  the context may use. The host's runtime dir is never mounted as a whole.
- **Wayland:** the compositor's socket at the same name (`WAYLAND_DISPLAY`).
- **Sound:** the PipeWire socket at `/run/user/<uid>/pipewire-0`; the PulseAudio
  socket at a path of its own (`/run/user/<uid>/pulse-native`) with
  `PULSE_SERVER` set, never below a mount-point directory owned by root.
- **Session bus:** each context runs its own D-Bus session bus as the container's
  main process (`dbus-daemon --session --nofork`, under `--init`), with
  `DBUS_SESSION_BUS_ADDRESS` set. Programs of one context find each other over it
  (Firefox remoting, `xdg-open`). What may reach the host's session bus is M2.4.
- **`/dev/shm`:** `--shm-size 1g` (a limit, not reserved memory).
- **PID 1:** `--init`.

## Consequences

+ Links, `xdg-open` and single-instance apps work inside a context without
  touching the host's session bus.
+ Chromium/Electron apps run stable.
- Sockets are bind-mounted files: if the host's PipeWire or compositor restarts,
  the context keeps a stale socket and must be restarted (or koje must remount;
  follow-up in M3).
