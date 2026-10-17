# AGENTS.md – koje

Instructions for coding agents (and humans) working in this repository.

## What koje is

koje isolates projects into **contexts** on Linux with Hyprland. Each context is a
rootless Podman container that runs all programs for that work (editor, browser,
terminal, …) plus its own nested Podman for dev containers. Hyprland shows the
windows of all contexts on one desktop; koje colors each window border in its
context's color, switches named **layouts**, stashes unused windows, saves and
restores **states**, pauses contexts and enforces memory limits.

The full concept (German) is `docs/concept/koje-konzept.pdf` (source: `.typ`).
An English summary is `docs/architecture.md`. Settled decisions are in
`docs/decisions/`, unsettled ones in `docs/open-questions.md`.

## Terminology – use these words consistently

| Term | Meaning |
|---|---|
| context | The unit koje manages: one container, name, color, states, limits. CLI argument `<context>`. |
| project | The user's work (repos, documents). koje has **no project object**; a context mounts folders. |
| container | Only the technical implementation: *context container*, *dev container*, compose container. |
| state | Named snapshot of a context: which windows run + their layouts. Roles: `default`, `fresh`; reserved: `empty`. |
| layout | Named arrangement of a context's running windows. Part of a state. |
| stash | Hidden special workspace holding running windows not in the active layout. |
| overlay | Hyprland special workspace for cross-context apps (mail, chat) in context `global`. |
| access profile | Which host resources a context gets (`gpu`, `sound`, `wayland`, `nested`, `network`): a profile (`full`, `desktop`, `headless`, `none`) plus overrides. ADR 0017. |
| integration | Optional description of how koje launches/remembers a program. Programs work without one. |
| koje / kojed | The CLI / the background service. Never use "koje" for a context. |

## Repository layout

```
crates/koje-core   pure domain logic: names, config, states, layouts, limits, paths, ports (traits)
crates/koje        CLI (clap). Thin: parse → call core / talk to kojed
crates/kojed       background service (Hyprland events, colors, memory watch) – stub until M4
docs/plan          roadmap + one file per milestone with task checkboxes
docs/decisions     ADRs (numbered); docs/open-questions.md for undecided items
docs/spikes        reports of experiments run on the host (Hyprland, Podman)
docs/concept       the concept document (German, Typst + PDF)
.devcontainer      dev environment (Rust, just, Podman, Claude Code)
```

## Commands

Always run `just check` before saying a change is done. It runs `cargo fmt --check`,
`cargo clippy --workspace --all-targets -D warnings` and `cargo test --workspace`.

| Task | Command |
|---|---|
| all checks | `just check` |
| tests (filter) | `just test layout` |
| run CLI | `just koje list` |
| format | `just fmt` |

## How to work here

- **Follow the plan.** Find the current task in `docs/plan/` (see its README). Mark
  it `[~]` when starting and `[x]` when done; add discovered follow-ups as new tasks.
  The `next-task` skill does this.
- **Decisions are recorded.** If you settle a design question, write an ADR (`record-decision`
  skill). If something is unclear and not covered by an ADR or the concept, ask —
  do not invent product behavior.
- **Keep the CLI in sync.** `crates/koje/src/cli.rs`, the command table in
  `docs/architecture.md` and ADR 0005 describe the same commands. Change them together.
- **Small, reviewable changes.** One task per change; commit message `M1.3: <summary>`.

## Architecture rules

- `koje-core` does no I/O against Hyprland, Podman, D-Bus or the network. File
  reading of koje's own config/state files is fine. Everything else goes through
  the traits in `koje-core/src/ports.rs`, so logic is testable with fakes.
- Pure functions for decisions (e.g. `state::resolve_load`, `layout::plan_switch`,
  `memory::classify`); thin adapters execute them.
- Containers are driven with Podman directly (no Distrobox). Rootless only.
- Hyprland is the only supported compositor; talk to it via its command and event
  sockets. Do not add abstractions for other compositors.
- Programs inside contexts are never modified or configured by koje. Integrations
  only influence how koje launches them and what it remembers.
- File locations follow XDG; always go through `koje_core::paths::Paths`.

## Rust conventions

- Edition 2024, MSRV 1.88 (let-chains). Stable toolchain from `rust-toolchain.toml`.
- Libraries: typed errors with `thiserror`. Binaries: `anyhow` with context
  (`.with_context(|| …)`). No `unwrap`/`expect` outside tests (clippy warns; CI denies).
- Validate at the edges: config and state files are parsed into validated types
  (`ContextName`, `Color`, `ByteSize`, …); `#[serde(deny_unknown_fields)]` for files users write.
- Logging with `tracing`; user-facing output via `println!` in the CLI only.
- Prefer `BTreeMap`/`BTreeSet` where order is visible (files, output) for stable diffs.
- Every bug fix gets a regression test. New logic gets unit tests in the same module.

## Testing and safety

- Unit tests must not need Hyprland, Podman, a display or network.
- CLI integration tests (`crates/koje/tests/`) run the binary with temporary
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`. Never read or write the real
  user config in tests.
- Tests that need real Podman or Hyprland must be `#[ignore]` with a reason and
  run explicitly (`just test -- --ignored`). They may only touch containers
  labelled `io.koje.test=true` and must clean up after themselves.
- Never run destructive commands against the host's containers or Hyprland session
  (`podman rm -a`, `podman system reset`, `hyprctl dispatch exit`, killing windows).

## Documentation language

Code, comments, docs and commit messages in English. The concept document stays
German; user-facing CLI text is English until the language question is decided.
