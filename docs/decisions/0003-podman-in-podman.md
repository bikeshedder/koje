# 0003. Each context runs its own nested Podman

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

Dev containers and compose projects must be isolated per project. Alternatives
were "everything in the dev container" (mixes the shareable devcontainer.json with
personal apps, big images, browser data lost on rebuild) and "sibling containers via
the host Podman socket" (socket access = control over all user containers and any
host path – no security boundary; compose names like `backend_` collide).

## Decision

Each context runs its own rootless Podman with its own image store volume. Shared
base images may come from a read-only `additionalimagestores`. No other variant is
supported.

## Consequences

+ Real isolation, no name collisions, devcontainer.json stays team-shareable.
- Nested rootless Podman needs setup (sub-UIDs, `/dev/fuse`, storage) – spike M2.5.
- Images are stored per context unless shared read-only.
