# M2 – Spikes (host)

Goal: prove the risky assumptions on the real machine before building on them.
Each spike ends with a report in `docs/spikes/` and, where something is settled, an
ADR. Agents can prepare scripts and report skeletons; the user runs them on the host.
Runbook and scripts: [`docs/spikes/setup/`](../spikes/setup/README.md). Order: M2.3,
M2.5, M2.6 under plain GNOME; then M2.2, M2.4, M2.8 with Hyprland nested in GNOME;
M2.1 and the final checks of M2.7/M2.9 in a real Hyprland session.

- [ ] **M2.1 Hyprland daily setup** *(host)*
  - Done when: all windows floating, Super+LMB/MMB move/resize, edge snapping, two special workspaces (`mail`, `chat`) toggle by key; one week of daily use noted in the report.
  - Record: Hyprland version, config snippets that worked, anything that annoyed.
- [ ] **M2.2 Hyprland IPC from Rust** *(host)*
  - Done when: a small throwaway program lists clients (`j/clients`), moves a window, sets a per-window border color, and prints `openwindow`/`closewindow` events from socket2. Measure: time to move 5 windows in one batch.
  - Notes: decides the shape of the `Compositor` port and whether batching (`[[BATCH]]`) is enough for fast layout switches.
- [x] **M2.3 Base image and a hand-made context container** *(host)*
  - Done when: `podman run` with Wayland socket, PipeWire, `/dev/dri`, home volume, label `io.koje.context=<name>` shows a browser and a terminal on the desktop with sound and GPU; link clicked in the terminal opens in the container's browser.
  - Record: the exact `podman run` flags → becomes the spec for M3.
  - Notes: [report](../spikes/M2.3-context-container.md), ADR 0015 (runtime env), ADR 0016 (GPU). Done under GNOME; the Hyprland column is filled during M2.2.
- [ ] **M2.4 Filtered D-Bus** *(host)*
  - Done when: `xdg-dbus-proxy` (or equivalent) inside the setup from M2.3 allows notifications and the file chooser portal but blocks `OpenURI` on the host. Decide which other interfaces to allow (screen sharing for video calls, secrets).
- [x] **M2.5 Nested Podman** *(host)*
  - Done when: inside the context container, rootless Podman runs a compose project and a dev container (VS Code inside the context attaches to it). Shared read-only image store via `additionalimagestores` tried.
  - Record: required flags (`/dev/fuse`, sub-UIDs, storage driver), startup time, disk usage.
  - Notes: [report](../spikes/M2.5-nested-podman.md), ADR 0014. Needs `unmask=/proc/sys`, `/dev/net/tun`, file caps on `newuidmap`. Electron apps also need a bigger `/dev/shm` (M2.3).
- [x] **M2.6 Memory sharing between contexts** *(host)*
  - Done when: two contexts from the same base image run the same browser; compare PSS/shared pages (`smem` or `/proc/*/smaps_rollup`) with the browser run twice on the host. Also: does the cgroup memory of a context include nested containers?
  - Notes: [report](../spikes/M2.6-memory-sharing.md). Libraries shared across contexts (libxul Pss 116 → 67 MiB); nested containers counted; page cache charged first-touch. Measured per library instead of against the host's Firefox (a Snap on Ubuntu).
- [ ] **M2.7 Files between contexts** *(host)*
  - Done when: drag a file from a context's file manager into a mail client in `global`; check same-path mounts and the exchange folder; note which apps pass paths vs content.
- [ ] **M2.8 Pausing a context with windows** *(host)*
  - Done when: `podman pause` a context with visible windows; observe Hyprland behavior (ANR dialog?), resume; same with windows moved to a special workspace first.
- [ ] **M2.9 Screen capture permissions** *(host)*
  - Done when: Hyprland permission rules deny screencopy to context programs while a video call in `global` (or an allowed context) can still share the screen.
