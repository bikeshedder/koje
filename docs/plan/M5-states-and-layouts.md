# M5 – States & layouts

Goal: koje remembers and restores the work environment.

- [x] **M5.1 Capture**: running windows of a context → `State` (window IDs, commands, geometry)
  - Done (host-tested 2026-10-20), ADR 0030: kojed's live state per context (`koje_core::capture::Live`; ids from integration or app id + counter), specs from `/proc` (`kojed::procfs`), D-Bus `State(context)`, `koje save <context> [-s <state>]` (the save half of M5.4).
- [x] **M5.2 Integrations**: file format, loader, built-ins `generic`, `terminal`, `editor`, `devcontainer`, `browser`
  - Done in code, ADR 0031: `koje_core::integration` (TOML, `detect` = `argv-last` / `cwd` / `shell-cwd`), built-ins terminal/editor/browser, user files in `~/.config/koje/integrations/`. Launching: M5.3. `devcontainer` with dev container windows in states moved to M9.1.
- [x] **M5.3 Restore on `up`**: resolve state, launch windows, match new windows to IDs, place them
  - Done (host-tested 2026-10-20: foot in its cwd, Firefox with 3 windows, nautilus, all in place): `koje up` resolves (requested → `fresh` → `empty`) and sends the state to kojed (`Restore`); `koje_core::restore::plan` (launch templates, `cwd`, `launch_once`), `ContainerRuntime::launch`, matching by app id in `Live::add`, placement from the active layout, 60 s expiry. Not yet: stash for windows outside the active layout (M5.5), dev container windows.
- [x] **M5.4 `down` / `save` with `-s`**, ordered window close
  - Done (host-tested 2026-10-20; Firefox 158 quits cleanly on SIGTERM, reopens its windows with "Open previous windows and tabs"): `koje down [-s]` saves first, then kojed ends the programs (`CloseWindows`: close windows, or SIGTERM per process for `quit = "signal"`), waits up to 10 s for the windows and the signalled processes (Firefox writes its session after closing its windows – first host test showed the restore prompt), asks before stopping if something stays; without kojed it stops directly (`--no-save`) or refuses (would lose the state).
- [ ] **M5.5 Layout switching** using `plan_switch`; stash as special workspace; focus handling
- [ ] **M5.6 `stash` / `unstash`**; new/closed windows update layouts
- [ ] **M5.7 Autosave** of geometry changes into the active layout (debounced)
  - Notes: M2.2 – no event for moving/resizing floating windows; re-read `j/clients` on focus changes / debounced and diff.
- [ ] **M5.8 Crash recovery**: continuous recovery file; `up` prompt, `--recover`/`--no-recover`
- [ ] **M5.9 State switching in a running context** (uses M1.9)
- [ ] **M5.10 Matching several windows of the same program** (open question → spike/ADR)
- [ ] **M5.11 Apply access changes to a running context** (from M3.13, ADR 0017): on `koje edit`/config change ask "now (save state, recreate, restore windows) or at next `up`"; note that recreating loses what was installed in the container; mounts without recreating depend on spike M2.12
- [x] **M5.12 File manager windows reopen in their folders** (wish, 2026-10-20): Nautilus runs one process for all windows, so `/proc` cannot tell a window's folder. Idea: a detect method that reads `org.freedesktop.FileManager1.OpenWindowsWithLocations` on the context's bus (`podman exec … dbus-send`), matches windows by title (= folder name), and launches `nautilus --new-window <folder>` per window; ADR 0031 update. First check on the host that the property lists every window's folder.
  - Done (host-tested 2026-10-20: two windows reopen in Home and Downloads, in place; only the active tab comes back): built-in `files`, detect `file-manager` (ADR 0031), `ContainerRuntime::query`; the property lists every tab, the title picks the active one.
- [-] **M5.13 Nautilus tabs** – moved to M9.2
- [~] **M5.14 `koje run <context> <program> [args…]`** (wish 2026-10-20): start a program in a running context without `podman exec` and container names; detached, no shell, in the current directory if the context mounts it
  - Done in code (awaiting host test); CLI test with fake podman.
- [~] **M5.15 A running context's workspace stays while empty** (wish 2026-10-20: it vanished from Waybar): kojed keeps it persistent (`keyword workspace name:<ctx>, persistent:true`) from `koje up` (D-Bus `Started`) to `koje down` (`Stopped`); re-applied after reloads, reconnects and kojed restarts (running contexts asked from Podman)
  - Done in code (awaiting host test); syntax also checked by the live test (M4.8).
