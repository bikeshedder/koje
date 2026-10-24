# M8 – UI

koje's UI has two parts (ADR 0036): a **settings app** built with Kiesel Desktop
for everything that is viewed and changed, and the **bar** (Waybar with koje's
modules, plus fuzzel pickers) for glancing and switching. Window overlays and the
beacon window are small extra surfaces. See the mockups in the concept (sections
"Layouts" and "Oberfläche"); the panel mockups now describe pages of the settings
app. Component priorities for Kiesel Desktop: `docs/upstream/kiesel-desktop-components.md`.

## Launcher and bar

- [ ] **M8.1 Launcher menu** (fuzzel/rofi dmenu mode): contexts with status, actions per context
- [ ] **M8.2 Waybar module**: active context capsule, layouts, stash icons, memory; click actions
  - Notes: icons from the bundled Lucide font (ADR 0020), looked up by name in `codepoints.json`; ship a complete bar configuration, not only the module. Started with `koje bar` (M5.15). What a custom module cannot do opens the settings app (M8.15) or a picker.
- [x] **M8.16 Session in koje's Hyprland snippet** (ADR 0037): Waybar, mako, hypridle/hyprlock, polkit agent, basic bindings, workspaces 1–9, look; `koje terminal` for Super+Return; `koje doctor` checks the session packages; `koje setup` reloads Hyprland and Waybar and starts missing session services, and writes the whole `hyprland.conf` (system keyboard plus `us`) when the user has none of their own
  - Notes: to check on the host: Super+Return on a context workspace (terminal in the context) and on a numbered one (host terminal); a `hyprland.conf` with the generated default config doubles Super+Q.
- [x] **M8.17 Application menu** (Super+D): the programs of the active context – its `.desktop` entries (base image plus what the user installed in it), read from the running container – in fuzzel; Enter starts the picked one there. On a workspace without a context: the running contexts (context picker).
  - `koje apps [-c] [--start <name>]`, parsing in `koje_core::apps`, `assets/bin/koje-app-menu`. Runs the entry's `Exec` itself (`koje_core::apps::exec_command`; `Terminal=true` in foot): `gio launch` opened nothing for Nautilus (D-Bus activatable) on the host. The menu's prompt and the context list show memory use against the hard (else soft) limit (`koje apps --menu`, `koje bar --names --memory`) – a stopgap until M8.18 / the settings app's memory page (M8.14).
