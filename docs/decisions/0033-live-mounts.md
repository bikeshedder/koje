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
- **Folders below `$HOME` are always live mounts**: `koje up` binds every
  configured one to `<home volume dir>/<its path below $HOME>` in Podman's
  namespace before the container starts – a real mount at its own path in the
  context. `koje edit` (running or stopped) and the next `koje up` add new ones
  and unmount removed ones lazily (`umount -l`: a process using it keeps
  running). koje finds them by listing the mounts below the shared home.
- **Folders outside `$HOME`** were planned as mounts of the container (label
  `io.koje.home-mounts`, changed by a recreate); ADR 0034 refuses them for the
  first version. The label stays (empty) to recognize containers with a shared
  home.
- Mounts are read-write. Read-only mounts, if they come to `context.toml`, need a
  recreate (or the staging trick of M2.12 round 2).
- `koje rm` releases the shared home in Podman's namespace before it removes the
  volume.
- **Other changes** (M5.11 part 2): image, effective access and limits only take
  effect in a new container. The container carries their fingerprint
  (`io.koje.config`; only configured values, no host details, so koje does not
  ask without a reason). `koje up` of a stopped context with other settings asks
  whether to recreate it now; a running context and `koje edit` point to `koje
  recreate`, which saves the windows and restores them. koje never recreates
  without asking – what was installed in the container is lost.

## Consequences

+ Adding or removing a project folder below the home takes effect immediately,
  at its usual path, without symlinks, and `koje down`/`koje up` follow the
  config – no recreate for mounts below the home.
+ After a reboot `koje up` mounts them again before the container starts.
- Only folders below `$HOME` (ADR 0034).
- A folder mounted over an existing directory of the context's home hides that
  directory's content while mounted.
- Containers created before this need one recreate to get the shared home.
