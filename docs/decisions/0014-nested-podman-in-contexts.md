# 0014. Nested Podman in context containers

- Status: Accepted
- Date: 2026-10-17
- Source: spike [M2.5](../spikes/M2.5-nested-podman.md) on Ubuntu 26.04

## Context

Each context runs its own rootless Podman for dev containers and compose
projects (ADR 0003). The spike tried this inside a rootless context container
and hit, in order: missing file capabilities on `newuidmap`, setuid not working
as a replacement, no `/dev/net/tun` for pasta, read-only `/proc/sys`, a refused
fresh `/proc` mount, `sethostname` denied, and netavark failing to write
per-netns sysctls for compose networks. AppArmor and Ubuntu's user-namespace
restriction were not the cause.

## Decision

Context containers are started with:

- `--userns=keep-id` (the user keeps its uid; works at both levels)
- `--device /dev/fuse` and `--device /dev/net/tun`
- `--security-opt unmask=/proc/sys` – only `/proc/sys`; the rest of `/proc`
  stays masked. Rootless, so only per-namespace settings become writable.
- `--init`

The base image (M3.3) provides:

- `newuidmap`/`newgidmap` with file capabilities (not setuid)
- `/etc/subuid`/`/etc/subgid`: the user gets every id up to 65535 except its own
- fuse-overlayfs storage, and an inner `containers.conf` with
  `cgroups = "disabled"`, `default_sysctls = []`, `utsns = "host"`,
  `volumes = ["/proc:/proc"]`, `cgroup_manager = "cgroupfs"`, `events_logger = "file"`

Each dev container keeps its own network (pasta); unlike `quay.io/podman/stable`,
the network and IPC namespaces are not shared with the context.

## Consequences

+ Dev containers, compose and VS Code Dev Containers work inside a context;
  inner container start ≈ 0.16 s.
+ Files created in dev containers keep the user's ownership on the host.
- Dev containers see the context's processes (`/proc` bind) and share its
  hostname. Acceptable: they belong to the same context.
- `ping` in dev containers needs `--cap-add NET_RAW`.
- A shared read-only image store (`additionalimagestores`) works when a
  context's inner Podman fills it; how it is updated is still open.
