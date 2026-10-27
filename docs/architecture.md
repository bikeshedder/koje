# Architecture (English summary of the concept)

Source of truth for product behavior: `docs/concept/koje-konzept.pdf` (German) plus
the ADRs in `docs/decisions/`. This file summarizes both for day-to-day work.

## Goals

| Goal | How |
|---|---|
| Windows of a project belong together and are recognizable | one context = one container, one Hyprland workspace, one border color |
| Links and files open in the right context | each container has its own default apps; what opens inside stays inside |
| Projects are isolated from each other | own namespaces (PID, net, IPC, mounts); own nested Podman per context |
| Inactive projects cost nothing | stop or pause contexts as a unit; memory limits per context |
| The work environment can be restored | states and layouts; continuous crash recovery |
| Programs are not modified | programs run as on a normal system; optional integrations |

## Big picture

```
Host: Hyprland · kojed · rootless Podman
├── context "kundenportal" (container)        border color green
│   ├── editor, browser, terminal, …          (Wayland socket passed in)
│   └── nested Podman → dev container, compose services
├── context "shop" (container)                border color blue
│   └── nested Podman → … (same compose names do not collide)
└── context "global" (not isolated)           overlays: mail, calendar, chat
shared base image: read-only layers used by all contexts
```

Graphical programs talk directly to Hyprland through the Wayland socket that koje
binds into the container. Everything else – colors, placement, layouts – koje does
from outside through Hyprland's command and event sockets.

## Context containers

Created by koje directly with Podman. Each gets:

- its own home volume and its own PID, network and IPC namespaces,
- the Wayland socket, the PipeWire socket and `/dev/dri`,
- a **filtered D-Bus** via a proxy (like Flatpak's): notifications and file
  chooser allowed, opening URIs on the host denied (links must not leave the context),
- a label with the context name (PID → cgroup → container → context),
- memory limits.

**Podman in Podman:** each context runs its own rootless Podman with its own image
store volume. Common images can come from a shared read-only store
(`additionalimagestores`). Needs sub-UIDs/GIDs inside, `/dev/fuse`.

**Wayland security:** clients cannot read each other's input or content, except via
privileged protocols (screen capture). Hyprland's permission system should deny
screen capture to context programs.

## Memory

Code and libraries are mmapped files; processes using the *same file* share pages,
also across containers – if all contexts use the same base image layers and native
overlayfs. Private memory (heap, JS, caches) is never shared and dominates for
browsers/editors. One browser per context therefore costs about as much as one
browser profile per project. Put common programs into the base image.

| Limit | Behavior | Mechanism |
|---|---|---|
| soft | warn at 90 % and 100 %, keep running | kojed watches the cgroup, desktop notification |
| hard | orderly shutdown, saving to `default` | kojed, like `koje down` |
| safety net | kernel OOM inside this context only | cgroup `memory.max` slightly above hard |

## Desktop (Hyprland)

- All windows floating (`windowrule = float, class:.*`); Super+LMB move, Super+MMB resize.
- koje's Hyprland snippet is a complete session (ADR 0037): Waybar, mako,
  hypridle/hyprlock, basic bindings, workspaces 1–9 for host windows, look.
  Super+Return opens a terminal in the active context (`koje terminal`).
  Super+F1 lists all keyboard shortcuts (`hyprctl binds` in fuzzel).
- **Look and feel follow GNOME** (ADR 0043): libadwaita, Adwaita themes and
  symbolic icons, a bar laid out like GNOME's top bar, no gaps; Super alone
  opens the overview with the dash (ADR 0049), Super+arrows maximize, restore and tile halves (M8.41).
  The context color is the deliberate difference.
- **System settings are GNOME's** (ADR 0044): GNOME Settings edits them,
  kojed applies GNOME's gsettings to Hyprland (input, keyboard layouts, idle and
  power, night light, background); koje's settings app is only about koje.
