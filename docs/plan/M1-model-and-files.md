# M1 – Model & files

Goal: everything koje stores on disk works end to end through the CLI – without any
container or Hyprland. Pure Rust, testable in the dev container.

- [x] **M1.1 Path expansion and mount validation**
  - Done when: `~` and `$HOME` in `mounts` expand against the real home; relative paths are rejected; duplicate mounts rejected; tests.
  - Notes: `mount::MountPath` validates on parse (also rejects `~user`, other `$VARS`, `..`); duplicates need the home, so they are checked in `ContextConfig::expand_mounts(home)` – callers (M1.5 `edit`, M3.2) must call it. Refused targets: ADR 0009, implemented in M1.11.
- [x] **M1.2 Global config loading with defaults**
  - Done when: missing `config.toml` = defaults; `Limits::or` applied when resolving a context's effective limits; tests.
  - Notes: `GlobalConfig::load`, `ContextConfig::effective_limits` (re-validates soft < hard after merging).
- [x] **M1.3 Load and save states**
  - Done when: `State` round-trips the concept example; `State::issues` checked on load (error vs warning decided and documented); writes are atomic (temp file + rename in the same dir); writing `empty` is refused; tests.
  - Notes: ADR 0010 (repair dangling, error on duplicates) and ADR 0011 (layouts in file order via `IndexMap` + `toml/preserve_order`; initial layout `main`). `fsutil::write_atomic` also fsyncs; reuse it for every koje file.
- [x] **M1.4 `koje new <context>`**
  - Done when: creates `context.toml` with name, prefix (default: first letters, uppercase), color (default: next unused color from a fixed palette), default image; refuses existing names; CLI test with temp XDG dirs.
  - Notes: ADR 0012. New `context` module (list/load/create); `context::load` also checks that `name` matches the directory.
- [x] **M1.5 `koje edit <context>` and `koje rm <context>`**
  - Done when: `edit` opens `$VISUAL`/`$EDITOR` and re-validates after save (reopen on error); `rm` asks for confirmation unless `--yes`, removes config + states + recovery file. (Container removal joins in M3.)
  - Notes: `edit` works on a draft next to `context.toml` (original replaced atomically only when `context::validate` passes: syntax, name = directory, ADR 0009 mounts); fallback editor `vi`, run via `sh -c` like git. `rm` deletes the three per-context dirs; no input on stdin = no.
- [x] **M1.6 `koje state list|cp|mv|rm`**
  - Done when: list shows name, window/layout counts, modified time and marks role states; cp/mv/rm follow ADR 0005 (`empty` reserved, roles cannot be removed while configured? → decide); CLI tests.
  - Notes: ADR 0013. New `states` module (file ops incl. `read`/`write` from M1.3). Fixed `Display` of names/colors ignoring width (`f.pad`). Running contexts are not checked yet – revisit when `kojed` owns states (M5).
- [x] **M1.7 `koje list` with more detail**
  - Done when: shows prefix, color swatch (truecolor when on a TTY), number of states; machine-readable `--json`.
  - Notes: swatch only on a TTY and without `NO_COLOR`. A broken `context.toml` is listed with its error instead of failing. JSON fields (`name`, `prefix`, `color`, `image`, `mounts`, `states`, `error`) are an interface: only add; `status` joins in M3.
- [x] **M1.8 Layout model operations**
  - Done when: pure functions for `layout new [--copy]`, `mv`, `rm` (cannot remove active layout), `autosave on|off`, next/prev order, "window closed → remove from all layouts", "new window → join active layout"; unit tests.
  - Notes: `layouts` module; also `save_active` (`layout save`) and `window_moved` (autosave). `layout new` makes the new layout active (concept: "all windows start in the stash"). Order per ADR 0011.
- [x] **M1.9 State switch planning**
  - Done when: pure function computing keep/close/start sets when switching states in a running context (concept "Zustand wechseln"); unit tests.
  - Notes: `state::plan_state_switch`; matches windows by id (refine in M5.10). Added `koje up --no-save` (concept had it, CLI did not); docs + ADR 0005 synced. Same id with a different launch spec counts as "keep" for now.
- [x] **M1.10 Recovery file model**
  - Done when: write/read/delete helpers, atomic; "recovery present" detection; unit tests. (Writing it continuously is M5.)
  - Notes: `recovery` module; state-file format, time = mtime (ADR 0006 clarified). `recovery::decide` maps presence + `--recover`/`--no-recover` to None/Ask/Restore/Discard for `up` (M3.4/M5).
- [x] **M1.11 Refuse dangerous mount targets** (ADR 0009)
  - Done when: `mounts` accepts string and `{ path, unsafe }` table form (round-trips both); the three rule groups of ADR 0009 are refused unless `unsafe = true`; protected paths come from `Paths`/XDG, not hard-coded home paths; tests per rule.
  - Notes: `Paths::host_dirs()` collects home + protected dirs; `ContextConfig::expand_mounts(&HostDirs)`. `mount::Mount` has a hand-written `Deserialize` so path errors keep their reason.
- [x] **M1.12 Access profiles in config** (ADR 0017)
  - Done when: `[access]` in `config.toml` and `context.toml` parses (profile + switches, `deny_unknown_fields`); `ContextConfig::effective_access(&GlobalConfig)` follows the resolution rule; profile table as data; `koje new` template mentions it; tests per rule.
  - Notes: `access` module (`Profile::access()` is the table, `access::resolve` the rule). Profile `none` is `Profile::NoAccess` in Rust. Not shown in `koje list` yet.
