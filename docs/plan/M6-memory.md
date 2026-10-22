# M6 – Memory

- [x] **M6.1 Read memory usage per context** (cgroup v2 `memory.current`, incl. nested containers per M2.6)
  - Notes: M2.6 – dev containers have no own cgroup; read `memory.stat` too (anon vs file).
  - Done in code: `ContainerRuntime::memory_usage` → `MemoryUsage` (host test: `CgroupPath` ends in `/container`, the init only – the scope above it is read) (`memory.current`, `anon`, `inactive_file`), working set per ADR 0035; `memory::Watch` (once per crossing, re-armed below 85 %).
- [x] **M6.2 Soft limit**: notifications at 90 % and 100 % (no spam: hysteresis)
  - Done (host-tested 2026-10-22): kojed's 5 s tick reads each running context's working set (`Daemon::check_memory`, limits from the config files), `memory::Watch` decides, desktop notification.
  - Notes: M2.6 – page cache stays charged to the context that read it first; decide whether limits use `memory.current` or exclude reclaimable file pages (→ ADR).
- [x] **M6.3 Hard limit**: orderly shutdown with save to `default`, timeout, then hard stop
  - Done (host-tested 2026-10-22): saves into the current state (ADR 0032; else `default`), ends the programs in order, waits 10 s, stops the container, notifies (kojed `hard_stop`).
- [x] **M6.4 Kernel safety net** (`memory.max`) set at container creation and on `koje limit`
  - Done (host-tested 2026-10-22): after every start and on `koje limit` (koje writes the scope's `memory.max` itself after every start – `podman update` refused `-1` and its stored swap made crun refuse to start).
- [x] **M6.5 `koje stats`** (table + `--json` for the bar module) and **`koje limit`** (`podman update`)
  - Done (host-tested 2026-10-22): `koje stats [<context>] [--json]` (working set, charged, limits, level), `koje limit <context> [--soft X|none] [--hard X|none]` (edits `context.toml` with comments kept, checked with `config.toml`'s defaults first, kernel limit live, kojed reload).
- [x] **M6.6 Default limits** policy (open question → ADR)
  - Done: ADR 0035 – no limits by default; working set counts.
  - Host test 2026-10-22: tmpfs files as memory (`head -c … > /run/user/<uid>/hog`): near/over soft notified once each, hard limit → saved, stopped, notified; `koje up` restores. Streaming pipes (`head | tail`) hold no memory; bash's `$(printf …)` doubles its buffer and was killed by the kernel limit (safety net works).
