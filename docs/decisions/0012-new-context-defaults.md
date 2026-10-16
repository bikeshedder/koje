# 0012. Defaults for new contexts

- Status: Accepted
- Date: 2026-10-16
- Source: plan task M1.4, discussion

## Context

`koje new <context>` should work without options, so it needs a default prefix
(the short label in bars and menus) and a default border color that tells
contexts apart.

## Decision

- **Prefix:** the initials of the `-`-separated parts of the name, or the first two
  characters of a single-word name, uppercase: `kunden-portal` → `KP`,
  `kundenportal` → `KU`, `x` → `X`, `my-big-project` → `MBP`. `--prefix` overrides.
- **Color:** the first color of a fixed palette that no existing context uses; if
  all are used, the least used one (palette order breaks ties). `--color` overrides.

  | # | Color | | # | Color |
  |---|---|---|---|---|
  | 1 | `#2E7D5B` green | | 6 | `#00ACC1` cyan |
  | 2 | `#1E88E5` blue | | 7 | `#FDD835` yellow |
  | 3 | `#E53935` red | | 8 | `#D81B60` pink |
  | 4 | `#FB8C00` orange | | 9 | `#6D4C41` brown |
  | 5 | `#8E24AA` purple | | 10 | `#546E7A` blue-grey |

- **File:** `koje new` writes `context.toml` from a template with the optional
  keys (`limits`, `states`) as comments, because the file is maintained by hand.

## Consequences

+ `koje new kp` just works; the first ten contexts get distinct colors.
- The palette lives in code (`context::PALETTE`); making it configurable is a
  later change if needed.
