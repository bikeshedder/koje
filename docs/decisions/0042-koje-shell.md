# 0042. The koje shell: an own bar, and GTK4 for all of koje's UI

- Status: Accepted
- Date: 2026-10-24
- Source: discussion (Waybar's limits, the toolkit for an own bar, one program
  for all of koje's UI); revises ADR 0036, answers the open question "Own bar"

## Context

ADR 0036 kept the bar on Waybar with koje's custom modules and planned an own
bar only "when Waybar's limits get in the way". The settings app, window
overlays and beacon window were to be built with Kiesel Desktop.

The bar should now do things Waybar cannot:

- **Move windows to other workspaces** by dragging them in the bar (from the
  task list onto a workspace button).
- **Show which overlay is open** (`mail`, `chat`; Hyprland's `activespecial`
  event) and toggle it.
- **Manage contexts and layouts in the bar:** switch layouts, stashed windows,
  start, pause and stop contexts – menus and popovers with live data from kojed.

Waybar's custom modules only print text; koje already works around that
(M8.20 writes a CSS file for the context colors, M8.21 draws the memory meter
with box-drawing characters, M8.2 needs a Waybar newer than 0.15.0). Waybar's
CFFI modules can build real widgets, but only in GTK3 (the Rust bindings are
archived) and without dragging across modules.

An own bar is mostly platform work: layer-shell, popups anchored to a
layer-shell surface, drag and drop, several monitors, fractional scaling. Kiesel
Desktop has none of it yet; layer-shell was planned "later" there. GTK4 has all
of it (gtk4-layer-shell for the layer surface).

Bar, OSD, overlays and later notifications all need the same data (kojed,
Hyprland events) and the same look. Separate programs would each hold their own
connections and state. One process is simpler to build, but it is one failure
domain: Rust rules out memory errors in koje's own code, not panics (a double
`RefCell` borrow in a GTK callback aborts), crashes in GTK or drivers, Wayland
protocol errors (they close the whole connection) or a blocked main loop.

## Decision

- **koje gets its own bar**, a host program and a client of kojed (ADR 0027).
  It replaces Waybar.
- **The koje shell** (`koje-shell`, crate `crates/koje-shell`): one binary for
  all of koje's UI. Its long-running process is one GTK application with the
  bar (one per monitor), the OSD, the window overlays and later the
  notifications. It runs as a systemd user service that restarts on failure and
  keeps no state of its own – kojed holds it (ADR 0030) – so a crash costs a
  moment without a bar, nothing else.
- **The settings app is the same binary in its own process**
  (`koje-shell settings`, one instance): it is the largest UI and rarely open; a
  bug there must not take the bar down. The shell opens it on request.
- **Stays separate:** kojed (no GTK; limits and colors work without the shell),
  hyprlock as the lock screen, the fuzzel pickers for now.
- **Notifications later:** the shell becomes the notification server instead of
  mako – notifications in their context's color (each context reaches the
  host's bus through its own forwarder, ADR 0019) and a list in the bar. Not
  needed for the switch from Waybar; mako stays until then.
- **Nothing blocks the main loop:** D-Bus and Hyprland calls are asynchronous
  and have timeouts (a paused context's bus does not answer).
- **GTK4 for all of koje's UI:** the bar, the settings app, the window overlays
  (ADR 0024), the beacon window (ADR 0028) if it is built, and the OSD
  (ADR 0041). In Rust with gtk4-rs and gtk4-layer-shell. Kiesel Desktop is no
  longer used by koje.
- **Waybar is replaced at once:** the first version of the own bar that ships is
  the session's bar (ADR 0037); there is no phase with both. It therefore
  covers everything koje's Waybar config shows today – workspaces, the task list
  in the context colors, clock, the active context's memory, keyboard layout,
  CPU, memory, volume, network, battery and the **tray** (StatusNotifierItem
  host and DBusMenu with GTK4 menus; `libdbusmenu-gtk3` has no GTK4 version).
  Until then the session keeps Waybar.
- **Dragging** works inside the bar only: Hyprland accepts no drops on the
  desktop. A drop onto a workspace button moves the window there.
- **Data:** koje's own data (contexts, layouts, stash, memory) from kojed over
  D-Bus; workspaces and windows from Hyprland's event socket; system data from
  the usual services (PipeWire, NetworkManager, UPower).
- **Decisions stay pure:** what the bar shows and what a click or drop does are
  functions without GTK, unit-tested like the rest of koje; the widgets only
  render them and send the commands.
- The settings app, the dropped panel and layout overview, and the overlays
  stand as decided in ADR 0036 – only their toolkit changes.

## Consequences

+ The three wishes above become possible, and koje's bar no longer needs
  workarounds or a self-built Waybar.
+ One toolkit and one theme for every surface koje draws.
+ One connection to kojed and one Hyprland event stream for all surfaces; the
  settings app shares the code and opens quickly.
- Bar, OSD, overlays and notifications fail together; the service restart and
  the stateless shell keep that short.
+ koje's UI no longer waits for Kiesel Desktop; GTK4 brings accessibility, input
  methods and scaling.
- koje maintains what Waybar gave for free: tray, volume, network, battery,
  several monitors, hotplug. The first release of the bar is large.
- GTK4 without libadwaita looks plain; koje's own look is CSS work (open
  question).
- Building needs the GTK4 and gtk4-layer-shell development packages, also in
  the dev container.
- koje's Waybar config, `koje bar`'s Waybar output, the context colors CSS
  (M8.20), `scripts/build-waybar-deb.sh` and Waybar in `koje setup`/`doctor`
  (ADR 0021, 0037) go away when the bar ships.
- `docs/upstream/kiesel-desktop-components.md` is no longer a request; it stays
  as a list of what koje's settings app needs.
- Revises ADR 0036 (bar and toolkit) and the Kiesel OSD in ADR 0041.
- Follow-ups: plan tasks M8.27–M8.36.
