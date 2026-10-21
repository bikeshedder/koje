# 0033. Mounts follow `context.toml` while a context runs

- Status: Accepted
- Date: 2026-10-21
- Source: plan M5.11, spike [M2.12](../spikes/M2.12-live-mounts.md), discussion

## Context

Changing a context's mounts or access meant recreating its container, which
loses what was installed in it (ADR 0023: the context container is the user's
workspace). Spike M2.12 showed that folders can be mounted into a running
rootless container without root: a directory shared inside rootless Podman's
namespace, mounted into the container with `rslave`, passes later submounts on.

## Decision

- **Configured mounts stay real mounts at their own path** when the container is
  created; the container records them in the label `io.koje.mounts`.
- **Every context has a live directory**, `$XDG_RUNTIME_DIR/koje/<context>/mounts`,
  made a shared mount point in Podman's namespace by `koje up` before the
  container starts, and mounted at `$XDG_RUNTIME_DIR/koje-mounts` in it
  (`rslave`).
- **`koje up` and `koje edit`** bring a running context in line with
  `context.toml`:
  - a configured folder the container was not created with is bound into the
    live directory (`podman unshare mount --bind`) and shown at its own path by a
    symlink inside the context (as the user in the home, as root elsewhere; if
    something real is at that path, only the live directory has it);
  - a live folder no longer configured is unmounted lazily (`umount -l`: a
    process using it keeps running) and its symlink removed;
  - a real mount no longer configured stays until the container is recreated;
    koje says so.
- Mounts are read-write. Read-only mounts, if they come to `context.toml`, need a
  recreate (M2.12 found a way to make them live too; not needed now).
- **Other access changes** (M5.11 part 2): koje notices them (a fingerprint of the
  container's settings as a label) on `koje edit` and `koje up` and asks whether
  to recreate now – saving the state first and restoring it after; it never
  recreates without asking.

## Consequences

+ Adding a project folder to a running context takes effect immediately.
+ After a reboot `koje up` mounts the added folders live again; they stay
  correct until the next recreate makes them real mounts.
- A live folder is a symlink inside the context: programs that resolve links
  see `/run/user/<uid>/koje-mounts/…`.
- Containers created before this need one recreate to get the live directory.
