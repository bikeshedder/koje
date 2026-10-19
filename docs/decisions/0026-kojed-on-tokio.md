# 0026. kojed runs on tokio

- Status: Accepted
- Date: 2026-10-19
- Source: plan task M4.1, discussion

## Context

kojed waits on several sources at once: Hyprland's event socket, requests from
`koje` and the UI, timers (autosave, recovery file, memory checks), and later
the Wayland connection for security-context sockets. `zbus` and `tokio` are
already dependencies (`koje-forward`, ADR 0025).

## Decision

- kojed uses one **tokio current-thread runtime**; work that blocks (Podman CLI
  calls) runs in `spawn_blocking`.
- The ports in `koje-core` (`Compositor`, `ContainerRuntime`) become `async fn`
  traits where kojed needs them asynchronously; the decision logic in the core
  stays pure and synchronous.
- The `koje` CLI stays synchronous (zbus blocking API to talk to kojed).

## Consequences

+ One event loop, no thread synchronization; the same stack as `koje-forward`.
- Async adapters are tested with tokio test runtimes; fakes need `async fn`s.
