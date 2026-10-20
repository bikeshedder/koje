# 0031. Integrations are declarative

- Status: Accepted
- Date: 2026-10-20
- Source: plan M5.2, concept "Integrationen", open question "Integrations"

## Context

Programs work without an integration (concept): koje remembers the command line
and starts it again. Integrations improve launching and what is remembered. The
open question was whether a declarative file is enough, and how `terminal`
finds the working directory.

## Decision

- An integration is a **TOML file** as in the concept: `name`, `match`
  (`app_id` list), `launch` (template with `{command}` and remembered values),
  `remember` (keys stored in the state), `detect` (key → method). No executable
  hooks for now.
- **Detect methods** (a fixed set, implemented by kojed from `/proc` on the
  host): `argv-last` (the last argument of the window's process), `cwd` (its
  working directory) and `shell-cwd` for terminals:
  1. one shell below the terminal process → that shell's working directory;
  2. several (one process, many windows, e.g. foot server mode) → the window
     title if it holds a path (`~/src/kp`, `user@host: ~/src/kp`);
  3. otherwise nothing; the terminal starts in the home directory.
  OSC 7 is not usable: the terminal consumes it and exposes it to nobody.
  - `file-manager` (M5.12): the folder of a file manager window. Nautilus runs
    one process for all windows, so kojed asks it once per context over the
    context's bus (`org.freedesktop.FileManager1.OpenWindowsWithLocations`,
    each window's tabs) and gives each window the tab named like its title
    (the active one), the rest in order. Tabs are not restored (no command line
    option); a window reopens with its active folder.
- **Built-ins** `generic`, `terminal`, `editor`, `browser`, `files` ship in koje (same
  format); files in `~/.config/koje/integrations/` add to or replace them by
  name. `devcontainer` follows with dev container support in states.
- **`quit`** (M5.4): how `koje down` ends a program – `close` (default: its
  windows are closed; it may ask to save) or `signal` (one SIGTERM to its
  process: it quits with all windows, so a browser's session keeps every
  window; closing them one by one would leave only the last). Built-in
  `browser` uses `signal`. Whether a browser reopens its windows on the next
  start is its own setting (Firefox: "Open previous windows and tabs").
- koje never changes a program's configuration to make detection work.

## Consequences

+ Integrations are data: easy to read, share and test.
+ Terminals work without shell configuration in the context.
- Detection is heuristic for one-process-many-windows terminals.
- A program that needs logic beyond these methods waits for a new method (or,
  later, hooks).
