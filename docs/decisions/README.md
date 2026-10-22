# Decisions (ADRs)

Numbered, never renumbered. Superseded ADRs stay with a link to their successor.
New ones: copy [template.md](template.md) or use the `record-decision` skill.

| # | Decision | Status |
|---|---|---|
| 0001 | [Hyprland is the only compositor](0001-hyprland-only.md) | Accepted |
| 0002 | [Drive Podman directly, no Distrobox](0002-podman-directly.md) | Accepted |
| 0003 | [Each context runs its own nested Podman](0003-podman-in-podman.md) | Accepted |
| 0004 | [Layouts are part of a state](0004-layouts-in-states.md) | Accepted |
| 0005 | [Named states and save/load semantics](0005-named-states-and-cli.md) | Accepted (save targets: 0032) |
| 0006 | [Crash recovery via a separate recovery file](0006-crash-recovery-file.md) | Accepted |
| 0007 | [Terminology: context, no project object](0007-terminology-context.md) | Accepted |
| 0008 | [Crate structure: pure core with ports](0008-crate-structure.md) | Accepted |
| 0009 | [Refused mount targets](0009-refused-mount-targets.md) | Accepted |
| 0010 | [State consistency on load](0010-state-consistency-on-load.md) | Accepted |
| 0011 | [Layout order and the initial layout](0011-layout-order.md) | Accepted |
| 0012 | [Defaults for new contexts](0012-new-context-defaults.md) | Accepted |
| 0013 | [State file operations and role states](0013-state-file-operations.md) | Accepted |
| 0014 | [Nested Podman in context containers](0014-nested-podman-in-contexts.md) | Accepted |
| 0015 | [Runtime environment of a context container](0015-context-runtime-environment.md) | Accepted |
| 0016 | [GPU access in context containers](0016-gpu-access.md) | Accepted |
| 0017 | [Access profiles: configurable host access per context](0017-access-profiles.md) | Accepted |
| 0018 | [Window colors by tag rule](0018-window-colors-by-tag-rule.md) | Accepted |
| 0019 | [Host interfaces through a forwarder and a filtered proxy](0019-host-interfaces-via-forwarder.md) | Accepted |
| 0020 | [Lucide as koje's icon font](0020-lucide-icon-font.md) | Accepted |
| 0021 | [Installer: what it may change](0021-installer.md) | Accepted |
| 0022 | [A security-context Wayland socket per context](0022-wayland-security-context.md) | Accepted |
| 0023 | [The base image `koje/base`](0023-base-image.md) | Accepted |
| 0024 | [One koje overlay for sleeping and hanging windows](0024-unresponsive-window-overlay.md) | Accepted |
| 0025 | [Running the host-bus proxy and the forwarder](0025-host-bus-runtime.md) | Accepted |
| 0026 | [kojed runs on tokio](0026-kojed-on-tokio.md) | Accepted |
| 0027 | [Client API: kojed on the session bus](0027-dbus-client-api.md) | Accepted |
| 0028 | [One named workspace per context](0028-context-workspaces.md) | Accepted |
| 0029 | [Sound sockets as links in a mounted directory](0029-sound-socket-links.md) | Accepted |
| 0030 | [kojed holds the live state of running contexts](0030-live-state-in-kojed.md) | Accepted |
| 0031 | [Integrations are declarative](0031-declarative-integrations.md) | Accepted |
| 0032 | [Saving goes into the state a context is in](0032-save-into-the-current-state.md) | Accepted |
| 0033 | [Mounts follow `context.toml` while a context runs](0033-live-mounts.md) | Accepted |
| 0034 | [Mounts are folders below the home](0034-mounts-below-the-home.md) | Accepted |
| 0035 | [Memory limits use the working set; no limits by default](0035-memory-accounting.md) | Accepted |
