# 0019. Host interfaces through a forwarder and a filtered proxy

- Status: Accepted
- Date: 2026-10-17
- Source: spike [M2.4](../spikes/M2.4-filtered-dbus.md), discussion; extends ADR 0015 and ADR 0017

## Context

Each context has its own session bus (ADR 0015), so programs in it never see the
host's bus. Some host services are still wanted: notifications on the desktop,
the host's file chooser, the desktop theme. Opening links on the host
(`OpenURI`) is not: links stay in the context. A Flatpak-style filtered host bus
was rejected because programs in two contexts would register the same names on
the host (e.g. Firefox remoting) and links could cross contexts.

## Decision

- **Per context, koje runs an `xdg-dbus-proxy` on the host** with a filter built
  from the context's effective access. Its socket lives in a per-context
  directory that is mounted into the context (no stale socket after a restart).
- **In the context, a forwarder** owns the allowed names on the context's bus
  (`org.freedesktop.Notifications`, `org.freedesktop.portal.Desktop`) and passes
  method calls to the proxy and signals back. It must own the names before
  anything D-Bus-activates the image's own `xdg-desktop-portal`. The spike used a
  Python prototype; koje ships its own (Rust).
- **Allowed host interfaces**, each an access switch (ADR 0017):
  `notifications`, `file_chooser`, `settings` (read-only), `inhibit`,
  `screencast`, `print`. Everything else is denied, in particular `OpenURI`,
  other portals and other services.
- **Secrets stay in the context:** each context runs its own keyring on its own
  bus; `org.freedesktop.secrets` is never forwarded to the host.

## Consequences

+ Links and single-instance apps stay in their context; selected host services
  work as on the host.
+ The allowed set follows the access profile; no extra configuration.
- The host's portal sees the proxy as an unsandboxed host program: the file
  chooser returns host paths, usable in the context only inside its mounts.
- Screen sharing passes PipeWire file descriptors; the forwarder must carry fds
  (not tested in M2.4: M2.10).
- Sharing selected secrets between contexts is wanted but undesigned (open
  question).