- **Context color** per window: kojed sees each new window on the event socket,
  maps PID → context, tags it and sets the context's border colors. Borders are
  off by default – windows look as GNOME's; koje's plugin makes a window on
  another context's or a host workspace glow in its context's color and gives
  it a badge with the context's name (ADR 0057). The bar
  (a clone of GNOME's top bar, ADR 0047) shows each workspace's icon and
  label, a context's in its color (ADR 0055).
- **Icons and labels** (ADR 0055): host workspaces (`[workspace.<n>]` in
  `config.toml`), contexts (`context.toml`) and overlays each have an icon and
  a label for the bar and the settings app.
- **Workspaces:** one per context per monitor. Several work areas inside a context
  are **layouts**, not workspaces.
- **Snapping:** Hyprland edge snapping (built in); zones per monitor via
  `koje snap <zone>`; drag-into-zone preview would need a plugin (later).
- **Overlays:** special workspaces toggled by a key, configured as
  `[overlay.<name>]` in `config.toml` (`key`, `label`, `icon`, `apps`; without any, `mail`
  on Super+M and `chat` on Super+C); kojed binds the keys, the bar shows them
  after the workspaces. Apps run in the non-isolated context `global`. `koje pin` puts an overlay window onto the
  current workspace and keeps it visible; `koje unpin` sends it back.
- **Files between contexts:** clipboard text/images always work. Drag & drop of
  files passes paths, so koje mounts folders at the same path as on the host. A
  shared exchange folder (e.g. `~/Austausch`) is a deliberate hole. Context A →
  context B fails by design. Mounts that would hide the context's home, expose
  koje's own files or host sockets, or replace system folders are refused unless
  marked `unsafe` (ADR 0009). Mounts are folders below the home (ADR 0034) and
  follow `context.toml` while the context runs (ADR 0033).
- **Wayland:** each context gets its own socket created by kojed through
  `security-context-v1`; its programs get normal windows but cannot capture the
  screen directly – screen sharing goes through the portal (ADR 0022).
- **Access profiles:** each context gets GPU, sound, Wayland, nested containers and
  network according to its access profile (`full`, `desktop`, `headless`, `isolated`
  plus overrides; default from `config.toml`, ADR 0017).

## Layouts

A layout is a named arrangement of a context's windows (`code`, `review`, `docs`).
A window may be in several layouts with different geometry. Running windows not in
the active layout sit in the **stash** (hidden special workspace) and appear as
icons in the bar.

| Term | Defines | Switch cost |
|---|---|---|
| context | container, color, windows | instant |
| state | *which* programs run, plus their layouts | expensive, rare |
| layout | *how* running windows are arranged, which are visible | milliseconds |
| stash | running windows missing from the active layout | – |

Switching: windows not in the target go to the stash; target windows get their
geometry (unstashing as needed); focus goes to the last active window of the
target. Nothing starts or stops. Pure logic: `koje_core::layout::plan_switch`.

Editing: **autosave** per layout (default on) writes moves/resizes back immediately;
with autosave off a layout is a template, updated with `koje layout save`. New
windows join the active layout; closed windows leave all layouts.

Layouts are **part of a state** (they reference concrete windows). Day to day there
is only the `default` state, so layouts feel like they belong to the context.

## Integrations

Optional. Without one, any window is recognized, colored, saved and relaunched with
its original command line (`generic`). Integrations improve launching and what is
remembered: `terminal` (cwd, context vs dev container), `editor` (folder),
`devcontainer` (start inner dev container first), `browser` (leave tabs to the
browser's session restore, accept `koje open`). Format: small TOML files in
`~/.config/koje/integrations/` with `match`, `launch`, `remember`, `detect`.

## CLI

| Command | Effect |
|---|---|
| `koje up <context> [-s <state>] [--no-save] [--no-focus]` | start, show the context's workspace (unless `--no-focus`), open and arrange the state's windows (default role `default`); if running, switch to the state (saving the current windows into the state it is in first unless `--no-save`) |
| `koje up <context> --recover \| --no-recover` | after a crash, take or discard the recovery file without asking |
| `koje down <context> [-s <state>]` | save (only that state), close windows in order, stop container |
| `koje down <context> --no-save` | shut down without saving; all states unchanged |
| `koje save <context> [-s <state>]` | save without shutting down |
| `koje state list \| cp \| mv \| rm` | manage states (`rm` is the only way to delete) |
| `koje layout <name> \| next \| prev` | switch layout |
| `koje layout new \| save \| autosave \| list \| mv \| rm` | manage layouts |
| `koje stash [--show]` / `koje unstash [<window>] [--to <layout>]` | stash the focused window / show the stash / bring a window back – by id or Hyprland address (default: the focused one); `-c <context>`, default the active one |
| `koje focus <context>` | switch to a context |
| `koje snap <left\|right\|maximize\|restore>` | the focused window to a half of its monitor below the bar, all of it, or back where it was (Super+arrows, M8.41); zones from `config.toml` later (M7.3) |
| `koje overlay <name>` / `pin` / `unpin` | overlays |
| `koje sleep \| wake <context>` | pause / resume (CPU 0, memory kept) |
| `koje stats` / `koje limit <context>` | memory per context / change limits |
| `koje open <context> <url\|file>` | open inside a context |
| `koje run <context> [--no-focus] <program> [args…]` | switch to the context's workspace (unless `--no-focus`) and start a program there, detached (in the current directory if mounted) |
| `koje apps [-c <context>] [--start <name>] [--menu \| --json]` | list the programs of the active context (its `.desktop` entries), or start one there by name or desktop file id – koje's application menu (Super+D); `--menu` puts the context and its memory first; `--json` with icons, keywords, window class and program for the shell |
| `koje favorites [-c <context>] [--set <id>… \| --reset]` | the dash's favorites in a context (ADR 0053): desktop file ids of its programs in `context.toml`, pinned from the dash; unset, GNOME's favorites as far as the context has them |
| `koje dotfiles [-c <context>]` | link the dotfiles presets again now in every running context (or one) and list the presets (ADR 0038) |
| `koje terminal [-c <context>]` | open a terminal (foot) in the active context; on a workspace without a context, on the host (Super+Return, ADR 0037) |
| `koje shell <context> [--dev [<container>]]` | shell in the context, or in its (only / named) dev container |
| `koje settings [<context>]` | open the settings app (`koje-shell settings`), on a context if given |
| `koje config [--stdin]` | edit `config.toml` in $EDITOR; `--stdin` takes the new file from stdin (the settings app) |
| `koje new \| edit \| rm <context>`, `koje list [--json]` | manage contexts; `edit --stdin` takes the new `context.toml` from stdin (the settings app) |
| `koje recreate <context> [--yes]` | new container from the current config; saves and restores the windows; loses what was installed in the old one outside the home |
| `koje image build [--no-cache]` | build the base image `koje/base` for the current user (ADR 0023) |
| `koje setup [--dry-run] [--yes]` | install koje's per-user files after the package (ADR 0045), remove the old `install.sh` layout: plan, warnings, one question (ADR 0021) |
| `koje bar [--memory] [--names]` | Waybar module: running contexts in their colors, the active one marked; `--memory`: the active context's memory as a meter against its limits; `--names`: list the running contexts once (with `--memory`: with use and limit) |
| `koje media <action>` | volume, microphone, brightness and media keys (ADR 0041): `volume-up`, `volume-down`, `mute`, `mic-mute`, `brightness-up`, `brightness-down` with an OSD; `play-pause`, `next`, `previous` for the most recently active player on the host or in any context |
| `koje screenshot [area\|screen\|window]` | a screenshot as GNOME takes it: into Pictures/Screenshots, onto the clipboard, with a notification (Print, Shift+Print, Alt+Print) |
| `koje doctor` | check packages, Hyprland, installed files, kojed, security contexts, image, sub-ids, NVIDIA |
| `koje session start \| restart \| end <log-out\|restart\|power-off>` | the koje session (ADR 0048): `start` runs Hyprland with koje's config (the login screen's "koje" entry) and stops koje's services when it exits; `restart` restarts koje's services with their current unit files (Waybar and mako stop: the shell is the bar and the notification server) and loads koje's Hyprland plugin again from its file, the session goes on – `koje setup` does it after a package update; `end` asks through the shell (GNOME's 60-second countdown) |

