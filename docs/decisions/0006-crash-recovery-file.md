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
- Update (M5.8/M5.9): `up` asks when the context is stopped and has a
  recovery file, or when it runs but kojed reports its windows lost (a
  restarted kojed found the recovery file and none of its windows – Hyprland or
  the session crashed, the container survived). A running context without
  windows is not a crash (`up -s empty`); kojed keeps its recovery file current
  also then.
