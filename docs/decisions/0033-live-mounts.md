# 0033. Mounts follow `context.toml` while a context runs

- Status: Accepted
- Date: 2026-10-21
- Source: plan M5.11, spike [M2.12](../spikes/M2.12-live-mounts.md) (round 3), discussion

## Context

Changing a context's mounts or access meant recreating its container, which
loses what was installed in it (ADR 0023). Spike M2.12 showed that folders can
be mounted into a running rootless container without root, through a directory
that is shared inside rootless Podman's namespace and mounted into the
container with `rslave`. Round 3 used the context's home as that directory, so
folders appear at their own path as real mounts. Everything the user works on
is below `$HOME`.

## Decision

- **The home is the propagation point.** `koje up` creates the home volume if
  needed, makes its directory a shared mount point in Podman's namespace before
  the container starts, and the container mounts that directory at `$HOME` with
  `rslave` instead of the named volume – the same data, ownership right without
  `:U`.
- **Configured mounts stay real mounts at their own path** when the container is
  created; the container records them in the label `io.koje.home-mounts` (only
  containers with a shared home have it).
- **`koje up` and `koje edit`** bring a running context in line with
  `context.toml`:
  - a configured folder below `$HOME` that the container was not created with
    is bound to `<home volume dir>/<its path below $HOME>` in Podman's namespace:
    a real mount at its own path in the context;
  - a live folder no longer configured is unmounted lazily (`umount -l`: a
    process using it keeps running);
  - a real mount no longer configured, and a new folder outside `$HOME`, wait
    for the next recreate; koje says so.
- Mounts are read-write. Read-only mounts, if they come to `context.toml`, need a
  recreate (or the staging trick of M2.12 round 2).
- `koje rm` releases the shared home in Podman's namespace before it removes the
  volume.
- **Other access changes** (M5.11 part 2): koje notices them (a fingerprint of the
  container's settings as a label) on `koje edit` and `koje up` and asks whether
  to recreate now – saving the state first and restoring it after; it never
  recreates without asking.

## Consequences

+ Adding a project folder below the home to a running context takes effect
  immediately, at its usual path, without symlinks.
+ After a reboot `koje up` mounts the added folders live again; they stay
  correct until the next recreate makes them real mounts.
- Only folders below `$HOME` can be added live.
- A folder mounted over an existing directory of the context's home hides that
  directory's content while mounted.
- Containers created before this need one recreate to get the shared home.
