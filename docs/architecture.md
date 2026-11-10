# Architecture (English summary of the concept)

Source of truth for product behavior: `docs/concept/koje-konzept.pdf` (German) plus
the ADRs in `docs/decisions/`. This file summarizes both for day-to-day work.

## Goals

| Goal | How |
|---|---|
| Windows of a project belong together and are recognizable | one context = one container, one GNOME workspace, one color |
| Links and files open in the right context | each container has its own default apps; what opens inside stays inside |
| Projects are isolated from each other | own namespaces (PID, net, IPC, mounts); own nested Podman per context |
| Inactive projects cost nothing | stop or pause contexts as a unit; memory limits per context |
| The work environment can be restored | states; continuous crash recovery |
| Programs are not modified | programs run as on a normal system; optional integrations |

## Big picture

```
Host: GNOME Shell + koje's extension · kojed · rootless Podman
├── context "kundenportal" (container)        color green
│   ├── editor, browser, terminal, …          (Wayland socket passed in)
│   └── nested Podman → dev container, compose services
└── context "shop" (container)                color blue
    └── nested Podman → … (same compose names do not collide)
shared base image: read-only layers used by all contexts
```

GNOME provides the desktop: top bar, overview, dash, settings, displays,
notifications, lock screen and the session (ADR 0059, 0061). Graphical programs
talk directly to GNOME's Mutter through the Wayland socket koje binds into the
container. koje's GNOME Shell extension (`gnome/src/extension`, TypeScript) adds
what is koje's own and serves `io.koje.Extension1`; kojed drives it through the
`koje-gnome` adapter (windows, workspaces, placement, events).

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
privileged protocols. Mutter offers no screen capture or input injection to
Wayland clients (capture goes through D-Bus, which contexts do not see), but it
has no security contexts either: contexts share its socket (ADR 0059, L1).

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

## Desktop (GNOME)

- **GNOME as it is**, extended by koje's extension (ADR 0059): one GNOME
  workspace per context; kojed keeps a running context's workspace alive.
- **Context color:** kojed maps each new window's PID to its context and tells
  the extension. A window outside its context's workspace glows in the
  context's color and carries a badge naming it (ADR 0057). GNOME's workspace
  switchers show context workspaces in their color with their name (G.21).
- **Top-bar indicator:** the current context, its memory against its limits,
  "koje Settings…" (G.12, G.18).
- **Context apps** appear in GNOME's app grid and search on the context's
  workspace and start inside the context (G.10). The same program in several
  contexts is one app in Alt+Tab (ADR 0059, L3).
- **A dash per context** (planned, ADR 0062, G.32): on a context's workspace the dash
  shows only that context's favorites and running apps, on a host workspace
  only the host's. `favorites` in `context.toml`; unset, GNOME's favorites the
  context has installed.
- **Sleep:** sleeping windows get the overlay "<context> is asleep – Wake up"
  and receive no input (ADR 0024, 0059 L8).
- **Icons and labels** (ADR 0055): a context has an icon and a label
  (`context.toml`).
- **Files between contexts:** clipboard text/images always work. Drag & drop of
  files passes paths, so koje mounts folders at the same path as on the host. A
  shared exchange folder (e.g. `~/Austausch`) is a deliberate hole. Context A →
  context B fails by design. Mounts that would hide the context's home, expose
  koje's own files or host sockets, or replace system folders are refused unless
  marked `unsafe` (ADR 0009). Mounts are folders below the home (ADR 0034) and
  follow `context.toml` while the context runs (ADR 0033).
- **Access profiles:** each context gets GPU, sound, Wayland, nested containers and
  network according to its access profile (`full`, `desktop`, `headless`, `isolated`
  plus overrides; default from `config.toml`, ADR 0017).

## Layouts

