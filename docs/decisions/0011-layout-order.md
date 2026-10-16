# 0011. Layout order and the initial layout

- Status: Accepted
- Date: 2026-10-16
- Source: plan tasks M1.3 and M1.8, discussion

## Context

`koje layout next|prev` needs an order. Alphabetical order is simple but lets
names dictate the cycle (`code`, `docs`, `review`). The concept shows layouts in
the order the user made them (`code`, `review`, `docs`). A new state (and the
virtual `empty`) also needs one layout to be active, and the concept does not
name it.

## Decision

- Layouts keep **file order**: the order of `[layout.*]` tables in the state file.
  `koje layout new` appends; `koje layout mv` keeps the position. `next`/`prev`
  cycle in this order and wrap around.
- A state created from nothing has a single empty layout named **`main`**.
- Implementation: `IndexMap` for `State::layout`, `toml` with `preserve_order`.

## Consequences

+ The cycle matches how the user built the layouts; reordering is a hand edit of
  the state file.
- `toml` must keep `preserve_order`; a test guards the round-trip order.
