# Development

## Host requirements

| Need | For |
|---|---|
| VS Code + Dev Containers extension | editing, dev container |
| rootless Podman (≥ 4.3 for `keep-id:uid=`) | dev container, later the contexts themselves |
| GNOME 50 (Ubuntu 26.04) | testing on the host; `just nested-gnome` for extension work |

VS Code user setting (once): `"dev.containers.dockerPath": "podman"`.

## The dev container

`.devcontainer/` builds on `mcr.microsoft.com/devcontainers/rust:1-trixie` and adds
`just`, Podman, `jq`, `ripgrep`, the Claude Code feature and the GitHub CLI.

- The host user is mapped to `vscode` (uid 1000) via `--userns=keep-id:uid=1000,gid=1000`,
  so files created in the container belong to you on the host.
- Named volumes keep Claude Code's login/settings (`CLAUDE_CONFIG_DIR`), the Cargo
  registry and nested Podman's storage across rebuilds.
- `rust-analyzer` uses a separate target dir so it does not block `cargo` builds.

## Where things can be tested

| Kind | Where | How |
|---|---|---|
| Unit tests, CLI tests | anywhere (dev container, CI) | `just check` |
| GNOME adapter against the extension | dev container with the GNOME lab | `docs/spikes/setup/gnome-lab/lab.sh start`, then `just test-extension` |
| Context containers, nested Podman | host; dev container without cgroups | host: `cargo test -p koje-podman -- --ignored`; dev container: add `--skip cgroups` |
| The extension's code | dev container | `just lint-extension` (types), `just check-ui`; the GNOME lab, headless without VS Code's Wayland (`lab.sh start --headless`) |
| Full end-to-end (windows from contexts, colors, states) | host only | `just deb`, install the package, `koje setup` |

Tests needing real systems are `#[ignore]`d so `just check` stays hermetic.

### Nested Podman in the dev container

Rootless Podman works inside the dev container, set up like nested Podman in
contexts (ADR 0014): `newuidmap`/`newgidmap` with file capabilities, sub-ids
inside the dev container's id range, `/dev/fuse` and `/dev/net/tun`, and
`/etc/containers/containers.conf.d/koje-dev.conf` with no cgroups, the outer
`/proc` and no default sysctls.

The dev container's cgroups are read-only, so inner containers get none: no
memory figures, no mapping of pids to contexts, no `pause`. The tests that need
them have `cgroups` in their name and run on the host only.

`base_image_runs_the_session_bus` needs `koje/base`; build it first with
`just koje image build`.

## Running koje for real

Contexts must run on the host (they need the host's Wayland, PipeWire and GPU).
Build the package inside the container, install it on the host (ADR 0045):

```sh
just deb                                            # host or dev container (Podman)
sudo apt install ./target/debian/koje_*_amd64.deb   # host, in the repository
koje setup && koje list
```

After a change: `just deb` (an Ubuntu 26.04 container builds the package – on the host or in the dev container), then `just install` on the host
(`scripts/install-deb.sh`: the newest package, reinstalled if the version is the
same, then `koje setup`; `--yes` asks nothing). It warns when the package is
older than the checkout.

The version carries the commit (`0.1.0+git<date>.<time>.<commit>`, `.dirty` with
uncommitted changes), so each commit's package is newer than the last. Two
builds of the same commit have the same version; `just install` reinstalls.

Use throwaway contexts for experiments and keep them labelled
(`io.koje.test=true`) so they are easy to clean up.

## Workflow

1. Pick a task (`/next-task` in Claude Code, or by hand from `docs/plan/`).
2. Implement with tests, `just check`.
3. Update the plan checkbox, write an ADR if you decided something.
4. Commit as `M<milestone>.<task>: <summary>`.
