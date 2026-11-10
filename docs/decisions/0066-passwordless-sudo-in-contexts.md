# 0066. Passwordless sudo in contexts, as an access switch

- Status: Accepted
- Date: 2026-11-10
- Source: discussion (user, 2026-11-10: "sudo should be available inside the
  container without a password (similar to typical ubuntu live systems) …
  make that configurable"); ADR 0014, 0017; plan G.42

## Context

Contexts run as the host user (same name, uid and home; `--userns=keep-id`,
koje's user layer, ADR 0040), never as root. Installing a package for a
moment, or fixing something in the container, needed `podman exec --user
root` from the host. Users expect `sudo` as on a live system.

## Decision

- A new access switch **`sudo`**: `sudo` without a password for the user
  inside the context. It is configured as every switch (ADR 0017): per
  context in `[access]` of `context.toml`, by default from `config.toml`,
  and in koje Settings among the access switches.
- Profiles: on in `full`, `desktop` and `headless`; off in `isolated`.
- koje writes `/etc/sudoers.d/90-koje` (`<user> ALL=(ALL) NOPASSWD: ALL`,
  0440) as root in the running container at every start, and removes it
  when the switch is off; a change applies to a running context at once.
  It needs no new container, so it is not part of the container's
  fingerprint (M5.11) and changing it asks for no recreate.
- Images need `sudo`; koje's `fedora` has it. An image without it gets the
  warning of a missing tool for the switch (ADR 0040's probe).
- Root inside a context is not root on the host: the container is rootless,
  its root maps to a sub-uid of the user (ADR 0014). It can change the
  container's files (lost on recreate) and what the context can reach.

## Consequences

+ Live-system convenience inside contexts, switchable per context.
- What root installs in a container is lost on recreate; the image is the
  place for it (ADR 0040, `podman diff` hints of M3.23).
- Amends ADR 0017's profile table.
