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

The file has the format of a state file (no extra fields). Its modification time
is when that state was current ("restore the state from 15:42?").

## Consequences

+ No setting needed; state semantics unchanged.
- Programs restore their own content (browser tabs, editor buffers); koje restores
  programs, layouts and geometry only.
