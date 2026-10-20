# 0029. Sound sockets as links in a mounted directory

- Status: Accepted
- Date: 2026-10-20
- Source: plan task M4.11, host test, discussion

## Context

A context got the host's PipeWire and PulseAudio sockets as bind-mounted files
(ADR 0015). A file mount keeps the inode it was made with: when the sound server
replaces its sockets (`systemctl --user restart pipewire.socket …`, a crashed
server, a new session), the context keeps the dead ones and has no sound until
it is restarted. A first M4.11 version only notified the user about that.
Restarting just the services keeps the sockets (socket activation) and is
harmless.

## Decision

- kojed keeps a **per-context directory** `$XDG_RUNTIME_DIR/koje/<context>/sound/`
  with **hard links** to the host's `pipewire-0` and `pulse/native` (same
  tmpfs, the user's own files). `koje up` asks kojed for it (`SoundSockets`)
  and mounts it at `$XDG_RUNTIME_DIR/koje-sound` in the context;
  `PIPEWIRE_REMOTE` and `PULSE_SERVER` point at the links there.
- Every 5 s kojed compares each link with the host socket (device, inode) and
  re-links a replaced socket (link beside it, then rename).
- No notification: the context does not need a restart.

## Consequences

+ A sound server restart costs running programs their current connection, not
  the context; the next connection (a new stream, an app that reconnects) works.
+ Same pattern as the Wayland socket (ADR 0022): mount a directory, change its
  content.
- Sound needs kojed (like Wayland); without it `koje up` fails for contexts with
  sound.
- Up to 5 s after a restart the links are old; an app connecting in that window
  fails once.
- Containers created before this keep the file mounts until recreated.
- Mounts that change while a context runs (new folders) are a different
  problem: the mount itself would have to change. Spike M2.12.
