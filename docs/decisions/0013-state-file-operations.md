# 0013. State file operations and role states

- Status: Accepted
- Date: 2026-10-16
- Source: plan task M1.6, discussion; refines ADR 0005

## Context

ADR 0005 says states are deleted only with `koje state rm` and copied or renamed
with `state cp|mv`. Open were: may the states playing the `default` and `fresh`
roles be removed or renamed, and may `cp`/`mv` replace an existing state?

## Decision

- **Role states are not protected.** `state rm` and `state mv` work on them and
  print a note: removing `default` means `up` falls back to `fresh` until the next
  save; removing `fresh` means a missing state falls back to `empty`; renaming a
  role state leaves the role in `context.toml` pointing at the old name.
- **`cp` and `mv` never overwrite.** An existing target is an error that points to
  `koje state rm`; replacing a state is "remove, then copy". This keeps deletion in
  one command.
- **`empty`** can be the source of `cp` (writes a state with no windows and one
  empty layout, ADR 0011), but never a target, and cannot be renamed or removed.
- `cp` validates the source and copies the file byte for byte, so hand-written
  formatting survives.
- `state list` shows stored states, then role states not saved yet, then `empty`.

## Consequences

+ No hidden deletes; the load fallback (ADR 0005) already handles missing roles.
- Overwriting takes two commands.
