# 0002. Drive Podman directly, no Distrobox

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

Distrobox's strength is tight integration with the host (shared home, host
commands, app export) – exactly what koje turns off. koje needs direct control over
limits, labels, nested Podman and a D-Bus proxy.

## Decision

koje creates and manages context containers with rootless Podman directly
(CLI first, REST API later). Toolbx, Bubblewrap and VMs were also considered and
rejected (too little isolation, too much manual work, too heavy).

## Consequences

+ Full control over the container spec; one less layer.
- koje must replicate what Distrobox does for GUI apps (Wayland, PipeWire, GPU) – see spike M2.3.
