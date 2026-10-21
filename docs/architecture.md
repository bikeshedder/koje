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
- **Border color** per context: kojed sees each new window on the event socket,
  maps PID → context, sets active/inactive border colors.
- **Workspaces:** one per context per monitor. Several work areas inside a context
  are **layouts**, not workspaces.
- **Snapping:** Hyprland edge snapping (built in); zones per monitor via
  `koje snap <zone>`; drag-into-zone preview would need a plugin (later).
- **Overlays:** special workspaces toggled by a key (`mail`, `chat`), apps run in
  the non-isolated context `global`. `koje pin` puts an overlay window onto the
  current workspace and keeps it visible; `koje unpin` sends it back.
- **Files between contexts:** clipboard text/images always work. Drag & drop of
  files passes paths, so koje mounts folders at the same path as on the host. A
  shared exchange folder (e.g. `~/Austausch`) is a deliberate hole. Context A →
  context B fails by design. Mounts that would hide the context's home, expose
  koje's own files or host sockets, or replace system folders are refused unless
  marked `unsafe` (ADR 0009).
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
| `koje up <context> [-s <state>] [--no-save]` | start, open and arrange the state's windows (default role `default`); if running, switch to the state (saving the current windows into `default` first unless `--no-save`) |
| `koje up <context> --recover \| --no-recover` | after a crash, take or discard the recovery file without asking |
| `koje down <context> [-s <state>]` | save (only that state), close windows in order, stop container |
| `koje down <context> --no-save` | shut down without saving; all states unchanged |
| `koje save <context> [-s <state>]` | save without shutting down |
| `koje state list \| cp \| mv \| rm` | manage states (`rm` is the only way to delete) |
| `koje layout <name> \| next \| prev` | switch layout |
| `koje layout new \| save \| autosave \| list \| mv \| rm` | manage layouts |
| `koje stash [--show]` / `koje unstash [<window>] [--to <layout>]` | stash the focused window / show the stash / bring a window back (default: the focused one); `-c <context>`, default the active one |
| `koje focus <context>` | switch to a context |
| `koje snap <zone>` | put focused window into a zone |
| `koje overlay <name>` / `pin` / `unpin` | overlays |
| `koje sleep \| wake <context>` | pause / resume (CPU 0, memory kept) |
| `koje stats` / `koje limit <context>` | memory per context / change limits |
| `koje open <context> <url\|file>` | open inside a context |
| `koje run <context> <program> [args…]` | start a program in the running context, detached (in the current directory if mounted) |
| `koje shell <context> [--dev [<container>]]` | shell in the context, or in its (only / named) dev container |
| `koje new \| edit \| rm <context>`, `koje list [--json]` | manage contexts |
| `koje image build [--no-cache]` | build the base image `koje/base` for the current user (ADR 0023) |
| `koje setup [--dry-run] [--yes]` | install koje's files for the user: plan, warnings, one question (ADR 0021) |
| `koje bar [--names]` | Waybar module: running contexts in their colors, the active one marked (`--names`: list them once) |
| `koje doctor` | check packages, Hyprland, installed files, kojed, security contexts, image, sub-ids, NVIDIA |

**States:** `default` (read by `up`, written by `down` without `-s`), `fresh`
(curated start), `empty` (reserved, virtual, read-only), any other name. Missing
state → `fresh` → `empty` (only when a state was never created). Role names are
renamable in `context.toml`. Switching states in a running context keeps windows
present in both, closes the rest, starts the missing ones; saves to `default`
first unless `--no-save`.

**Crash recovery:** kojed writes the current state after every change (debounced,
atomic rename) to the recovery file. A clean `down` deletes it. If `up` finds one,
it asks whether to restore.

**Pausing:** `koje sleep` freezes the whole context incl. dev containers; windows go
to the stash first. Memory stays allocated (can be swapped out).

## Files

| Path | Content |
|---|---|
| `~/.config/koje/config.toml` | global: zones, overlays, default limits |
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
| `koje-gui` (later) | panel overlay via layer-shell (toolkit undecided) |

UI stages: 1) launcher menu (fuzzel/rofi), 2) Waybar module (context, layouts,
stash, memory), 3) own panel overlay.

## Outlook (not planned now)

Real hibernation via CRIU checkpoints (root-only today; breaks with Wayland
connections), fly-into-bar animation, drag-into-zone preview, colored title bars
(all need Hyprland plugins).
