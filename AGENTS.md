# AGENTS.md – koje

Instructions for coding agents (and humans) working in this repository.

## What koje is

koje isolates projects into **contexts** on Linux with GNOME. Each context is a
rootless Podman container that runs all programs for that work (editor, browser,
terminal, …) plus its own nested Podman for dev containers. GNOME shows the
windows of all contexts on one desktop, one workspace per context; a window
outside its context glows in the context's color with a badge naming it (ADR
0057). koje saves and restores **states**, pauses contexts and enforces memory
limits.

koje runs inside a stock GNOME session; a GNOME Shell extension adds what is
koje's own (ADR 0059). The Hyprland path is gone (ADR 0061); its history is in
git. Current work: `docs/plan/G-gnome.md`. All UI (extension, settings app) is
TypeScript on GJS, and kojed's API is D-Bus `io.koje.Daemon1` (ADR 0060).

The full concept (German) is `docs/concept/koje-konzept.pdf` (source: `.typ`).
An English summary is `docs/architecture.md`. Settled decisions are in
`docs/decisions/`, unsettled ones in `docs/open-questions.md`.

## Terminology – use these words consistently

| Term | Meaning |
|---|---|
| context | The unit koje manages: one container, name, color, states, limits. CLI argument `<context>`. |
| project | The user's work (repos, documents). koje has **no project object**; a context mounts folders. |
| container | Only the technical implementation: *context container*, *dev container*, compose container. |
| state | Named snapshot of a context: which windows run and where. Roles: `default`, `fresh`; reserved: `empty`. |
| image | Where a context's programs come from (ADR 0040, 0064): koje's built-in `fedora` or an image folder `~/.config/koje/images/<name>/` (Containerfile, `image.toml`); koje adds the user on top. Kept and listed like contexts. |
| layout | Named arrangement of a context's running windows. Part of a state's file; not switchable on GNOME yet (ADR 0061). |
| access profile | Which host resources a context gets (`gpu`, `sound`, `wayland`, `nested`, `network`, …): a profile (`full`, `desktop`, `headless`, `isolated`) plus overrides; also host interfaces (`notifications`, `file_chooser`, …). ADR 0017, 0019. |
| integration | Optional description of how koje launches/remembers a program. Programs work without one. |
| dotfiles preset | Folder in `~/.config/koje/dotfiles/<name>` mounted read-only into contexts that list it and linked into their home (ADR 0038). |
| extension | koje's GNOME Shell extension (`gnome/src/extension`, built into `gnome/build/extension/koje@koje.io`): glow and badge, sleep overlay, indicator, context apps; serves `io.koje.Extension1`. |
| settings app | `io.koje.Settings`, koje's own settings (TypeScript on GJS, ADR 0060). |
| koje / kojed | The CLI / the background service. Never use "koje" for a context. |

## Repository layout

```
crates/koje-core   pure domain logic: names, config, states, layouts, limits, paths, ports (traits)
crates/koje        CLI (clap). Thin: parse → call core / talk to kojed
crates/kojed       background service (windows and contexts, colors, states, memory watch; D-Bus API)
crates/koje-gnome  GNOME adapter (ADR 0059): Compositor port over koje's GNOME Shell extension
crates/koje-podman Podman adapter (ContainerRuntime), base image
crates/koje-runtime driving context containers (up, down, mounts, host bus, limits); kojed runs the lifecycle, the CLI shell/run/open
crates/koje-forward static D-Bus forwarder inside every context (ADR 0025)
gnome              koje's UI (ADR 0060): the GNOME Shell extension (gnome/src/extension), the settings app (gnome/src/settings, TypeScript + Blueprint), the D-Bus client generated from kojed's interface file (gnome/src/api)
build             the Ubuntu 26.04 image `just deb` builds the package in (ADR 0045)
assets            files koje ships besides binaries: kojed's unit, icon, base image
scripts           host helpers outside koje itself (package build and install, nested GNOME session)
docs/plan          roadmap + one file per milestone with task checkboxes
docs/decisions     ADRs (numbered); docs/open-questions.md for undecided items
docs/spikes        reports of experiments run on the host (GNOME, Podman; older ones Hyprland)
docs/upstream      drafts of requests to upstream projects (GNOME, …)
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
| Debian package (ADR 0045) | `just deb`; on the host `just install` |
| UI (TypeScript, ADR 0060) | `just ui` builds `gnome/build`; `just check-ui` (generated types current, `tsc`, vitest, ESLint); `just settings` runs the app |
| GNOME extension | `just lint-extension` (types); `just ui`, then with the GNOME lab running (`lab.sh start --headless` without VS Code's Wayland) `just test-extension`; on the host `just nested-gnome` |

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

- `koje-core` does no I/O against the compositor, Podman, D-Bus or the network. File
  reading of koje's own config/state files is fine. Everything else goes through
  the traits in `koje-core/src/ports.rs`, so logic is testable with fakes.
- Pure functions for decisions (e.g. `state::resolve_load`, `layout::plan_switch`,
  `memory::classify`); thin adapters execute them.
- Containers are driven with Podman directly (no Distrobox). Rootless only.
- GNOME Shell is the only supported desktop (ADR 0061); kojed talks to it only
  through koje's extension (`koje-gnome`, `io.koje.Extension1`). Every GNOME
  internal the extension touches is listed in `docs/upstream/gnome-apis.md`.
- koje's UI is TypeScript on GJS (ADR 0060): the extension and the settings app.
  It talks to kojed only over `io.koje.Daemon1`, whose contract is
  `crates/kojed/dbus/io.koje.Daemon1.xml` (change it with kojed's API; `just ui`
  regenerates `gnome/src/api/daemon1.ts`). Logic without `gi://` imports lives
  in plain modules with vitest tests (`model.ts`); widgets only render it;
  nothing blocks the main loop. Run `just check-ui` besides `just check` when
  `gnome/` or the interface file changed.
- GNOME provides the desktop and its settings; koje builds no pages for system
  settings.
- Programs inside contexts are never modified or configured by koje. Integrations
  only influence how koje launches them and what it remembers. Dotfiles presets
  (ADR 0038) are the user's own files that koje only links.
- File locations follow XDG; always go through `koje_core::paths::Paths`.

## Rust conventions

- Edition 2024, MSRV 1.92 (let-chains need 1.88). Stable toolchain from `rust-toolchain.toml`.
- Libraries: typed errors with `thiserror`. Binaries: `anyhow` with context
  (`.with_context(|| …)`). No `unwrap`/`expect` outside tests (clippy warns; CI denies).
- Validate at the edges: config and state files are parsed into validated types
  (`ContextName`, `Color`, `ByteSize`, …); `#[serde(deny_unknown_fields)]` for files users write.
- Logging with `tracing`; user-facing output via `println!` in the CLI only.
- Prefer `BTreeMap`/`BTreeSet` where order is visible (files, output) for stable diffs.
- Every bug fix gets a regression test. New logic gets unit tests in the same module.

## Testing and safety

- Unit tests must not need GNOME Shell, Podman, a display or network.
- CLI integration tests (`crates/koje/tests/`) run the binary with temporary
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`. Never read or write the real
  user config in tests.
- Tests that need real Podman or GNOME Shell must be `#[ignore]` with a reason and
  run explicitly (`just test -- --ignored`). They may only touch containers
  labelled `io.koje.test=true` and must clean up after themselves.
- Never run destructive commands against the host's containers or GNOME session
  (`podman rm -a`, `podman system reset`, ending the session, killing windows).

## Documentation language

Code, comments, docs and commit messages in English. The concept document stays
German; user-facing CLI text is English until the language question is decided.
