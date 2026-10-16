# Development

## Host requirements

| Need | For |
|---|---|
| VS Code + Dev Containers extension | editing, dev container |
| rootless Podman (≥ 4.3 for `keep-id:uid=`) | dev container, later the contexts themselves |
| Hyprland | spikes (M2) and everything from M4 on – testing on the host |

VS Code user setting (once): `"dev.containers.dockerPath": "podman"`.

## The dev container

`.devcontainer/` builds on `mcr.microsoft.com/devcontainers/rust:1-trixie` and adds
`just`, Podman, `jq`, `ripgrep`, the Claude Code feature and the GitHub CLI.

- The host user is mapped to `vscode` (uid 1000) via `--userns=keep-id:uid=1000,gid=1000`,
  so files created in the container belong to you on the host.
- Named volumes keep Claude Code's login/settings (`CLAUDE_CONFIG_DIR`) and the Cargo
  registry across rebuilds.
- `rust-analyzer` uses a separate target dir so it does not block `cargo` builds.

## Where things can be tested

| Kind | Where | How |
|---|---|---|
| Unit tests, CLI tests | anywhere (dev container, CI) | `just check` |
| Hyprland adapter against a real session | host, or dev container with the Hyprland mount | `just test -- --ignored hypr` |
| Context containers, nested Podman | host, or dev container with `/dev/fuse` | `just test -- --ignored podman` |
| Full end-to-end (windows from contexts, colors, layouts) | host only | `just install`, then use `koje` |

Tests needing real systems are `#[ignore]`d so `just check` stays hermetic.

### Talking to the host's Hyprland from the dev container (optional)

Uncomment the Hyprland mount and the two env variables in
`.devcontainer/devcontainer.json`, then rebuild. The host's
`$XDG_RUNTIME_DIR/hypr` appears at `/run/host-hypr`; the Hyprland adapter should
look for the socket in `$KOJE_HYPR_DIR/$HYPRLAND_INSTANCE_SIGNATURE/`. Hyprland must
be running on the host when the container starts, otherwise the bind mount fails.

### Nested Podman in the dev container (optional, M3+)

Uncomment `--device=/dev/fuse` in `runArgs` and rebuild. Inside, `podman info`
should work rootless. This is the same setup koje needs for contexts, so problems
found here are valuable – record them in the M2 spike report.

## Running koje for real

Contexts must run on the host (they need the host's Wayland, PipeWire and GPU).
Build inside the container, install on the host:

```sh
# on the host, in the repository
just install          # cargo install into ~/.cargo/bin
koje list
```

Use throwaway contexts for experiments and keep them labelled
(`io.koje.test=true`) so they are easy to clean up.

## Workflow

1. Pick a task (`/next-task` in Claude Code, or by hand from `docs/plan/`).
2. Implement with tests, `just check`.
3. Update the plan checkbox, write an ADR if you decided something.
4. Commit as `M<milestone>.<task>: <summary>`.
