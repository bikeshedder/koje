# 0022. A security-context Wayland socket per context

- Status: Accepted
- Date: 2026-10-18
- Source: spike [M2.9](../spikes/M2.9-screen-capture.md), Hyprland 0.53.3 real session

## Context

With the compositor's normal socket, any program in a context can capture the
whole screen (`wlr-screencopy`) without asking. Hyprland's permission rules
match the client's executable path, but the host sees a context program's path
as inside the container (`/usr/bin/grim`), so a rule hits host and context
alike. Wayland's `security-context-v1` lets a privileged client create a socket
whose clients the compositor treats as sandboxed. Hyprland 0.53.3 supports it,
withholds the screen capture protocol from those clients, and still lets the
portal capture on their behalf.

## Decision

- **kojed creates one Wayland socket per context** through
  `wp_security_context_v1` (sandbox engine `io.koje`, app id `io.koje.<context>`,
  instance id = context name) and keeps the security context alive while the
  context runs.
- The socket lives in a **per-context directory** under kojed's runtime dir,
  and that directory is mounted into the context; `WAYLAND_DISPLAY` points at
  the socket inside. A context never gets the compositor's normal socket.
- Screen sharing goes through the portal (ADR 0019, `screencast` switch), so the
  user picks what to share each time.
- `wayland = false` in the access profile (ADR 0017) means no socket at all.

## Consequences

+ Contexts cannot capture the screen behind the user's back; video calls still
  work.
+ The mounted directory also fixes stale sockets (M3.11): after a compositor
  restart kojed creates a new socket in the same directory.
- Contexts depend on kojed for their Wayland socket: if kojed dies, the
  compositor stops accepting *new* connections on it (existing windows stay).
  kojed must recreate sockets when it restarts.
- Requires a compositor with `security-context-v1`; koje checks for it (M4.9
  `koje doctor`).
- Further privileged protocols are withheld too (whatever Hyprland considers
  privileged for sandboxed clients); watch for apps that miss one.
