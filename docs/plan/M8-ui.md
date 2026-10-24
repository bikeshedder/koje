# M8 – UI

koje's UI has two parts (ADR 0036): a **settings app** for everything that is
viewed and changed, and the **bar** (plus fuzzel pickers) for glancing and
switching. Window overlays and the beacon window are small extra surfaces. All
of them are built with GTK4 in the koje shell (`koje-shell`); the bar is koje's
own and replaces Waybar (ADR 0042). See the mockups in the concept (sections
"Layouts" and "Oberfläche"); the panel mockups now describe pages of the
settings app. What the settings app needs:
`docs/upstream/kiesel-desktop-components.md` (written for Kiesel Desktop, still
valid as a list).

## Launcher and bar

- [ ] **M8.1 Launcher menu** (fuzzel/rofi dmenu mode): contexts with status, actions per context
- [-] **M8.2 Waybar module**: active context capsule, layouts, stash icons, memory; click actions
  - Dropped (ADR 0042): koje's own bar does this, M8.29 and M8.35.
  - Notes: icons from the bundled Lucide font (ADR 0020), looked up by name in `codepoints.json`; ship a complete bar configuration, not only the module. Started with `koje bar` (M5.15). What a custom module cannot do opens the settings app (M8.15) or a picker.
- [x] **M8.16 Session in koje's Hyprland snippet** (ADR 0037): Waybar, mako, hypridle/hyprlock, polkit agent, basic bindings, workspaces 1–9, look; `koje terminal` for Super+Return; `koje doctor` checks the session packages; `koje setup` reloads Hyprland and Waybar and starts missing session services, and writes the whole `hyprland.conf` (system keyboard plus `us`) when the user has none of their own
  - Notes: to check on the host: Super+Return on a context workspace (terminal in the context) and on a numbered one (host terminal); a `hyprland.conf` with the generated default config doubles Super+Q.
- [x] **M8.17 Application menu** (Super+D): the programs of the active context – its `.desktop` entries (base image plus what the user installed in it), read from the running container – in fuzzel; Enter starts the picked one there. On a workspace without a context: the running contexts (context picker).
  - `koje apps [-c] [--start <name>]`, parsing in `koje_core::apps`, `assets/bin/koje-app-menu`. Runs the entry's `Exec` itself (`koje_core::apps::exec_command`; `Terminal=true` in foot): `gio launch` opened nothing for Nautilus (D-Bus activatable) on the host. The menu's prompt and the context list show memory use against the hard (else soft) limit (`koje apps --menu`, `koje bar --names --memory`) – a stopgap until M8.18 / the settings app's memory page (M8.14).
