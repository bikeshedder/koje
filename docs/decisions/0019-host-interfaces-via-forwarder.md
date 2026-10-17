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
- **Printing:** the `print` switch gives the context the host's CUPS socket
  (`/run/cups/cups.sock`) in addition to the print portal. Most programs
  (Firefox, GTK's own dialog) talk to CUPS directly and do not use the portal
  (M2.10). Managing printers still needs the host's admin rights.
- **Secrets stay in the context:** each context runs its own keyring on its own
  bus; `org.freedesktop.secrets` is never forwarded to the host.

## Consequences

+ Links and single-instance apps stay in their context; selected host services
  work as on the host.
+ The allowed set follows the access profile; no extra configuration.
- The host's portal sees the proxy as an unsandboxed host program: the file
  chooser returns host paths, usable in the context only inside its mounts.
- Screen sharing passes PipeWire file descriptors; the forwarder must carry fds.
- Portal clients predict request/session paths from their own bus name; the
  forwarder needs one host connection per client and translates that path
  segment (M2.10). The per-client connection also releases a client's inhibits
  and sessions when it exits.
- With `print`, a context can see and cancel the user's print jobs.
- Sharing selected secrets between contexts is wanted but undesigned (open
  question).
