# 0025. Running the host-bus proxy and the forwarder

- Status: Accepted
- Date: 2026-10-19
- Source: plan task M3.14, discussion; implements ADR 0019

## Context

ADR 0019 needs two programs per context: `xdg-dbus-proxy` on the host (a
filtered view of the host's session bus) and a forwarder inside the context
(owns the allowed names on the context bus). kojed (M4) does not exist yet, and
the forwarder must run in every context regardless of its image.

## Decision

- **Forwarder:** a Rust binary `koje-forward` (crate `crates/koje-forward`,
  `zbus`), built **statically (musl)** by `install.sh` into
  `~/.local/lib/koje/koje-forward` and **mounted read-only** into every context
  at `/usr/local/libexec/koje/koje-forward`. `koje up` starts it in the context
  (`podman exec -d`) and waits until it owns its names. It implements the M2.10
  design: one host connection per client, request/session path translation in
  paths and bodies, file descriptors passed on, reconnect when the proxy
  restarts.
- **Proxy:** `koje up` starts `xdg-dbus-proxy` as a transient **systemd user
  unit** `koje-proxy-<context>` (`systemd-run --user`, restarted on failure,
  collected when stopped); `koje down` stops it. Its socket lives in
  `$XDG_RUNTIME_DIR/koje/<context>/host-bus/`, which is mounted into the context.
  The filter comes from the effective access (pure function in `koje-core`).
  kojed may take over the units in M4.
- Only access switches that need the host bus (notifications, file chooser,
  settings, inhibit, screencast, print) start the proxy and the forwarder.

## Consequences

+ No image rebuild for forwarder updates; contexts with any image get it.
+ systemd restarts a crashed proxy; the forwarder reconnects.
- Needs a systemd user session (normal on desktop distributions).
- `install.sh` needs the musl target (`rustup target add x86_64-unknown-linux-musl`).
