# M3 – Context containers

Goal: real contexts with Podman. Windows appear on the desktop; no colors or layouts
yet. Break down further using the M2.3–M2.5 reports.

- [ ] **M3.1 Podman adapter implementing `ContainerRuntime`** (CLI-based first; REST later)
- [ ] **M3.2 Container spec from `context.toml`**: mounts at same path, home volume, sockets, devices, D-Bus proxy, labels, limits (`memory.max` safety net); canonicalize mount sources and re-check ADR 0009 (symlinks); nested Podman flags per ADR 0014; runtime env per ADR 0015; GPU per ADR 0016; devices/sockets per the effective access (ADR 0017); spec in the M2.3 report
- [ ] **M3.3 Base image build** (`koje/base`): Containerfile in the repo, `koje image build`? (decide); image settings per ADR 0014 (start from `docs/spikes/setup/base/`)
- [ ] **M3.4 `koje up <context> -s empty` / `koje down <context> --no-save`**
- [ ] **M3.5 `koje shell <context> [--dev]`**
- [ ] **M3.6 `koje open <context> <url|file>`**
- [ ] **M3.7 `koje sleep` / `koje wake`** (windows to stash comes with M5)
- [ ] **M3.8 `koje rm` removes containers and volumes** (with confirmation)
- [ ] **M3.9 Ignored integration tests against real Podman** (label `io.koje.test=true`, cleanup)
- [ ] **M3.10 Context `global`** for overlays: created and started at login (systemd user unit?)
- [ ] **M3.11 Stale sockets** after a host PipeWire/compositor restart (ADR 0015): detect and restart or remount
  - Notes: seen in M2.4 – after restarting the nested Hyprland, a running context kept the old `wayland-1` socket; mounting the socket's directory (as for the host bus proxy) would avoid it.
- [ ] **M3.12 NVIDIA CDI detection** (ADR 0016): find the CDI spec and the host's GBM backend; hint when missing or outdated
- [ ] **M3.13 Apply access changes to a running context** (ADR 0017): on `koje edit`/config change ask "now (recreate, restore windows) or at next `up`"
- [ ] **M3.14 Host interfaces per context** (ADR 0019): start `xdg-dbus-proxy` with filters from the effective access; forwarder in the context (Rust) started before anything activates the image's portal; own keyring per context
