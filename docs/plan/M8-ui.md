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
  - Follow-up 2026-10-26: Super+Return opened two terminals on the host – most likely an own `bind = $mod, Return, …` left in `hyprland.conf` from the M2.1 starter config. `koje doctor` now lists key combinations the running Hyprland binds more than once (`doctor::duplicate_bindings` on `j/binds`).
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
- [ ] **M8.26 OSD with GTK4** (ADR 0041, 0042, 0043): a layer-shell OSD for volume and brightness instead of the notification, looking and placed like GNOME's
  - Notes: was planned with Kiesel Desktop; GTK4 since ADR 0042. Builds on M8.27.

## Own bar (ADR 0042)

Replaces Waybar at once: the session switches in M8.32, when the bar covers
everything koje's Waybar config shows (M8.27–M8.31). M8.33–M8.35 are what
Waybar cannot do. The bar is laid out like GNOME's top bar and uses libadwaita
(ADR 0043).

- [x] **M8.27 Shell and bar skeleton**: crate `koje-shell`, one GTK application (GTK4, gtk4-layer-shell, gtk4-rs) as a systemd user service with restart on failure; libadwaita (ADR 0043); one bar per monitor with hotplug, exclusive zone, symbolic icons, light and dark; client of kojed (ADR 0027) with live updates and of Hyprland's event socket, all asynchronous with timeouts
  - Notes: check the GTK4, libadwaita (1.6 or later) and gtk4-layer-shell packages on Ubuntu 26.04 and add the dev packages to the dev container. What the bar shows and what clicks do are pure functions with unit tests.
  - `crates/koje-shell`: `adw::Application` `io.koje.Shell` (one instance through its bus name), a layer-shell bar per monitor (`model::reconcile` on the monitor list), `model::Shell` turns updates into a `BarView`; kojed (`RunningContexts`, `ContextsChanged`, its bus name's owner) and Hyprland's events (`Hyprland::active_workspace` on connect) on a tokio thread, sent to GTK over an `async-channel`, 2 s timeouts. The skeleton shows the focused workspace, the active context and "kojed is not running". `assets/systemd/koje-shell.service` is not installed yet (M8.32); `just shell` runs it. Versions: Debian trixie (GTK 4.18, libadwaita 1.7, gtk4-layer-shell 1.0) in the dev container, Ubuntu 26.04 (4.22, 1.9, 1.3) on the host and in CI (`ubuntu-26.04`); gtk4-rs 0.11 raised the MSRV to 1.92.
  - Smoke-tested in a headless sway in the dev container: the bar reserves 32 px, libadwaita's light style, a second output gets its own bar and loses it on unplug. To check on the host (`just shell` while Waybar runs): the workspace name follows Hyprland, the active context follows focus, the warning appears when kojed stops and goes when it starts, dark style with `color-scheme` `prefer-dark`.
- [x] **M8.28 Workspaces and task list**: workspaces by name; the current workspace's windows with icon and title, click focuses and raises; the context color as accent of the active workspace and the focused window (replaces M8.20's CSS file)
  - Per monitor (GDK connector = Hyprland monitor name): its workspaces, numbered first, then named; specials hidden. The backend follows Hyprland's raw event lines (`Events::next_line`, `hypr::changes_desktop`) and after each burst (20 ms quiet) reads `j/monitors`, `j/workspaces`, `j/clients`, `j/activewindow` again – no copy that drifts. Clicks are `model::Action`s sent to the backend, which applies `FocusWorkspace`/`FocusWindow` (focus and raise). Accents: `Shell::accent_css` gives every running context a class setting libadwaita's accent variables (text black or white by luminance), host workspaces libadwaita's slate (M8.20's grey); the active workspace is a filled pill, the focused task underlined. Icons from the window class's `.desktop` entry or icon theme, else a generic one – context programs mostly get the generic one until M8.18. M8.20's CSS file goes with Waybar in M8.32.
  - Follow-up: running contexts Hyprland has no workspace for (empty, or asleep with their windows in the stash – ADR 0028) show dimmed among the named workspaces on the focused monitor's bar; a click asks kojed to focus the context, and Hyprland creates the workspace.
  - Fixed after the host test: focusing a window reordered the task list – Hyprland lists windows in stacking order, which every raise changes; the shell now keeps the order in which windows first appeared (`Shell::order`).
  - Smoke-tested in headless sway with a fake Hyprland and a fake kojed: order, active pill and underline in the context color, light and dark. Clicks could not be tested there (no pointer device). To check on the host: clicks switch workspaces and focus and raise windows; the list follows opening, closing, moving windows and switching workspaces without lag; two monitors.
