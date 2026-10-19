# M4 – kojed + Hyprland

Goal: a running service that knows every window's context and colors it.

- [x] **M4.1 Decide service model** (async runtime or threads) and client API (D-Bus vs Unix socket) → ADRs
  - Notes: ADR 0026 (tokio, current-thread), ADR 0027 (D-Bus `io.koje.Daemon`, interface `io.koje.Daemon1`).
- [~] **M4.2 Hyprland adapter implementing `Compositor`** (command socket, batching)
  - Notes: port shape and request syntax per M2.2 report (0.53 syntax); all request strings in one place.
- [ ] **M4.3 Event stream** from socket2 with reconnect
- [ ] **M4.4 Window → context mapping** (PID → cgroup → container label), cached
- [ ] **M4.5 Border colors per context** (active/inactive), reapplied on reconnect
  - Notes: ADR 0018 – tag rule per context, re-add on `configreloaded`; check how to replace a rule on color change.
- [ ] **M4.6 One workspace per context per monitor**; `koje focus <context>`
- [ ] **M4.7 systemd user unit for kojed**; `koje` talks to kojed, clear error when it is not running
- [ ] **M4.8 Ignored integration tests against a live Hyprland**
- [ ] **M4.9 `koje setup` and `koje doctor`** (ADR 0021): the steps of `install.sh` in the CLI (plan, warn, ask; dependency check), plus the systemd user unit from M4.7; `install.sh` then only builds koje and calls `koje setup`
- [ ] **M4.10 Security-context socket per context** (ADR 0022): kojed creates `wp_security_context_v1` sockets (engine `io.koje`, app id `io.koje.<context>`) in per-context runtime directories, keeps them alive, recreates them after a compositor or kojed restart; prototype in `docs/spikes/setup/wl-secure-socket`
- [ ] **M4.11 Stale sockets** (from M3.11, ADR 0015): kojed notices replaced PipeWire/PulseAudio sockets in `$XDG_RUNTIME_DIR` and notifies which running contexts lost sound (a restarted context picks up the new socket)
