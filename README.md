# koje

**Every project in its own bunk – isolated, color-coded, there and gone at a keystroke.**

koje (German for a ship's bunk) runs each project in its own isolated **context** on
Linux with [GNOME](https://www.gnome.org): a rootless Podman container holding all
programs for that work – editor, browser, terminal – plus its own nested Podman for
dev containers. Each context has its own GNOME workspace and color; a window
outside its context glows in that color. A context can be paused, stopped and
restored as you left it. The programs themselves stay unchanged.

> **Status:** early development, in daily use on GNOME 50 (Ubuntu 26.04). Contexts
> start, stop, sleep and restore their states; koje's GNOME Shell extension marks
> their windows and workspaces. The settings app is being rebuilt
> ([ADR 0060](docs/decisions/0060-ui-in-typescript-api-on-d-bus.md)). See
> [the plan](docs/plan/G-gnome.md).

## Install (on the host)

koje ships as a Debian package for Debian/Ubuntu on amd64
([ADR 0045](docs/decisions/0045-debian-package.md)). Build it in the dev
container, install it on the host, then run setup as your user:

```sh
just deb                                            # host or dev container (Podman)
sudo apt install ./target/debian/koje_*_amd64.deb   # on the host
koje setup --dry-run                                # show what setup would change
koje setup                                          # asks once before changing anything
```

`scripts/install-deb.sh` (or `just install`) does the host part in one step: it
installs the newest package in `target/debian/` – again if the version is the
same – and runs `koje setup`; `--yes` asks nothing.

The package puts `koje` and `kojed` into `/usr/bin`, the static `koje-forward`
into `/usr/lib/koje/`, kojed's systemd user unit and koje's GNOME Shell extension
into their system folders, and pulls in Podman and what koje needs. `koje setup`
does the per-user part: it enables the extension (log out and in once if GNOME
does not know it yet), builds the base image and restarts kojed when it runs an
old binary. Run it again after every upgrade. `koje doctor` checks the result and
lists missing system packages. Details: [ADR 0021](docs/decisions/0021-installer.md),
[ADR 0061](docs/decisions/0061-remove-the-hyprland-path.md).

## Documentation

| Document | Purpose |
|---|---|
| [docs/concept/koje-konzept.pdf](docs/concept/koje-konzept.pdf) | Full concept (German) with mockups |
| [docs/architecture.md](docs/architecture.md) | English summary of the concept |
| [docs/plan/](docs/plan/README.md) | Roadmap and milestone task lists |
| [docs/decisions/](docs/decisions/README.md) | Architecture decision records |
| [docs/open-questions.md](docs/open-questions.md) | What is not decided yet |
| [docs/development.md](docs/development.md) | Dev environment, testing, host setup |
| [AGENTS.md](AGENTS.md) | Rules for coding agents and contributors |

## Getting started (VS Code + Podman + Claude Code)

1. Host: install VS Code, the *Dev Containers* extension and rootless Podman.
   In VS Code user settings set `"dev.containers.dockerPath": "podman"`.
2. `git init` (if you got this as an archive), open the folder in VS Code and run
   **Dev Containers: Reopen in Container**.
3. In the container terminal:
   ```sh
   just check        # fmt, clippy, tests
   just koje list    # run the CLI
   claude            # sign in once; the login survives rebuilds
   ```
4. In Claude Code, run `/next-task` to start on the next open task.

## Layout

```
crates/koje-core   domain logic (no I/O to GNOME/Podman)
crates/koje        CLI
crates/kojed       background service
crates/koje-gnome  kojed's adapter to koje's GNOME Shell extension
gnome/             koje's GNOME Shell extension
docs/              concept, plan, decisions, spikes
```

## License

Not decided yet (see [open questions](docs/open-questions.md)).
