# koje

**Every project in its own bunk – isolated, color-coded, there and gone at a keystroke.**

koje (German for a ship's bunk) runs each project in its own isolated **context** on
Linux with [Hyprland](https://hypr.land): a rootless Podman container holding all
programs for that work – editor, browser, terminal – plus its own nested Podman for
dev containers. Every window carries its context's color, named layouts rearrange
windows instantly, and a context can be paused, stopped and restored exactly as you
left it. The programs themselves stay unchanged.

> **Status:** early development. Only `koje list` works; every other command tells
> you which milestone delivers it. See [the roadmap](docs/plan/roadmap.md).

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
