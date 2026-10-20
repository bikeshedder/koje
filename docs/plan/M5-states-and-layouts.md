# M5 – States & layouts

Goal: koje remembers and restores the work environment.

- [~] **M5.1 Capture**: running windows of a context → `State` (window IDs, commands, geometry)
  - Done in code (awaiting host test), ADR 0030: kojed's live state per context (`koje_core::capture::Live`; ids from integration or app id + counter), specs from `/proc` (`kojed::procfs`), D-Bus `State(context)`, `koje save <context> [-s <state>]` (the save half of M5.4).
- [~] **M5.2 Integrations**: file format, loader, built-ins `generic`, `terminal`, `editor`, `devcontainer`, `browser`
  - Done in code, ADR 0031: `koje_core::integration` (TOML, `detect` = `argv-last` / `cwd` / `shell-cwd`), built-ins terminal/editor/browser, user files in `~/.config/koje/integrations/`. Left: `devcontainer` (with dev container windows in states), launching (M5.3).
- [ ] **M5.3 Restore on `up`**: resolve state, launch windows, match new windows to IDs, place them
- [ ] **M5.4 `down` / `save` with `-s`**, ordered window close
- [ ] **M5.5 Layout switching** using `plan_switch`; stash as special workspace; focus handling
- [ ] **M5.6 `stash` / `unstash`**; new/closed windows update layouts
- [ ] **M5.7 Autosave** of geometry changes into the active layout (debounced)
  - Notes: M2.2 – no event for moving/resizing floating windows; re-read `j/clients` on focus changes / debounced and diff.
- [ ] **M5.8 Crash recovery**: continuous recovery file; `up` prompt, `--recover`/`--no-recover`
- [ ] **M5.9 State switching in a running context** (uses M1.9)
- [ ] **M5.10 Matching several windows of the same program** (open question → spike/ADR)
- [ ] **M5.11 Apply access changes to a running context** (from M3.13, ADR 0017): on `koje edit`/config change ask "now (save state, recreate, restore windows) or at next `up`"; note that recreating loses what was installed in the container; mounts without recreating depend on spike M2.12
