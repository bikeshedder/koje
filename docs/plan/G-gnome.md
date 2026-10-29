# G – Experiment: koje on GNOME Shell

Branch `gnome` only (ADR 0059). koje runs inside a stock GNOME 50 session. A
GNOME Shell extension adds what is koje's own; GNOME provides everything else.
No custom launcher, overview, bar, OSD, notifications or settings bridges. The
Hyprland path stays untouched until G.14 decides. Limitations: ADR 0059;
upstream asks: [docs/upstream/gnome-apis.md](../upstream/gnome-apis.md).

Kept: contexts and containers, states (one geometry per window), memory limits,
sleep with the sleep overlay, the koje settings app, dotfiles, mounts, access
profiles. Deferred: layouts and stash, overlays, pin/unpin, snap zones.

## Decisions and spikes

- [x] **G.1 ADR 0059, plan, upstream list**
  - ADR 0059 (Proposed), this file, `docs/upstream/gnome-apis.md`, roadmap entry.
- [ ] **G.2 Wayland globals for a context client on Mutter** *(host)*
  - Done when: [report](../spikes/G.2-mutter-wayland-globals.md) lists every global a context client sees, and screen capture without the portal fails. Decides L1.
- [ ] **G.3 Freezing a context under Mutter** *(host)*
  - Done when: [report](../spikes/G.3-mutter-freeze.md) says whether windows survive 1/10/30 min frozen with input going on, and what GNOME shows. Decides L8; if windows are lost, sleep needs another design before G.11.
- [ ] **G.4 Headless gnome-shell for tests** *(host first, then dev container)*
  - Done when: [report](../spikes/G.4-headless-gnome-shell.md) says whether `gnome-shell --headless` loads an unpacked extension in the build container and can be driven over D-Bus.
  - First finding (G.15, dev container): `gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800` in the lab image starts, loads an extension from `/usr/share/gnome-shell/extensions`, takes clients and answers `Eval` (unsafe mode); screenshots never return on the software-rendered virtual monitor.
- [x] **G.5 App identity across contexts** *(host; ran in the lab)*
  - Done when: [report](../spikes/G.5-app-identity.md) shows how the dash, Alt+Tab and the app grid treat the same app in two contexts and on the host, with and without exported `.desktop` files. Decides L3/L5.
  - Result: exported entries alone change nothing (app id `foot` → `foot.desktop` wins). A per-context app id plus a matching `StartupWMClass` separates the contexts everywhere (dash, Alt+Tab), but few programs let the caller set their app id. Decided 2026-10-29: **accept the grouping** for the prototype; other solutions (per-context app ids, a koje-owned window → app mapping, upstream) are tried once it works. Exported entries must never change their `StartupWMClass` (GNOME keeps the old mapping for a while).

- [x] **G.15 GNOME lab in the dev container**: GNOME Shell 50 nested (devkit window on the desktop) with spike contexts on it, so the spikes run isolated from the host session
  - `docs/spikes/setup/gnome-lab/` (`lab.sh`, README with the differences from the host). Checked: the shell starts, a context's foot and a host foot open in it with their real PIDs, freeze/thaw, `stop` leaves no processes. G.2's first step ran as a check: context and host clients see the same 38 globals, `grim` fails. Not checked from here: what the devkit window looks like on the desktop.

## Build

- [ ] **G.6 `koje-gnome` adapter**: zbus client of `io.koje.Shell1` implementing `koje_core::ports::Compositor`
  - Done when: windows, events and every `CompositorCommand` map to calls; tests against a fake service; `just check`.
  - Notes: interface designed here first (XML in `gnome/extension/koje@koje.io/dbus/`); `address` = `Meta.Window.get_id()` as decimal; workspace names `<context>` / `host-<n>`.
- [ ] **G.7 Extension skeleton**: `io.koje.Shell1` (`Windows`, `ActiveWorkspace`, `Monitors`, `Apply`, event signals), workspace ↔ context map, context workspaces kept alive (L6)
  - Done when: installable with `gnome-extensions install`; ESLint clean; checked on the host with `gdbus call`.
- [ ] **G.8 kojed on GNOME**: picks the adapter (`KOJE_COMPOSITOR`, else `XDG_CURRENT_DESKTOP`); Hyprland-only services (gsettings bridge, DisplayConfig, overlays, binds, plugin) only with the Hyprland adapter; Wayland socket per G.2
  - Done when: `koje up`/`down`/`focus`/`run`/`save` work in a GNOME session on the host; unit tests for the selection.
- [ ] **G.9 Glow and badge** for windows outside their context (ADR 0057's rule) in the extension; colors from kojed
- [ ] **G.10 Context apps in GNOME's grid, search and dash** (L4/L5): kojed exports each running context's `.desktop` entries; the extension shows those of the current workspace and launches into its context
  - Notes: the same program in several contexts stays one app in the dash and Alt+Tab (L3, G.5); activation prefers the current workspace's windows. No per-context app ids yet.
- [ ] **G.11 Sleep overlay**: "<context> is asleep – Wake up" on sleeping windows; GNOME's not-responding dialog suppressed for them (L8, after G.3)
- [ ] **G.12 Top-bar indicator**: current context (name, color), memory against its limits, "koje Settings…"
- [ ] **G.13 Packaging and CLI**: the extension in the .deb, `koje setup` enables it, `koje doctor` checks GNOME version and extension; deferred commands report "not available on GNOME" (CLI, `docs/architecture.md`, ADR 0005 together)
- [ ] **G.14 Evaluation**: code kept vs. not needed (lines), daily-use findings, limitations hit, recommendation (adopt → ADR to remove the Hyprland path; or stop)
