# 0005. Named states and save/load semantics

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

Several flag designs were discussed (`--clean`, `--no-restore`, `--from`, `--save-initial`,
`--discard`). They were asymmetric or mixed saving with deleting.

## Decision

- All states are named and equal; `default` and `fresh` are *roles* (renamable in
  `context.toml`); `empty` is reserved, virtual and read-only.
- `koje up <ctx> [-s NAME]` loads (default role `default`). Missing → `fresh` → `empty`.
- `koje down <ctx> [-s NAME]` writes exactly that one state. `--no-save` writes nothing
  and changes nothing. `-s` and `--no-save` are mutually exclusive.
- `koje save <ctx> [-s NAME]` saves without shutting down.
- Deleting happens only via `koje state rm`. Copy/rename via `state cp|mv`.

## Consequences

+ Shutting down decides only *whether* to save, starting only *what* to load.
- `down -s review` leaves `default` untouched; the next plain `up` shows the older
  `default`. `koje down` should say which state it wrote.
