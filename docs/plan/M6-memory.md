# M6 – Memory

- [~] **M6.1 Read memory usage per context** (cgroup v2 `memory.current`, incl. nested containers per M2.6)
  - Notes: M2.6 – dev containers have no own cgroup; read `memory.stat` too (anon vs file).
  - Done in code: `ContainerRuntime::memory_usage` → `MemoryUsage` (`memory.current`, `anon`, `inactive_file`), working set per ADR 0035; `memory::Watch` (once per crossing, re-armed below 85 %).
- [ ] **M6.2 Soft limit**: notifications at 90 % and 100 % (no spam: hysteresis)
  - Notes: M2.6 – page cache stays charged to the context that read it first; decide whether limits use `memory.current` or exclude reclaimable file pages (→ ADR).
- [ ] **M6.3 Hard limit**: orderly shutdown with save to `default`, timeout, then hard stop
- [ ] **M6.4 Kernel safety net** (`memory.max`) set at container creation and on `koje limit`
- [ ] **M6.5 `koje stats`** (table + `--json` for the bar module) and **`koje limit`** (`podman update`)
- [x] **M6.6 Default limits** policy (open question → ADR)
  - Done: ADR 0035 – no limits by default; working set counts.