- [ ] **M8.18 Application menu, later steps**: icons (fuzzel finds them only in host icon themes – copy or point at the context's), programs of all running contexts in one list in their colors, stopped contexts to start, host programs; a full-screen grid like GNOME's in GTK4 (ADR 0036, 0042)
- [x] **M8.19 Focus blip**: when a context window gets focus, its border flashes white for 200 ms, then shows the context's color again – feedback where focus went (Alt+Tab, task list, click)
  - kojed on `activewindowv2` (`windows::blip`/`border`, `Daemon::unblip` from the event loop). Host windows do not blip (no koje color to return to). To check on the host: how it looks with Hyprland's border color animation; whether 200 ms is right.
- [x] **M8.20 Task list in the context colors**: the context's color is the bar's accent – the underline of the focused window in the task list and of the active workspace in the workspace list
  - kojed writes `~/.config/waybar/koje-contexts.css` (`setup::waybar_colors`) when context colors change, only next to koje's own Waybar style; `style.css` imports it, Waybar reloads it by itself (`reload_style_on_change`). Needs Waybar after 0.15.0 (workspace name as CSS class on the button, `setup::waybar_class` mirrors its rule). `koje setup` creates the file when missing (Waybar refuses a missing import).
  - Fixed after the host test: koje's rule for the task list's workspace button also hit the task buttons (`border-bottom: none` removed the focused task's underline) – now `:not(.taskbar-window)`; inactive tasks are no longer underlined.
- [x] **M8.21 Memory in the bar**: the active context's memory as a meter (`koje bar --memory`, custom module) – used against the hard (else soft) limit as a line of 16 box-drawing cells in half steps (thick colored used part on a thin grey track, the soft limit as a crossing), colored by level – green, yellow from 90 % of the soft limit, orange over it, red at the hard one; the percentage of the limit to the right; used, charged and both limits with their percentages on hover
  - kojed keeps the last reading per running context (every tick, also without limits), D-Bus `Memory` and signal `MemoryChanged`; `koje_core::memory::meter` does the math. Without limits the module shows only the size; without an active context nothing.
- [x] **M8.19 Keyboard shortcuts list** (Super+F1): every binding of the running Hyprland (`hyprctl -j binds`) in fuzzel with its description – koje's and the user's own; Super+Shift+F maximizes
  - All of koje's bindings are `bindd`/`bindmd` (a test keeps it so); bindings without a description show their command. `assets/bin/koje-keys`. To check on the host: Hyprland 0.53 accepts `bindmd` and `fullscreen, 1`.
- [x] **M8.22 Title bars** (ADR 0039): hyprbars built by `scripts/setup-hyprbars.sh` against `hyprland-dev` (not hyprpm: Ubuntu's never records its headers, `add` fails), loaded with `plugin =`; koje's settings in a second snippet `hyprbars.conf`, no bar for apps with their own header bar
  - Option names and the `hyprbars:no_bar` rule checked against the plugin version pinned for Hyprland 0.53 (hyprland-plugins 64b7c2d). To check on the host: the script builds on Ubuntu 26.04; bars on foot and Firefox, none on Nautilus; dragging a bar moves the window.
- [ ] **M8.23 Title bars in the context color**: a `hyprbars:bar_color` rule per context tag from kojed – only when the plugin is loaded (`hyprctl plugin list`), else Hyprland reports an unknown rule
- [x] **M8.24 Volume and brightness keys with an OSD** (ADR 0041): `koje media volume-up|volume-down|mute|mic-mute|brightness-up|brightness-down`; volume with `wpctl`, brightness through logind's `SetBrightness` (no group `video`); the level as a replacing notification with a progress value (mako)
  - `koje_core::media` (wpctl parsing, backlight choice, perceptual steps, OSD texts), `crates/koje/src/media.rs`. Host-tested 2026-10-24: brightness down/up through logind without the group `video`, volume, mute and microphone; mako 1.10 crashes on `replaces_id` together with the synchronous hint, so only the hint replaces the OSD. `koje doctor` checks `wpctl`.
- [~] **M8.25 Media keys to the most recently active player** (ADR 0041): `koje media play-pause|next|previous`; kojed watches MPRIS `PlaybackStatus` on the host's bus and every running context's bus (`/proc/<pid>/root/run/user/<uid>/bus`), D-Bus method `MediaPlayer` for `koje media`
  - `kojed/src/players.rs`, `koje_core::media::player_order`, `Podman::running_pid`. Connecting to a context's bus from the host checked 2026-10-24 (`busctl --address`). To check on the host with the new kojed: play/pause in a context's Firefox and on the host; after pausing in the app the key resumes that player; a context started after kojed is reached within 5 s.
- [ ] **M8.26 OSD with GTK4** (ADR 0041, 0042): a layer-shell OSD for volume and brightness instead of the notification
  - Notes: was planned with Kiesel Desktop; GTK4 since ADR 0042. Builds on M8.27.

## Own bar (ADR 0042)

Replaces Waybar at once: the session switches in M8.32, when the bar covers
everything koje's Waybar config shows (M8.27–M8.31). M8.33–M8.35 are what
Waybar cannot do.

- [ ] **M8.27 Shell and bar skeleton**: crate `koje-shell`, one GTK application (GTK4, gtk4-layer-shell, gtk4-rs) as a systemd user service with restart on failure; one bar per monitor with hotplug, exclusive zone, Lucide font (ADR 0020), light and dark; client of kojed (ADR 0027) with live updates and of Hyprland's event socket, all asynchronous with timeouts
  - Notes: settle first (open question): libadwaita or plain GTK4 with koje's CSS. Check the GTK4 and gtk4-layer-shell packages on Ubuntu 26.04 and add the dev packages to the dev container. What the bar shows and what clicks do are pure functions with unit tests.
- [ ] **M8.28 Workspaces and task list**: workspaces by name; the current workspace's windows with icon and title, click focuses and raises; the context color as accent of the active workspace and the focused window (replaces M8.20's CSS file)
- [ ] **M8.29 Active context and memory**: the active context's capsule in its color; the memory meter as a real widget (`koje_core::memory::meter`, levels and hover details as M8.21)
- [ ] **M8.30 System modules**: clock with calendar, keyboard layout (click: next), CPU, memory, volume (PipeWire; click: pavucontrol), network (NetworkManager), battery (UPower)
- [ ] **M8.31 Tray**: StatusNotifierWatcher and host, icons, menus from DBusMenu as GTK4 menus
- [ ] **M8.32 Switch the session to koje's bar**: the snippet starts it instead of Waybar; remove koje's Waybar config, the context colors CSS (M8.20), `scripts/build-waybar-deb.sh` and Waybar in `koje setup`, `doctor` and the installer (a user's own Waybar config stays untouched); decide what stays of `koje bar`; update ADR 0021/0037 references, `docs/architecture.md` and `AGENTS.md`
  - Notes: only after M8.28–M8.31; host test of a full session before.
- [ ] **M8.33 Move windows by dragging in the bar**: drag a window from the task list onto a workspace button, it moves there (`movetoworkspace`); drops only inside the bar
- [ ] **M8.34 Active overlay in the bar**: which overlay (`mail`, `chat`) is open (Hyprland's `activespecial`); click opens or closes it
- [ ] **M8.35 Contexts and layouts in the bar**: the active context's layouts to switch, stashed windows as icons (click brings one back), a menu per context (start, pause, wake, stop, open the settings app, M8.15)
- [ ] **M8.36 Notifications in the shell**: the shell is the notification server (`org.freedesktop.Notifications`) instead of mako; notifications in their context's color, a list in the bar (ADR 0042)
  - Notes: find out how the shell tells a notification's context – each context's forwarder has its own connection to the host's bus (ADR 0019).

## Settings app

- [x] **M8.3 Choose panel toolkit** (iced, egui, Slint) → ADR
  - Kiesel Desktop, for a settings app instead of a panel overlay (ADR 0036). Changed to GTK4 (ADR 0042).
- [-] **M8.4 Panel prototype** (layer-shell overlay): contexts, states, layouts matrix, memory
  - Dropped (ADR 0036): replaced by the settings app, M8.9–M8.14.
- [-] **M8.5 Layout overview** (Super+L): thumbnails, drag windows between layouts
  - Dropped (ADR 0036): moved into the settings app's layouts page (M8.12).
  - Notes: until then (M2.1): Waybar's task list (the current workspace's windows: `hyprland/workspaces` in taskbar mode, `active-only`), Alt+Tab, Super+Tab picker (`assets/bin/koje-window-picker`).
- [ ] **M8.9 Settings app skeleton** (`koje-shell settings`): a normal GTK4 window in its own process (ADR 0042); client of kojed (ADR 0027) with live updates; context list with color and status; start, stop, pause, wake per context; one instance (a second start brings the window up, optionally on a given context)
  - Notes: shares the shell's code and look (M8.27).
- [ ] **M8.10 Contexts page**: create and edit a context – name, color, access profile and overrides, mounts, memory limits; recreate with confirmation
  - Notes: writes `context.toml` through the same core functions as `koje new`/`edit`; validation errors from core shown per field.
- [ ] **M8.11 States page**: states of a context with a thumbnail of the active layout; load, copy, rename, delete; roles `default` and `fresh`
- [ ] **M8.12 Layouts page**: the windows × layouts matrix (visible / stashed), "Save automatically" per layout, new and renamed layouts, double-click switches; layout thumbnails with dragging windows between them (from M8.5)
- [ ] **M8.13 Windows page**: a context's windows with their IDs (renamable) and integration
- [ ] **M8.14 Memory page**: all contexts against their soft and hard limits; change limits
- [ ] **M8.15 Open the settings app from the bar and the CLI**: click on the context capsule or a `koje` command opens the app on that context

## Extra surfaces

- [ ] **M8.6 Overlay on windows of sleeping contexts** (ADR 0024): "<context> is asleep – Wake up" on every window of a paused context (layer-shell, positioned from kojed's window list); click wakes the context
  - Notes: built with GTK4 and gtk4-layer-shell (ADR 0042); placing a surface over a window.
- [ ] **M8.7 Overlay for hanging windows** (ADR 0024): same overlay with "not responding – Wait / Quit", once Hyprland reports unresponsive windows (upstream request `docs/upstream/hyprland-anr-ipc.md`; fallback: plugin)
- [ ] **M8.8 Context beacon window** (decided 2026-10-21 as the solution for empty context workspaces, ADR 0028): a small host window per running context on its workspace – keeps the workspace alive (no persistent rules, no reloads) and carries the context's controls (down, sleep, layouts, stash, memory); closing it can mean `koje down`. An xdg toplevel (layer-shell surfaces belong to no workspace); kojed keeps it out of states, layouts, task bar, Alt+Tab and the window picker, and places it (pinned strip at an edge?). Until then `koje bar` (M5.15).
  - Notes: built with GTK4 (ADR 0042). May be dropped – `koje bar` already shows running contexts; decide before starting it.
