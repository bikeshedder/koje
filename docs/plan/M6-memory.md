# M6 – Memory

- [ ] **M6.1 Read memory usage per context** (cgroup v2 `memory.current`, incl. nested containers per M2.6)
- [ ] **M6.2 Soft limit**: notifications at 90 % and 100 % (no spam: hysteresis)
- [ ] **M6.3 Hard limit**: orderly shutdown with save to `default`, timeout, then hard stop
- [ ] **M6.4 Kernel safety net** (`memory.max`) set at container creation and on `koje limit`
- [ ] **M6.5 `koje stats`** (table + `--json` for the bar module) and **`koje limit`** (`podman update`)
- [ ] **M6.6 Default limits** policy (open question → ADR)