Layouts stay part of a state's file format (ADR 0004). On GNOME a state restores
one geometry per window; switching layouts, the stash and overlays are not
available (ADR 0061) until they are designed for GNOME.

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
| `koje focus <context>` | switch to a context |
| `koje sleep \| wake <context>` | pause / resume (CPU 0, memory kept) |
| `koje stats` / `koje limit <context>` | memory per context / change limits |
| `koje open <context> <url\|file>` | open inside a context |
| `koje run <context> [--no-focus] [-u <user>] <program> [args…]` | switch to the context's workspace (unless `--no-focus`) and start a program there, detached (in the current directory if mounted); `-u root` runs it as the context's root (your user on the host: contexts are rootless) |
| `koje apps [-c <context>] [--start <name>]` | list the programs of the active context (its `.desktop` entries), or start one there by name or desktop file id |
| `koje favorites [-c <context>] [--set <id>… \| --reset]` | the dash's favorites in a context (ADR 0053, 0062): desktop file ids of its programs in `context.toml`, pinned from the dash; unset, GNOME's favorites as far as the context has them |
| `koje dotfiles [-c <context>]` | link the dotfiles presets again now in every running context (or one) and list the presets (ADR 0038) |
| `koje terminal [-c <context>]` | open a terminal (foot) in the active context; on a workspace without a context, on the host (Super+Return, ADR 0037) |
| `koje shell <context> [--dev [<container>]] [-u <user>]` | shell in the context, or in its (only / named) dev container; `-u root` logs in as root there |
| `koje settings [<context>]` | open koje Settings (`koje-settings`, ADR 0060), on a context if given |
| `koje config [--stdin]` | edit `config.toml` in $EDITOR; `--stdin` takes the new file from stdin (the settings app) |
| `koje new \| edit \| rm <context>`, `koje list` | manage contexts; `edit --stdin` takes the new `context.toml` from stdin (the settings app) |
| `koje recreate <context> [--yes]` | new container from the current config; saves and restores the windows; loses what was installed in the old one outside the home |
| `koje image build [<image>…] [--no-cache] [--pull]` | build images (default: those contexts use) with what they build on and your user on top (ADR 0040); contexts get them when recreated |
| `koje image new <name> [--from <image>]` | create the image folder `~/.config/koje/images/<name>` with a Containerfile (ADR 0040) |
| `koje setup [--dry-run] [--yes]` | after the package (ADR 0045): enable koje's GNOME Shell extension, build koje's image `fedora`, restart an outdated kojed; plan first, asks once |
| `koje doctor` | check the package, packages, GNOME Shell and the extension, Mutter's Wayland globals, kojed, image, forwarder, sub-ids, NVIDIA |

Hyprland-only commands (`layout`, `stash`, `unstash`, `snap`, `overlay`,
`pin`, `unpin`, `bar`, `media`, `screenshot`, `session`) are gone with ADR 0061.

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

**Pausing:** `koje sleep` freezes the whole context incl. dev containers; its
windows get the sleep overlay. Memory stays allocated (can be swapped out).

## Files

| Path | Content |
|---|---|
| `~/.config/koje/config.toml` | global: default limits, default access; older Hyprland tables (`zones`, `overlay`, `workspace`, `bar`, `desktop`) are ignored (ADR 0061) |
| `~/.config/koje/contexts/<context>/context.toml` | hand-written: name, prefix, color, image, mounts, limits, state roles |
| `~/.config/koje/images/<name>/` | an image folder (ADR 0040): `Containerfile` and what it copies; `image = "<name>"` uses it |
| `~/.config/koje/integrations/*.toml` | user integrations |
| `~/.local/share/koje/<context>/states/<state>.toml` | states incl. layouts |
| `~/.local/state/koje/<context>/recovery.toml` | crash recovery |

## Crates

| Crate | Role |
|---|---|
| `koje-core` | domain types and pure logic; ports (traits) for the compositor and Podman |
| `koje-podman` | adapter: rootless Podman (`ContainerRuntime`), images: built-in ones, the user layer, build/pull |
| `koje-gnome` | adapter: koje's GNOME Shell extension over D-Bus (`Compositor`, ADR 0059) |
| `koje-forward` | static D-Bus forwarder that runs inside every context (ADR 0025) |
| `kojed` | service: windows and their contexts, colors, states, recovery, memory; D-Bus API `io.koje.Daemon1` (ADR 0027, 0060) |
| `koje-runtime` | driving context containers: up, down, sleep, mounts, dotfiles, host bus, limits; kojed runs `up`, `down`, `save`, `sleep` and `recreate`, the CLI the rest (`shell`, `run`, `open`, …) |
| `koje` | CLI, client of core, koje-runtime and kojed; `up`, `down`, `save`, `sleep`, `wake`, `recreate` go through kojed (G.29) |

UI (ADR 0060), in `gnome/`: the Shell extension (plain JS so far) and koje
Settings (`koje-settings`, `io.koje.Settings`: TypeScript on GJS, GTK 4 and
libadwaita, layout in Blueprint). Both talk to kojed over `io.koje.Daemon1`;
the settings app's client is generated from `crates/kojed/dbus/io.koje.Daemon1.xml`.

## Outlook (not planned now)

Real hibernation via CRIU checkpoints (root-only today; breaks with Wayland
connections); layouts, the stash and overlays on GNOME.
