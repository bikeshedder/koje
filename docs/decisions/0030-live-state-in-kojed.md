# 0030. kojed holds the live state of running contexts

- Status: Accepted
- Date: 2026-10-20
- Source: plan M5.1, discussion

## Context

A state (ADR 0004, 0005) lists a context's windows with stable ids and their
layouts. Saving (`koje save`, `koje down`), autosave of geometry (M5.7), the
crash recovery file (ADR 0006, M5.8) and layout switching (M5.5) all need to know
which window has which id while the context runs. Ids computed only at save time
would change between saves.

## Decision

- **kojed keeps one live `State` per running context** in memory: the windows
  with their ids and specs, the layouts, the active layout.
- **Window ids** are assigned when kojed maps a window to its context: the
  integration's name, else the program's name from its app id (last segment of
  a reverse-DNS id, lowercased, other characters replaced by `-`), plus `-2`,
  `-3`, … if taken (`editor`, `firefox`, `firefox-2`). An id stays with its
  window until the window closes. Ids can be renamed in the state file.
- **New windows join the active layout** with their current placement; **closed
  windows leave the state and all layouts** (ADR 0004). Exception: windows that
  close because koje shuts the context down (M5.4) – the state is taken first.
- Clients get the state over D-Bus (`State(context) → TOML`); `koje save`
  writes it to the state file. Geometry of the active layout is refreshed from
  the compositor when the state is taken (until autosave, M5.7).
- When kojed restarts, it rebuilds the live state from the windows that are
  there (ids are assigned again).
- **Restoring** (M5.3): `koje up` sends the loaded state to kojed (`Restore`),
  which starts the programs (`podman exec` with the integration's launch
  template, in a remembered `cwd`; `launch_once` integrations once). The state's
  windows wait for their windows: a new window takes the first waiting id with
  the same app id, else the first of the same integration (programs may change
  their app id after opening – Firefox), else, without integrations, the same
  program name and is placed from the active layout. Waiting windows that do
  not appear within 60 s leave the state. Several windows of one program are
  matched in start order until M5.10 decides better.

## Consequences

+ Stable ids while a context runs; autosave, recovery and layout switching have
  one place to work on.
- A kojed restart loses renamed ids and non-active layouts of running contexts
  unless they were saved; the recovery file (M5.8) will close that gap.
