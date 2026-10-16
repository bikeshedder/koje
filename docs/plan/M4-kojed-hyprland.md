# M4 – kojed + Hyprland

Goal: a running service that knows every window's context and colors it.

- [ ] **M4.1 Decide service model** (async runtime or threads) and client API (D-Bus vs Unix socket) → ADRs
- [ ] **M4.2 Hyprland adapter implementing `Compositor`** (command socket, batching)
- [ ] **M4.3 Event stream** from socket2 with reconnect
- [ ] **M4.4 Window → context mapping** (PID → cgroup → container label), cached
- [ ] **M4.5 Border colors per context** (active/inactive), reapplied on reconnect
- [ ] **M4.6 One workspace per context per monitor**; `koje focus <context>`
- [ ] **M4.7 systemd user unit for kojed**; `koje` talks to kojed, clear error when it is not running
- [ ] **M4.8 Ignored integration tests against a live Hyprland**
