# M1 – Model & files

Goal: everything koje stores on disk works end to end through the CLI – without any
container or Hyprland. Pure Rust, testable in the dev container.

- [x] **M1.1 Path expansion and mount validation**
  - Done when: `~` and `$HOME` in `mounts` expand against the real home; relative paths are rejected; duplicate mounts rejected; tests.
  - Notes: `mount::MountPath` validates on parse (also rejects `~user`, other `$VARS`, `..`); duplicates need the home, so they are checked in `ContextConfig::expand_mounts(home)` – callers (M1.5 `edit`, M3.2) must call it. Refused targets: ADR 0009, implemented in M1.11.
- [ ] **M1.2 Global config loading with defaults**
  - Done when: missing `config.toml` = defaults; `Limits::or` applied when resolving a context's effective limits; tests.
- [ ] **M1.3 Load and save states**
  - Done when: `State` round-trips the concept example; `State::issues` checked on load (error vs warning decided and documented); writes are atomic (temp file + rename in the same dir); writing `empty` is refused; tests.
- [ ] **M1.4 `koje new <context>`**
  - Done when: creates `context.toml` with name, prefix (default: first letters, uppercase), color (default: next unused color from a fixed palette), default image; refuses existing names; CLI test with temp XDG dirs.
  - Notes: palette and prefix rule are small decisions → ADR.
- [ ] **M1.5 `koje edit <context>` and `koje rm <context>`**
  - Done when: `edit` opens `$VISUAL`/`$EDITOR` and re-validates after save (reopen on error); `rm` asks for confirmation unless `--yes`, removes config + states + recovery file. (Container removal joins in M3.)
- [ ] **M1.6 `koje state list|cp|mv|rm`**
  - Done when: list shows name, window/layout counts, modified time and marks role states; cp/mv/rm follow ADR 0005 (`empty` reserved, roles cannot be removed while configured? → decide); CLI tests.
- [ ] **M1.7 `koje list` with more detail**
  - Done when: shows prefix, color swatch (truecolor when on a TTY), number of states; machine-readable `--json`.
- [ ] **M1.8 Layout model operations**
  - Done when: pure functions for `layout new [--copy]`, `mv`, `rm` (cannot remove active layout), `autosave on|off`, next/prev order, "window closed → remove from all layouts", "new window → join active layout"; unit tests.
- [ ] **M1.9 State switch planning**
  - Done when: pure function computing keep/close/start sets when switching states in a running context (concept "Zustand wechseln"); unit tests.
- [ ] **M1.10 Recovery file model**
  - Done when: write/read/delete helpers, atomic; "recovery present" detection; unit tests. (Writing it continuously is M5.)
- [ ] **M1.11 Refuse dangerous mount targets** (ADR 0009)
  - Done when: `mounts` accepts string and `{ path, unsafe }` table form (round-trips both); the three rule groups of ADR 0009 are refused unless `unsafe = true`; protected paths come from `Paths`/XDG, not hard-coded home paths; tests per rule.
