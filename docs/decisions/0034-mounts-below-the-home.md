# 0034. Mounts are folders below the home

- Status: Accepted
- Date: 2026-10-22
- Source: discussion after M5.11 part 1, spike M2.12

## Context

Folders below `$HOME` can be mounted into a running context and removed again
at any time, as real mounts at their own path (ADR 0033): the context's home is
shared for propagation. Folders elsewhere could only be mounts of the
container, fixed when it is created – changing them needs a recreate, which
loses what was installed in the container. Making the container's root
propagate too, or recreating while keeping the container's changes (`podman
commit`), would be more work and is unproven. The user's work lives below the
home anyway.

## Decision

- For the first version, `mounts` in `context.toml` may only name **folders
  below the home** (`~/…`, `$HOME/…`, or an absolute path inside it; not the
  home itself). Anything else is refused when the mounts are checked (`koje
  edit`, `koje up`), also after resolving symlinks.
- `allow_unsafe` does not lift this rule; it still lifts the protected-folder
  rules of ADR 0009 inside the home.
- Every mount is therefore a live mount (ADR 0033); the container's own mounts
  never change because of `mounts`.

## Consequences

+ Adding, removing and changing mounts never needs a recreate; `koje
  down`/`koje up` and `koje edit` follow the config.
+ One mechanism, one set of messages.
- Data elsewhere (`/srv`, a second disk under `/mnt`) cannot be mounted; a
  symlink in the home pointing there is refused too (it is resolved first). It
  needs a bind mount on the host into the home, or a later koje version (open
  question: root propagation or recreate with `podman commit`).
