# 0006. Crash recovery via a separate recovery file

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

An "autosave" option on states would make `down --no-save` meaningless because the
state would already be written.

## Decision

kojed continuously writes the current state (debounced, atomic temp+rename) to
`~/.local/state/koje/<ctx>/recovery.toml`. A clean `down` deletes it. If `up` finds
one, it asks; `--recover`/`--no-recover` decide without asking.

## Consequences

+ No setting needed; state semantics unchanged.
- Programs restore their own content (browser tabs, editor buffers); koje restores
  programs, layouts and geometry only.