- [x] **M8.29 Active context and memory**: the active context's capsule in its color; the memory meter as a real widget (`koje_core::memory::meter`, levels and hover details as M8.21)
  - On the right of the bar (`capsule.rs`): the active context's name as a pill in its accent, a drawn meter (64 px; free part in the text color, used part in the level's color from GNOME's palette – green, yellow from 90 % of the soft limit, orange over it, red at the hard one – the soft limit as a tick when there is a hard one too), the percentage of the limit, `koje_core::memory::details` on hover (also used by `koje bar --memory` now). Without limits only the size; without an active context nothing. kojed's `Memory("")` on `ContextsChanged`, `MemoryChanged` and a new owner; a reading of another context (before a switch) is not shown.
  - Smoke-tested in headless sway with fake Hyprland and kojed, light and dark. To check on the host: the meter follows the 5 s readings; the capsule follows focus between contexts and disappears on host workspaces. The active context showed twice (workspace pill and capsule): the capsule becomes the koje menu (M8.55).
- [ ] **M8.30 Clock and Quick Settings** (ADR 0043): the clock in the middle with a calendar popover; a Quick Settings button as GNOME's – volume (PipeWire), network (NetworkManager), battery (UPower), brightness, keyboard layout, dark style; where CPU and memory go (Waybar shows them today)
- [ ] **M8.31 Tray**: StatusNotifierWatcher and host, icons, menus from DBusMenu as GTK4 menus
- [ ] **M8.32 Switch the session to koje's bar**: the snippet starts it instead of Waybar; remove koje's Waybar config, the context colors CSS (M8.20), `scripts/build-waybar-deb.sh` and Waybar in `koje setup`, `doctor` and the installer (a user's own Waybar config stays untouched); decide what stays of `koje bar`; update ADR 0021/0037 references, `docs/architecture.md` and `AGENTS.md`
  - Notes: only after M8.28–M8.31; host test of a full session before. The package ships `koje-shell` and its unit (ADR 0045, M0.9); the session starts it.
- [x] **M8.55 koje menu** (decided 2026-10-26, replaces M8.29's capsule): a menu button on the right of the bar with a symbolic koje icon in the context's accent, the memory meter and its percentage. In a context its dropdown shows the context: memory with a graph of the last ten minutes and the limits, soft and hard limit editable (applied with `koje limit`, errors shown), "Context settings…" once the settings app exists (M8.9, M8.15). On a host workspace the icon is grey and the dropdown lists the running contexts with their meters; a click focuses one. More context information goes into the button and the dropdown later (M8.35).
  - kojed keeps ten minutes of readings per running context (`koje_core::memory::History`, D-Bus `MemoryHistory`).
  - Shell: `menu.rs` (pure: button, context panel, overview, graph scale, `limit_arg` checks what is typed before `koje limit` runs), `menu_ui.rs` (widgets). The backend reads `Memory` for every running context and the active one's `MemoryHistory` on every `MemoryChanged`; `Action::SetLimits` runs `koje limit <context> --soft … --hard …` (its error shown under the fields), `Action::FocusContext` calls kojed's `Focus`. The entries change only when the context or its limits change, not under typing. The icon (`assets/icons/io.koje.Koje-symbolic.svg`, installed by the package) is drawn by the bar itself in the accent: GTK 4.18 with librsvg 2.60 (Debian trixie) does not recolor symbolic SVGs from files – only its built-in icons; check on the host's GTK 4.22 before using the SVG elsewhere.
  - Smoke-tested in headless sway with fake Hyprland and kojed (menu opened by a temporary patch): context panel with graph, soft (dashed) and hard line, limit fields; overview on a host workspace; light and dark. To check on the host: changing a limit in the menu (and an invalid one), clicking a context in the overview, the graph after ten minutes. "Context settings…" comes with M8.15.
- [x] **M8.56 Contexts in the koje menu** (decided 2026-10-26): on a host workspace the koje menu manages contexts – every context with its status; running ones can be paused or woken, stopped and restarted (stop and restart confirmed in place), stopped ones started; "New context…" with a name and a color (else `koje new`'s default), started right away. Actions run the CLI (`koje new`, `up`, `down`, `sleep`, `wake`); a context shows that it is busy, and errors appear in its row.
  - kojed: D-Bus `Contexts()` – every context with color and status (`running`, `paused`, `stopped`); which are paused comes from the runtime with the running set and is kept by `Hide`/`Show` (`koje sleep`/`wake`). `Reload`, `Hide` and `Show` emit `ContextsChanged`.
  - Shell: `menu::Overview` (every context, running and paused ones first; busy and error per context; `context_name` checks a new name against core's rules and the existing ones), `overview_ui.rs` (rows with pause/wake, restart, stop, start; stop and restart armed by a first click, done by a second within 4 s; "New context…" with name, "Auto" or one of libadwaita's accent colors). `Action::koje` maps each action to its CLI commands (restart = `down` + `up`, create = `new` + `up`); the backend runs each in its own task (5 min timeout) and reports `Busy`, then `Done` with the CLI's error. Without a terminal `koje down` does not stop a context whose windows stay open (a program asking to save) – the row shows why; `koje up` keeps a recovery file for later.
  - Smoke-tested in headless sway with fake Hyprland and kojed (menu and form opened by a temporary patch), light and dark. To check on the host: each action on real contexts, a stop with an unsaved document open, creating a context with and without a color.
- [ ] **M8.33 Move windows by dragging in the bar**: drag a window from the task list onto a workspace button, it moves there (`movetoworkspace`); drops only inside the bar
- [ ] **M8.34 Active overlay in the bar**: which overlay (`mail`, `chat`) is open (Hyprland's `activespecial`); click opens or closes it
- [ ] **M8.35 Contexts and layouts in the bar**: the active context's layouts to switch, stashed windows as icons (click brings one back), a menu per context (start, pause, wake, stop, open the settings app, M8.15)
- [ ] **M8.36 Notifications in the shell**: the shell is the notification server (`org.freedesktop.Notifications`) instead of mako; banners and the list in the clock's popover as GNOME's, in their context's color (ADR 0042, 0043)
  - Notes: find out how the shell tells a notification's context – each context's forwarder has its own connection to the host's bus (ADR 0019).
- [ ] **M8.38 GNOME look for the session** (ADR 0043): Adwaita icon and cursor themes, GNOME's fonts, light and dark through `org.gnome.desktop.interface color-scheme`; no gaps, rounding and shadows close to GNOME's; maximize below the bar; hyprlock like GNOME's lock screen; `koje doctor` checks the packages
  - Notes: decide how `koje setup` sets GNOME's settings without overwriting the user's own (ADR 0021).
- [ ] **M8.39 Title bars like Adwaita header bars** (ADR 0039, 0043): hyprbars' height, colors, centered title and GNOME's buttons
- [ ] **M8.40 Launcher, window picker and shortcuts list in the shell** (ADR 0043): GNOME-style replacements for the fuzzel menus (M8.17, Super+Tab, Super+F1); the application grid is M8.18
- [ ] **M8.41 GNOME's key bindings** (ADR 0043): Super alone opens and closes the launcher (koje's application menu for now; on release only, not after Super+key or Super+drag); Super+Up maximizes, Super+Down restores the previous size and place, Super+Left/Right put the window on the left or right half of the monitor below the bar
  - Notes: check on the host that Hyprland's release binding (`bindr` on `Super_L`) does not fire after a combination or a Super+drag. The halves may be the first snap zones (M7.3); restoring needs the window's geometry before maximize or half, remembered by koje.


## GNOME integration (ADR 0044)

GNOME Settings is the system's settings; kojed applies GNOME's gsettings to
Hyprland; the shell takes over the rest of GNOME Shell's duties.

- [ ] **M8.42 GNOME Settings under Hyprland** *(host)*: under which `XDG_CURRENT_DESKTOP` GNOME Settings starts (e.g. `Hyprland:GNOME`); what that value changes in apps (tray icons, Qt themes) and portals; which panels work, which have no effect; `xdg-desktop-portal-gnome` or `-gtk`; whether GNOME Settings asks for Wi-Fi passwords while open
  - Done when: a report in `docs/spikes/` with a table per panel; the `XDG_CURRENT_DESKTOP` value for koje's session.
- [ ] **M8.43 gsettings bridge: input**: kojed applies mouse and touchpad (`org.gnome.desktop.peripherals.*`) and keyboard layouts and options (`org.gnome.desktop.input-sources`) to Hyprland – at start, on change, after a config reload; the mapping is a pure function in `koje-core`; `koje setup` no longer writes the keyboard into `hyprland.conf`
  - Notes: decide how kojed reads gsettings and watches changes (GIO or dconf directly; kojed runs on tokio without a GLib loop, ADR 0026). Depends on M8.42.
- [ ] **M8.44 gsettings bridge: idle, power, night light, background**: idle, blank and suspend times (`org.gnome.desktop.session`, `org.gnome.settings-daemon.plugins.power`) for hypridle; night light (`…plugins.color`) through hyprsunset; the background (`org.gnome.desktop.background`) through a wallpaper program
- [ ] **M8.45 Network secret agent in the shell**: asks for Wi-Fi and VPN passwords like GNOME Shell (NetworkManager `SecretAgent`)
  - Notes: urgent – nothing asks today unless M8.42 finds that GNOME Settings does.
- [ ] **M8.46 Polkit agent in the shell**: the authentication dialog like GNOME Shell's, replacing polkit-gnome in the session
- [ ] **M8.47 End session dialog**: log out, restart, shut down with a countdown like GNOME's; Super+Shift+E opens it instead of leaving Hyprland at once
- [ ] **M8.48 Screenshot UI like GNOME's**: area, window or screen, to the clipboard and a file; screen recording; through `xdg-desktop-portal-hyprland` or Hyprland's protocols; Print opens it
- [ ] **M8.49 Alt+Tab like GNOME's**: windows grouped by app, Alt+` within one app, in the shell
- [ ] **M8.50 GNOME Shell's data in the shell**: favorites (`org.gnome.shell favorite-apps`), app folders (`org.gnome.desktop.app-folders`) and the apps' search providers (`org.gnome.Shell.SearchProvider2`) in the launcher (M8.40, M8.18); notification settings per app and "Do Not Disturb" (`org.gnome.desktop.notifications`) for M8.36

- [ ] **M8.51 Mutter's display API in kojed** (ADR 0046): `org.gnome.Mutter.DisplayConfig` from `hyprctl monitors all` – `GetCurrentState`, `MonitorsChanged`, `ApplyMonitorsConfig` (verify, temporary, persistent) applied to Hyprland; the mapping between Mutter's and Hyprland's monitors, modes, scales and transforms pure in `koje-core`
  - Notes: after the bar and M8.42; check which version of the API the host's GNOME Settings speaks.
- [ ] **M8.52 `monitors.xml` and the generated monitor config** (ADR 0046): read and write `~/.config/monitors.xml` in Mutter's format (configurations per monitor set); generate `$XDG_CONFIG_HOME/koje/hyprland/monitors.conf` for the connected set, sourced by koje's snippet; again on changes of `monitors.xml` and on hotplug; `koje setup` stops writing a `monitor =` line
- [ ] **M8.53 "Keep these display settings?" in the shell** (ADR 0046): GNOME Shell's countdown dialog after a display change; reverts unless confirmed
- [ ] **M8.54 Monitors in `koje doctor`** (ADR 0046): warn when a screen runs below its best refresh rate at its resolution, and when an own `monitor` line in `hyprland.conf` overrides the generated file

## Settings app

- [x] **M8.3 Choose panel toolkit** (iced, egui, Slint) → ADR
  - Kiesel Desktop, for a settings app instead of a panel overlay (ADR 0036). Changed to GTK4 (ADR 0042).
- [-] **M8.4 Panel prototype** (layer-shell overlay): contexts, states, layouts matrix, memory
  - Dropped (ADR 0036): replaced by the settings app, M8.9–M8.14.
- [-] **M8.5 Layout overview** (Super+L): thumbnails, drag windows between layouts
  - Dropped (ADR 0036): moved into the settings app's layouts page (M8.12).
  - Notes: until then (M2.1): Waybar's task list (the current workspace's windows: `hyprland/workspaces` in taskbar mode, `active-only`), Alt+Tab, Super+Tab picker (`assets/bin/koje-window-picker`).
- [ ] **M8.9 Settings app skeleton** (`koje-shell settings`): a libadwaita window like GNOME's apps, in its own process (ADR 0042, 0043); client of kojed (ADR 0027) with live updates; context list with color and status; start, stop, pause, wake per context; one instance (a second start brings the window up, optionally on a given context)
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
  - Notes: built with GTK4 (ADR 0042). May be dropped – koje-shell's bar shows empty running contexts (M8.28) and the koje menu carries the controls (M8.55, M8.56); decide before starting it.
- [ ] **M8.37 Disk space in the settings app** (ADR 0040): reclaimable space in total and per context and image (old containers with their changes outside the home, unused image versions); one button reclaims everything, single items removable
