# 0059. Experiment: koje as a customization of GNOME Shell

- Status: Accepted ([0061](0061-remove-the-hyprland-path.md))
- Date: 2026-10-29
- Source: discussion ("rather than emulating GNOME, give customizing GNOME a
  try"); plan [G](../plan/G-gnome.md)

## Context

koje runs on Hyprland and rebuilds GNOME around it (ADR 0042–0058). The shell
clones the top bar, overview, dash, Quick Settings, OSD, notifications,
screenshot UI, share picker, background and end-session dialog. A Hyprland plugin draws the
overview and the foreign-window glow. On top of that, kojed bridges GNOME's settings to
Hyprland: gsettings (0044), displays (0046) and media keys (0041). koje also ships its own
session (0037, 0048). That is about half of the code, and every piece is a copy that can
drift from GNOME.

What koje actually cares about is small: contexts as containers, their
windows kept together and recognizable, states, memory limits and sleep.
GNOME Shell is extensible from the inside: an extension runs in the
compositor process and may use Mutter's and the Shell's internals.

## Decision

- **Try koje inside a stock GNOME session** (GNOME 50, Ubuntu 26.04) on the
  branch `gnome`. The Hyprland path stays on that branch untouched until the
  experiment is evaluated (G.14). Removing it needs a new ADR.
- **A GNOME Shell extension** (`gnome/extension/koje@koje.io`, GJS) is koje's
  only code in the Shell. It is thin. It exports `io.koje.Extension1` on the session bus
  (bus name `io.koje.Extension`; `io.koje.Shell` is koje-shell's app id) with windows, events, move, resize, workspace, focus and close and draws what is koje's: the glow and
  badge on windows outside their context (0057's semantics), the sleep overlay (0024), and
  one top-bar indicator (context, memory, koje Settings). It also patches the app grid, search and dash
  so they follow the current workspace's context.
- **kojed keeps its logic** and talks to the extension through a new
  `Compositor` adapter `koje-gnome`. It picks the adapter at start.
- **One GNOME workspace per context, across all monitors.** The extension
  keeps context workspaces alive. GNOME arranges and removes workspaces as
  it always does; context workspaces show in their context's color, with
  their name where there is room: the top bar's workspace dots, the
  overview's thumbnails and the keyboard switcher popup (decided 2026-10-30;
  keeping host workspaces in front was tried and reverted).
- **No** custom launcher, overview, bar, OSD, notification server, share picker,
  screenshot UI, session or settings bridge on GNOME: GNOME's own are used.
  The koje settings app stays. It is about koje only. It is rebuilt in
  TypeScript on GJS, like the extension (ADR 0060).
- **Deferred:** layouts and stash, overlays, pin and unpin, snap zones. A state keeps
  one geometry per window. The commands report "not available on GNOME".
- **The same program in several contexts is one app** in GNOME's dash and
  Alt+Tab for the prototype (L3, spike G.5). Activation prefers the current
  workspace's windows. Per-context app ids, a koje-owned window → app
  mapping or an upstream API are tried once the prototype works.
- **Internals are allowed** during the experiment. Every internal the extension
  touches is listed in [docs/upstream/gnome-apis.md](../upstream/gnome-apis.md)
  as a candidate upstream API.
- The following do not apply on GNOME: 0022 (no `security-context-v1` in Mutter, see
  Limitations), 0028's named workspaces, 0037, 0039, 0041–0044, 0046–0054, the
  plugin part of 0057, and 0058.

## Limitations

| # | Area | Limitation | Experiment workaround | Upstream ask |
|---|---|---|---|---|
| L1 | Wayland isolation | Mutter has no `wp_security_context_v1`: no per-context socket and no sandbox flag. | Contexts use Mutter's socket. Mutter offers no screen-capture or virtual-input protocol to Wayland clients (capture is D-Bus, which contexts don't see, ADR 0019). G.2: a context client sees the same 38 globals as a host client; none captures, injects input, lists windows or reads the clipboard in the background. On the host with Xwayland (G.2): only GPU paths and DRM leases of non-desktop outputs (VR) are added. | security-context-v1 |
| L2 | Window → context | Mutter knows no containers. | `Meta.Window.get_pid()` → kojed `ContextOfPid` (G.8: works in the lab, by PID namespace where nested Podman has no cgroups). | – |
| L3 | Same app in two contexts | `Shell.WindowTracker` maps a window to one `.desktop` by app id, in C. The dash groups Firefox of all contexts into one app. | Alt+Tab per workspace (`app-switcher current-workspace-only`). The dash's activate prefers the current workspace. **Grouping stays wrong**, except for programs whose app id koje can set per context (G.5: then the dash and Alt+Tab separate them). | Window → app hook |
| L4 | Context apps in grid/search | `Shell.AppSystem` reads only XDG `.desktop` files. | kojed exports each context's entries. The extension filters grid, search and dash by workspace (internals). | Grid filter/provider API |
| L5 | Launching on a context workspace | An app installed on the host launches on the host. | Patch AppIcon activation to run the context's entry. | as L3 |
| L6 | Dynamic workspaces | GNOME removes empty workspaces and has no names. GNOME's default `workspaces-only-on-primary` shows the windows of other monitors on every workspace, so a context's workspace spans only the primary monitor (seen in G.7, not yet decided). | Patch `WorkspaceTracker` (G.7: context workspaces kojed keeps stay while empty); labels in thumbnails and the indicator. | Named/pinned workspaces |
| L7 | Placing new windows | Mutter places Wayland windows itself. | Move on `window-created`. Possible one-frame flicker. | Placement hint for a trusted client |
| L8 | Frozen contexts | Shell shows "not responding" for a frozen window once it is focused (within 10 s). Mutter queues at most 1 MiB per client; when the queue of a frozen program is full, Mutter disconnects it at the thaw and all its windows are lost (G.3). | Sleeping windows get no input: the overlay catches the pointer, the extension never focuses, moves or resizes them, and suppresses the close dialog for them. Untouched, they survived 30 min (G.3). | Per-window "suspended" state |
| L9 | Foreign-window glow | – | An effect on `Meta.WindowActor`. Simpler than the plugin. | – |
| L10 | Notifications from contexts | GNOME shows the forwarder as the sender. | koje-forward adds hint `x-koje-context`; the extension tints the banner. | Source hint |
| L11 | Sharing a context | GNOME's picker offers monitors and windows, not workspaces. | none (as on Hyprland) | – |
| L12 | Fragility | Internals change every release. Reloading the Shell on Wayland needs a logout. | `shell-version: ["50"]`. | the asks above |
| L13 | Deferred features | No hidden/special workspaces. Minimized windows stay in the dash and Alt+Tab. | Layouts, stash and overlays are not built. | – |
| L14 | Testing | Extension code is JS, outside `cargo test`. | Adapter tests against a fake `io.koje.Extension1`, checked against the interface file the extension loads; the adapter against the real extension in the GNOME lab (`just test-extension`); ESLint (`just lint-extension`). | – |

## Consequences

+ GNOME provides everything except koje's own features: bar, overview, settings,
  displays, input, OSD, notifications, lock screen, network secrets, Polkit
  and the session.
+ The foreign-window glow and the sleep overlay are easier inside the
  compositor than through IPC.
- Weaker Wayland isolation story until Mutter supports security contexts
  (L1). The apps of one program in several contexts group together (L3).
  Update (G.33): `koje doctor` warns about any Mutter global spike G.2 did not
  review. A proxy that hides such globals from contexts stays a deferred
  option (it would hide the program's PID from Mutter, L2).
- Layouts, the stash and overlays, which are koje features, are gone for now.
- The extension depends on GNOME internals. Each GNOME release needs a check.
