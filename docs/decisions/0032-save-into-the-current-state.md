# 0032. Saving goes into the state a context is in

- Status: Accepted
- Date: 2026-10-21
- Source: plan M5.9, host test, discussion
- Supersedes: the save targets of ADR 0005 (`down`/`save` without `-s`, and the
  save before a state switch)

## Context

ADR 0005 always saved into the `default` role: `koje down` without `-s`, and
the current windows before `koje up -s <other>` switched a running context.
That is right only while the windows came from `default`. The host test showed
the failure: after `up -s empty`, switching back with `up -s default` first saved
the empty set of windows into `default` and destroyed it. Likewise, after
`up -s workshop`, a plain `down` wrote the workshop windows into `default`.

## Decision

- kojed remembers each running context's **current state**: the state it was
  loaded from (`up`, also from the recovery file) or last switched to. `empty`
  and fallbacks to `empty` mean none. The name lives beside the recovery file
  (`~/.local/state/koje/<context>/current-state`), so it survives a kojed
  restart; a clean `down` deletes it.
- **`koje save` and `koje down` without `-s`** write the current state; with
  none, the `default` role. `-s <name>` still writes exactly that state.
- **Before a switch** (`up -s <other>` on a running context) the current windows
  go into the current state unless `--no-save`; from `empty` nothing is saved.
  Switching to the current state itself only saves.

## Consequences

+ A state is only overwritten by windows that came from it; `empty` and other
  states never overwrite `default` by accident.
+ Working in `workshop` and shutting down keeps the work in `workshop`.
- The plain `up` still loads `default`: after `up -s workshop` … `down`, the next
  plain `up` shows `default`, not `workshop`. `down` says which state it wrote.
- Without kojed, `save`/`down` fall back to `default` (they need kojed for the
  windows anyway).
