# koje

**Every project in its own bunk – isolated, color-coded, there and gone at a keystroke.**

koje (German for a ship's bunk) runs each project in its own isolated **context** on
Linux with [Hyprland](https://hypr.land): a rootless Podman container holding all
programs for that work – editor, browser, terminal – plus its own nested Podman for
dev containers. Every window carries its context's color, named layouts rearrange
windows instantly, and a context can be paused, stopped and restored exactly as you
left it. The programs themselves stay unchanged.

> **Status:** early development. Contexts start and stop (`koje up -s empty`,
> `down`, `shell`, `open`, `sleep`, `wake`), kojed colors their windows and keeps
> them on their workspace (`koje focus`); states and layouts are next. Commands
> that are not there yet name the milestone that delivers them. See
> [the roadmap](docs/plan/roadmap.md).

## Install (on the host)

koje ships as a Debian package for Debian/Ubuntu on amd64
([ADR 0045](docs/decisions/0045-debian-package.md)). Build it in the dev
container, install it on the host, then run setup as your user:

```sh
just deb                                            # in the dev container
sudo apt install ./target/debian/koje_*_amd64.deb   # on the host
koje setup --dry-run                                # show what setup would change
koje setup                                          # asks once before changing anything
```

`scripts/install-deb.sh` (or `just install`) does the host part in one step: it
installs the newest package in `target/debian/` – again if the version is the
same – and runs `koje setup`; `--yes` asks nothing.

The package puts `koje`, `kojed` and `koje-shell` into `/usr/bin`, the static
`koje-forward` and koje's helper scripts into `/usr/lib/koje/`, the systemd user
units and the icon font into their system folders, and pulls in Podman and the
libraries koje needs. `koje setup` does the per-user part: it shows its plan and
asks once, installs koje's Hyprland snippet below `~/.local/share`, adds one
`source` line to `~/.config/hypr/hyprland.conf` – or writes that file, with your
system's keyboard layout, if you have no own config (the snippet is a complete
session: bar, notifications, lock, Super+Return terminal in the active context,
workspaces – [ADR 0037](docs/decisions/0037-koje-ships-the-session.md); put your
monitor and keyboard settings below that line), installs koje's Waybar config
only if you have none, builds the base image, removes what the old `install.sh`
left in `~/.local`, and restarts kojed when it runs an old binary. Inside
Hyprland it then reloads the config and starts the session's services (Waybar,
mako, …) that do not run yet. Run it again after every upgrade; it updates
koje's files and leaves yours alone. `koje doctor` checks the result and lists
missing system packages. Details: [ADR 0021](docs/decisions/0021-installer.md).

koje's bar lists the current workspace's windows; clicking one needs a Waybar
newer than 0.15.0, which no release has yet. `scripts/build-waybar-deb.sh`
builds one from upstream git as a `.deb` for your Debian/Ubuntu and installs it.

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
4. In Claude Code, run `/next-task` to start on the first open task of milestone M1.

## Layout

```
crates/koje-core   domain logic (no I/O to Hyprland/Podman)
crates/koje        CLI
crates/kojed       background service
docs/              concept, plan, decisions, spikes
```

## License

Not decided yet (see [open questions](docs/open-questions.md)).
