# 0008. Crate structure: pure core with ports

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

Most of koje's behavior is decision logic (which state to load, how to switch
layouts, when to warn) that should be testable without Hyprland or Podman.

## Decision

Cargo workspace: `koje-core` (domain types, pure logic, file formats, XDG paths,
traits `Compositor` and `ContainerRuntime`), `koje` (CLI), `kojed` (service),
`koje-gui` later. Adapters for Hyprland and Podman live outside the core.
Libraries use `thiserror`, binaries `anyhow`.

## Consequences

+ Fast, hermetic tests; adapters are thin.
- Adapter crates may be split out later if the binaries grow.