**States:** `default` (read by a plain `up`), `fresh`
(curated start), `empty` (reserved, virtual, read-only), any other name. Missing
state → `fresh` → `empty` (only when a state was never created). Role names are
renamable in `context.toml`. `save` and `down` without `-s` write the state the
context is in (loaded from or switched to; `default` if none, ADR 0032).
Switching states in a running context keeps windows present in both, closes
the rest, starts the missing ones; it saves into the current state first unless
`--no-save` (nothing from `empty`).

**Crash recovery:** kojed writes the current state after every change (debounced,
atomic rename) to the recovery file. A clean `down` deletes it. If `up` finds one,
it asks whether to restore.

**Pausing:** `koje sleep` freezes the whole context incl. dev containers; windows go
to the stash first. Memory stays allocated (can be swapped out).

## Files

| Path | Content |
|---|---|
| `~/.config/koje/config.toml` | global: zones, overlays, default limits, the bar's workspace style (`[bar]`), the windows' and overlays' look (`[desktop]`, ADR 0056) |
| `~/.config/koje/contexts/<context>/context.toml` | hand-written: name, prefix, color, image, mounts, limits, state roles |
| `~/.config/koje/integrations/*.toml` | user integrations |
| `~/.local/share/koje/<context>/states/<state>.toml` | states incl. layouts |
| `~/.local/state/koje/<context>/recovery.toml` | crash recovery |

