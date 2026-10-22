# 0035. Memory limits use the working set; no limits by default

- Status: Accepted
- Date: 2026-10-22
- Source: plan M6, spike [M2.6](../spikes/M2.6-memory-sharing.md), open question "Default limits"

## Context

A context's cgroup is charged with everything its programs and dev containers
use, including the page cache of files they read first – and that cache stays
charged after the programs exit (M2.6). Counting `memory.current` as is would
warn or shut down a context that only read many files.

## Decision

- **Usage for limits is the working set:** `memory.current` minus
  `inactive_file` from `memory.stat` – the file cache the kernel drops first
  under pressure (as `docker stats` and Kubernetes count it). `koje stats` also
  shows `memory.current` and the anonymous part.
- **Soft limit:** a desktop notification at 90 % and at 100 %, once per crossing;
  a notification comes again only after usage fell below 85 % (hysteresis).
- **Hard limit:** kojed shuts the context down in order – saves its windows
  into the state it is in (ADR 0032), ends its programs (as `koje down`), stops
  it – and notifies. If that takes too long, the container is stopped anyway.
- **Safety net:** the kernel limit `memory.max` is 10 % above the hard limit
  (the kernel then reclaims cache and, as a last resort, kills inside this
  context only).
- **Limits change live:** `koje limit` writes `context.toml` and applies the
  safety net to a running container with `podman update`; limits are not part
  of the settings fingerprint (M5.11), so they never need a recreate.
- **No limits by default:** a context without limits in `context.toml` and in
  `config.toml` (`default_limits`) has none; `koje stats` still shows its use.

## Consequences

+ Reading files does not trip limits; a runaway program does.
+ Nothing surprising happens on a fresh install.
- The working set can be lower than what `free` shows for the context.
- A hard limit means an automatic shutdown – opt-in, and saved first.
