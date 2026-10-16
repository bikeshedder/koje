# M3 – Context containers

Goal: real contexts with Podman. Windows appear on the desktop; no colors or layouts
yet. Break down further using the M2.3–M2.5 reports.

- [ ] **M3.1 Podman adapter implementing `ContainerRuntime`** (CLI-based first; REST later)
- [ ] **M3.2 Container spec from `context.toml`**: mounts at same path, home volume, sockets, devices, D-Bus proxy, labels, limits (`memory.max` safety net)
- [ ] **M3.3 Base image build** (`koje/base`): Containerfile in the repo, `koje image build`? (decide)
- [ ] **M3.4 `koje up <context> -s empty` / `koje down <context> --no-save`**
- [ ] **M3.5 `koje shell <context> [--dev]`**
- [ ] **M3.6 `koje open <context> <url|file>`**
- [ ] **M3.7 `koje sleep` / `koje wake`** (windows to stash comes with M5)
- [ ] **M3.8 `koje rm` removes containers and volumes** (with confirmation)
- [ ] **M3.9 Ignored integration tests against real Podman** (label `io.koje.test=true`, cleanup)
- [ ] **M3.10 Context `global`** for overlays: created and started at login (systemd user unit?)