## Crates

| Crate | Role |
|---|---|
| `koje-core` | domain types and pure logic; ports (traits) for Hyprland and Podman |
| `koje-podman` | adapter: rootless Podman (`ContainerRuntime`), base image |
| `koje-hyprland` | adapter: Hyprland command and event sockets (`Compositor`) |
| `koje-forward` | static D-Bus forwarder that runs inside every context (ADR 0025) |
| `kojed` | service: Hyprland events, colors, layouts, recovery, memory; D-Bus API `io.koje.Daemon1` (ADR 0027) |
| `koje` | CLI, client of core and kojed |
| `koje-shell` | koje's GTK4 UI with libadwaita, one binary: the shell (bar, overview, OSD, notifications, background; later window overlays) and the settings app in its own process (ADR 0036, 0042, 0043); client of kojed and Hyprland, both on a tokio thread |

UI (ADR 0036, 0042; the bar is Waybar until koje's own bar replaces it, M8.32):

| Part | What | Built with |
|---|---|---|
| Launcher | application menu (Super+D), contexts and actions, window picker, keyboard shortcuts (Super+F1) | fuzzel |
| Bar | workspaces and the current workspace's windows (context color as accent), the active context's memory; later layouts, stash | Waybar: hyprland/workspaces, custom module `koje bar --memory` (`koje bar` lists running contexts for own configs) |
| Settings app | contexts, states, layouts, windows, memory – everything viewed and changed | GTK4 with libadwaita, normal window |
| Window overlays | "asleep – Wake up", "not responding – Wait / Quit" on windows (ADR 0024) | GTK4, layer-shell |
| Beacon window | keeps an empty context workspace alive (ADR 0028); may be dropped | GTK4, normal window |

## Outlook (not planned now)

Real hibernation via CRIU checkpoints (root-only today; breaks with Wayland
connections), fly-into-bar animation, drag-into-zone preview, colored title bars
(all need Hyprland plugins).