- [ ] **M8.18 Application menu, later steps**: icons (fuzzel finds them only in host icon themes – copy or point at the context's), programs of all running contexts in one list in their colors, stopped contexts to start, host programs; a full-screen grid like GNOME's in Kiesel Desktop (ADR 0036)
- [x] **M8.19 Focus blip**: when a context window gets focus, its border flashes white for 200 ms, then shows the context's color again – feedback where focus went (Alt+Tab, task list, click)
  - kojed on `activewindowv2` (`windows::blip`/`border`, `Daemon::unblip` from the event loop). Host windows do not blip (no koje color to return to). To check on the host: how it looks with Hyprland's border color animation; whether 200 ms is right.
- [x] **M8.20 Task list in the context colors**: the context's color is the bar's accent – the underline of the focused window in the task list and of the active workspace in the workspace list
  - kojed writes `~/.config/waybar/koje-contexts.css` (`setup::waybar_colors`) when context colors change, only next to koje's own Waybar style; `style.css` imports it, Waybar reloads it by itself (`reload_style_on_change`). Needs Waybar after 0.15.0 (workspace name as CSS class on the button, `setup::waybar_class` mirrors its rule). `koje setup` creates the file when missing (Waybar refuses a missing import).
  - Fixed after the host test: koje's rule for the task list's workspace button also hit the task buttons (`border-bottom: none` removed the focused task's underline) – now `:not(.taskbar-window)`; inactive tasks are no longer underlined.
- [x] **M8.21 Memory in the bar**: the active context's memory as a meter (`koje bar --memory`, custom module) – used against the hard (else soft) limit in 10 cells of eighth blocks, the part beyond the soft limit in a dim red track, amber over the soft limit, red at the hard one; the percentage of the limit to the right; used, charged and both limits with their percentages on hover
  - kojed keeps the last reading per running context (every tick, also without limits), D-Bus `Memory` and signal `MemoryChanged`; `koje_core::memory::meter` does the math. Without limits the module shows only the size; without an active context nothing.
- [x] **M8.19 Keyboard shortcuts list** (Super+F1): every binding of the running Hyprland (`hyprctl -j binds`) in fuzzel with its description – koje's and the user's own; Super+Shift+F maximizes
  - All of koje's bindings are `bindd`/`bindmd` (a test keeps it so); bindings without a description show their command. `assets/bin/koje-keys`. To check on the host: Hyprland 0.53 accepts `bindmd` and `fullscreen, 1`.

## Settings app

- [x] **M8.3 Choose panel toolkit** (iced, egui, Slint) → ADR
  - Kiesel Desktop, for a settings app instead of a panel overlay (ADR 0036).
- [-] **M8.4 Panel prototype** (layer-shell overlay): contexts, states, layouts matrix, memory
  - Dropped (ADR 0036): replaced by the settings app, M8.9–M8.14.
- [-] **M8.5 Layout overview** (Super+L): thumbnails, drag windows between layouts
  - Dropped (ADR 0036): moved into the settings app's layouts page (M8.12).
  - Notes: until then (M2.1): Waybar's task list (the current workspace's windows: `hyprland/workspaces` in taskbar mode, `active-only`), Alt+Tab, Super+Tab picker (`assets/bin/koje-window-picker`).
- [ ] **M8.9 Settings app skeleton** (`koje-gui`): a normal window with Kiesel Desktop; client of kojed (ADR 0027) with live updates; context list with color and status; start, stop, pause, wake per context; one instance (a second start brings the window up, optionally on a given context)
  - Notes: depends on Kiesel Desktop priority 1 (foundations, list, buttons, tabs).
- [ ] **M8.10 Contexts page**: create and edit a context – name, color, access profile and overrides, mounts, memory limits; recreate with confirmation
  - Notes: writes `context.toml` through the same core functions as `koje new`/`edit`; validation errors from core shown per field.
- [ ] **M8.11 States page**: states of a context with a thumbnail of the active layout; load, copy, rename, delete; roles `default` and `fresh`
- [ ] **M8.12 Layouts page**: the windows × layouts matrix (visible / stashed), "Save automatically" per layout, new and renamed layouts, double-click switches; layout thumbnails with dragging windows between them (from M8.5)
- [ ] **M8.13 Windows page**: a context's windows with their IDs (renamable) and integration
- [ ] **M8.14 Memory page**: all contexts against their soft and hard limits; change limits
- [ ] **M8.15 Open the settings app from the bar and the CLI**: click on the context capsule or a `koje` command opens the app on that context

## Extra surfaces

- [ ] **M8.6 Overlay on windows of sleeping contexts** (ADR 0024): "<context> is asleep – Wake up" on every window of a paused context (layer-shell, positioned from kojed's window list); click wakes the context
  - Notes: built with Kiesel Desktop (ADR 0036); needs layer-shell and placing a surface over a window there.
- [ ] **M8.7 Overlay for hanging windows** (ADR 0024): same overlay with "not responding – Wait / Quit", once Hyprland reports unresponsive windows (upstream request `docs/upstream/hyprland-anr-ipc.md`; fallback: plugin)
- [ ] **M8.8 Context beacon window** (decided 2026-10-21 as the solution for empty context workspaces, ADR 0028): a small host window per running context on its workspace – keeps the workspace alive (no persistent rules, no reloads) and carries the context's controls (down, sleep, layouts, stash, memory); closing it can mean `koje down`. An xdg toplevel (layer-shell surfaces belong to no workspace); kojed keeps it out of states, layouts, task bar, Alt+Tab and the window picker, and places it (pinned strip at an edge?). Until then `koje bar` (M5.15).
  - Notes: built with Kiesel Desktop (ADR 0036). May be dropped – `koje bar` already shows running contexts; decide before starting it.
