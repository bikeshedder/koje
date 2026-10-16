# 0004. Layouts are part of a state

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

Layouts arrange concrete windows. A layout shared across states could reference
windows that do not exist in another state.

## Decision

A state stores its window set and all layouts (per window: monitor, x, y, w, h)
plus the active layout. Windows not in the active layout are stashed. Each layout
has `autosave` (default on); with autosave off it is a template updated via
`koje layout save`. Closed windows leave all layouts; new windows join the active one.

## Consequences

+ Simple, consistent model; day to day only `default` exists, so layouts feel per context.
- Copying layouts between states needs an explicit command later, if wanted.
