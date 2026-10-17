# 0017. Access profiles: configurable host access per context

- Status: Accepted
- Date: 2026-10-17
- Source: discussion after spike M2.3; amends ADR 0015 and ADR 0016

## Context

The spikes give every context the GPU, sound, the Wayland socket, the devices
for nested containers and a network. Some contexts need less (a headless build
context, an offline context for sensitive material). Mounts are already
configured per context; host devices and sockets should be too.

## Decision

`[access]` sets which host resources a context gets. A **profile** sets all
switches; single **switches** override it.

| Switch | On | Off |
|---|---|---|
| `gpu` | `/dev/dri`, NVIDIA via CDI (ADR 0016) | software rendering |
| `sound` | PipeWire + PulseAudio sockets (playback *and* microphone) | no sound |
| `wayland` | compositor socket | no GUI: shell, terminal, dev containers only |
| `nested` | `/dev/fuse`, `/dev/net/tun`, `unmask=/proc/sys` (ADR 0014) | no dev containers |
| `network` | own network (pasta) | `--network none` |

| Profile | gpu | sound | wayland | nested | network |
|---|---|---|---|---|---|
| `full` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `desktop` | – | ✓ | ✓ | ✓ | ✓ |
| `headless` | – | – | – | ✓ | ✓ |
| `none` | – | – | – | – | – |

```toml
# config.toml – default for all contexts (itself defaulting to "full")
[access]
profile = "full"

# context.toml
[access]
profile = "desktop"
sound   = false
```

**Resolution:** a context's own `profile`, if set, replaces the global one and
the global switches; otherwise the context starts from the global result. The
context's switches are applied last. A missing `[access]` everywhere means `full`.

**Changes:** access is fixed when the container is created. When the effective
access of a *running* context changes (`koje edit`, or `config.toml`), koje asks
whether to apply it now (save the state, recreate the container, restore the
windows) or at the next `koje up`.

Every context keeps its own session bus and runtime dir (ADR 0015) regardless of
the profile; `wayland = false` only drops the compositor socket.

## Consequences

+ One line restricts a context; the default stays "everything works".
+ `none` gives an offline, desktop-less context – isolated from host devices,
  but still sharing the kernel and the folders in `mounts`.
- `sound` cannot separate playback from the microphone yet; that needs
  PipeWire's permission handling (open question).
- "Limited Wayland" (deny screen capture, clipboard managers) is not a switch
  yet; it needs `security-context-v1` support in the compositor (M2.9).
- New term "access profile" in `AGENTS.md`.
