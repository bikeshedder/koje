# M4 – kojed + Hyprland

Goal: a running service that knows every window's context and colors it.

- [x] **M4.1 Decide service model** (async runtime or threads) and client API (D-Bus vs Unix socket) → ADRs
  - Notes: ADR 0026 (tokio, current-thread), ADR 0027 (D-Bus `io.koje.Daemon`, interface `io.koje.Daemon1`).
- [x] **M4.2 Hyprland adapter implementing `Compositor`** (command socket, batching)
  - Notes: port shape and request syntax per M2.2 report (0.53 syntax); all request strings in one place.
  - Done: crate `koje-hyprland` – `requests` (all strings, argument check against `;`/`,`), `clients` (`j/clients` + `j/monitors` → `WindowInfo`, placement relative to the monitor), instance discovery (signature, else newest; rediscovered when the socket goes away). Port: `Compositor::windows()` + `apply(&[CompositorCommand])` as one `[[BATCH]]`. Live check of the strings: M4.8.
- [x] **M4.3 Event stream** from socket2 with reconnect
  - Done: `koje_hyprland::events` – pure `parse_line` (open/close/focus/title/configreloaded, addresses normalized to `0x…`), `Events::next()` reconnects with backoff and yields `Connected` on every connection (kojed resyncs). Tested against fake sockets.
- [x] **M4.4 Window → context mapping** (PID → cgroup → container label), cached
  - Done (host-tested 2026-10-19): `koje_core::windows::WindowMap` (pure; pid cache lives as long as one of the process's windows), `kojed::daemon` resolves via `ContainerRuntime::context_of_pid` in `spawn_blocking`, resyncs on every `Connected`.
- [ ] **M4.5 Border colors per context** (active/inactive), reapplied on reconnect
  - Notes: ADR 0018 – tag rule per context, re-add on `configreloaded`; check how to replace a rule on color change.
  - Done in code (awaiting host test): rule `koje-<context>` with `Color::dimmed()` as inactive color; rules + tags again on `configreloaded`; a context created after start gets its rule with its first window. Color change: the rule is added again for new windows; open windows of that context get `setprop active_border_color`/`inactive_border_color` (host test showed rules reach only new windows).
- [x] **M4.6 One workspace per context per monitor**; `koje focus <context>`
  - Done (host-tested 2026-10-19), ADR 0028: workspace `<context>`, kojed moves context windows there (not from special workspaces), D-Bus `Focus`, `koje focus`, `koje up` focuses. Single monitor only; per-monitor workspaces wait for the multi-monitor question.
- [x] **M4.7 systemd user unit for kojed**; `koje` talks to kojed, clear error when it is not running
  - Done (host-tested 2026-10-19): `io.koje.Daemon1` with `ListContexts() → a(ssu)`, `ActiveContext() → s`, `Reload()`, signals `WindowAdded`/`WindowRemoved`; `assets/systemd/kojed.service` (`Type=dbus`), installed by `install.sh`, started by `exec-once` in koje's Hyprland snippet; `koje new`/`edit` call `Reload` (silent without kojed); `koje::kojed::call` fails with the start hint when the name has no owner. Test against a private `dbus-daemon`.
- [ ] **M4.8 Ignored integration tests against a live Hyprland**
- [ ] **M4.9 `koje setup` and `koje doctor`** (ADR 0021): the steps of `install.sh` in the CLI (plan, warn, ask; dependency check), plus the systemd user unit from M4.7; `install.sh` then only builds koje and calls `koje setup`
- [ ] **M4.10 Security-context socket per context** (ADR 0022): kojed creates `wp_security_context_v1` sockets (engine `io.koje`, app id `io.koje.<context>`) in per-context runtime directories, keeps them alive, recreates them after a compositor or kojed restart; prototype in `docs/spikes/setup/wl-secure-socket`
- [ ] **M4.11 Stale sockets** (from M3.11, ADR 0015): kojed notices replaced PipeWire/PulseAudio sockets in `$XDG_RUNTIME_DIR` and notifies which running contexts lost sound (a restarted context picks up the new socket)
