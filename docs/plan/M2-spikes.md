# M2 – Spikes (host)

Goal: prove the risky assumptions on the real machine before building on them.
Each spike ends with a report in `docs/spikes/` and, where something is settled, an
ADR. Agents can prepare scripts and report skeletons; the user runs them on the host.
Runbook and scripts: [`docs/spikes/setup/`](../spikes/setup/README.md). Order: M2.3,
M2.5, M2.6 under plain GNOME; then M2.2, M2.4, M2.8 with Hyprland nested in GNOME;
M2.1 and the final checks of M2.7/M2.9 in a real Hyprland session.

- [~] **M2.1 Hyprland daily setup** *(host)*
  - Done when: all windows floating, Super+LMB/MMB move/resize, edge snapping, two special workspaces (`mail`, `chat`) toggle by key; one week of daily use noted in the report.
  - Record: Hyprland version, config snippets that worked, anything that annoyed.
  - Also: with `hyprland-guiutils` installed, does a paused context trigger "not responding" (M2.8)?
  - Also: GTK 4 apps from a context (Nautilus) – black window with the Vulkan and cairo renderers in the nested Hyprland, fine with `GSK_RENDERER=gl` (M2.7). Same in a real session? If yes, set `GSK_RENDERER=gl` in contexts.
- [x] **M2.2 Hyprland IPC from Rust** *(host)*
  - Done when: a small throwaway program lists clients (`j/clients`), moves a window, sets a per-window border color, and prints `openwindow`/`closewindow` events from socket2. Measure: time to move 5 windows in one batch.
  - Notes: decides the shape of the `Compositor` port and whether batching (`[[BATCH]]`) is enough for fast layout switches.
  - Result: [report](../spikes/M2.2-hyprland-ipc.md), ADR 0018. Batch 0.1 ms; pid → cgroup works; no geometry events for floating windows.
- [x] **M2.3 Base image and a hand-made context container** *(host)*
  - Done when: `podman run` with Wayland socket, PipeWire, `/dev/dri`, home volume, label `io.koje.context=<name>` shows a browser and a terminal on the desktop with sound and GPU; link clicked in the terminal opens in the container's browser.
  - Record: the exact `podman run` flags → becomes the spec for M3.
  - Notes: [report](../spikes/M2.3-context-container.md), ADR 0015 (runtime env), ADR 0016 (GPU). Done under GNOME; the Hyprland column is filled during M2.2.
- [x] **M2.4 Filtered D-Bus** *(host)*
  - Done when: `xdg-dbus-proxy` (or equivalent) inside the setup from M2.3 allows notifications and the file chooser portal but blocks `OpenURI` on the host. Decide which other interfaces to allow (screen sharing for video calls, secrets).
  - Notes: [report](../spikes/M2.4-filtered-dbus.md), ADR 0019. Own bus + forwarder + `xdg-dbus-proxy`; notifications and file chooser work, `OpenURI` denied.
- [x] **M2.5 Nested Podman** *(host)*
  - Done when: inside the context container, rootless Podman runs a compose project and a dev container (VS Code inside the context attaches to it). Shared read-only image store via `additionalimagestores` tried.
  - Record: required flags (`/dev/fuse`, sub-UIDs, storage driver), startup time, disk usage.
  - Notes: [report](../spikes/M2.5-nested-podman.md), ADR 0014. Needs `unmask=/proc/sys`, `/dev/net/tun`, file caps on `newuidmap`. Electron apps also need a bigger `/dev/shm` (M2.3).
- [x] **M2.6 Memory sharing between contexts** *(host)*
  - Done when: two contexts from the same base image run the same browser; compare PSS/shared pages (`smem` or `/proc/*/smaps_rollup`) with the browser run twice on the host. Also: does the cgroup memory of a context include nested containers?
  - Notes: [report](../spikes/M2.6-memory-sharing.md). Libraries shared across contexts (libxul Pss 116 → 67 MiB); nested containers counted; page cache charged first-touch. Measured per library instead of against the host's Firefox (a Snap on Ubuntu).
- [x] **M2.7 Files between contexts** *(host)*
  - Done when: drag a file from a context's file manager into a mail client in `global`; check same-path mounts and the exchange folder; note which apps pass paths vs content.
  - Notes: [report](../spikes/M2.7-files-between-contexts.md). DnD passes paths: works via the shared folder, fails otherwise; clipboard works. Retest dragging into `global` once it exists (M3.10).
- [x] **M2.8 Pausing a context with windows** *(host)*
  - Done when: `podman pause` a context with visible windows; observe Hyprland behavior (ANR dialog?), resume; same with windows moved to a special workspace first.
  - Notes: [report](../spikes/M2.8-pausing.md). Frozen, no dialog (without `hyprland-guiutils`), input queued and delivered on unpause. Recheck the dialog in M2.1.
- [x] **M2.9 Screen capture permissions** *(host)*
  - Done when: Hyprland permission rules deny screencopy to context programs while a video call in `global` (or an allowed context) can still share the screen.
  - Notes: [report](../spikes/M2.9-screen-capture.md), ADR 0022. `security-context-v1` socket per context: no direct capture, portal sharing works. Path-based permission rules unusable.
- [x] **M2.10 More host interfaces through the forwarder** *(host)*
  - Done when: settings (`SettingChanged`), inhibit, screen sharing (PipeWire fd passing through the forwarder, a video call in a context shares a window) and print work through proxy + forwarder; Firefox uses the portal file chooser.
  - Notes: [report](../spikes/M2.10-host-interfaces.md). Settings, inhibit, screen sharing work; forwarder needs one host connection per client + path translation; contexts need `XDG_SESSION_TYPE`, `GTK_USE_PORTAL`. Printing → CUPS.
- [x] **M2.11 Remaining host interface checks** *(host)*
  - Done when: Firefox in a context prints to a host printer through the mounted CUPS socket (`/run/cups/cups.sock`); `SettingChanged` reaches the context when dark mode changes; an inhibit disappears from the host's inhibitor list when the calling program exits.
  - Notes: [report](../spikes/M2.11-host-interface-checks.md). CUPS printing, `SettingChanged`, inhibit release all work.
- [ ] **M2.12 Mounts on a running context** *(host)*
  - Done when: a host folder bind-mounted after the context started is visible inside it, rootless, without restarting the container – e.g. a per-context directory mounted with `rslave` propagation and `podman unshare mount --bind <folder> <dir>/<name>` – or it is shown not to work; the report says where such folders can appear (fixed directory vs. their own path) and what unmounting does to open files.
  - Notes: needed for M5.11 (apply access and mount changes without recreating); ADR 0029 solved the simpler case (changing files in a mounted directory).
