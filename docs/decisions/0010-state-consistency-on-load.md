# 0010. State consistency on load

- Status: Accepted
- Date: 2026-10-16
- Source: plan task M1.3, discussion

## Context

State files are written by koje but may be edited by hand. `State::issues` finds
three kinds of inconsistency: a window id used twice, a layout naming a window the
state does not have, and an `active_layout` that does not exist. Refusing to load
would block `koje up` over a typo; silently guessing could start the wrong windows.

## Decision

- **Duplicate window ids** are an error. Which window a layout means is ambiguous.
- **Layout entries for unknown windows** are dropped, matching the rule that
  closed windows leave all layouts (ADR 0004).
- **A missing active layout** is replaced by the first layout in file order; if
  the state has no layout, an empty layout of that name is created.
- Repairs are returned to the caller (`state::Loaded::repaired`) and reported as
  warnings. They reach the file the next time the state is saved.
- Saving a state that still has issues is refused, so koje never writes an
  inconsistent file.

## Consequences

+ A hand edit can only block loading when it is genuinely ambiguous.
- A repaired state differs from the file until the next save; the warning says so.
