# 0060. koje's UI in TypeScript on GJS; kojed's API is D-Bus

- Status: Accepted
- Date: 2026-10-30
- Source: discussion about the settings app (2026-10-29/30); plan
  [G](../plan/G-gnome.md) G.22–G.28

## Context

On GNOME, koje-shell is not used (ADR 0059). Only the settings app stays, and
with it the reason it is written in Rust is gone: ADR 0042 put all of koje's UI
in one Rust binary. The prototype settings app is large for what it does. Most
of that comes from its design: it rebuilds its widgets every few seconds and
runs a tokio thread beside GTK. It reads koje's files through `koje-core` and
makes changes by running `koje`.

The GNOME Shell extension has to be JavaScript on GJS. A second UI language for
the settings app would mean two toolchains and two API clients for the same
data.

kojed already serves `io.koje.Daemon1` on the session bus. The interface grew
call by call: it has no interface file and many Hyprland-only members, and the
CLI calls it through raw `call_method`. The CLI's `--json` output (`list`,
`stats`, `apps`) has a single consumer: koje-shell reads `apps --json`.

## Decision

- **All of koje's UI is TypeScript on GJS.** This covers the Shell extension and
  the settings app (`io.koje.Settings`, GTK 4 and libadwaita with Blueprint
  layouts), which share one API module. TypeScript is compiled to JS with types
  from `@girs/*`. View models and other logic without `gi://` imports are plain
  TS modules tested with vitest under Node.
- **Rust keeps kojed, the CLI and everything about containers.** kojed owns the
  contexts' lifecycle (up, down, sleep, wake, recreate, save). The CLI is the
  interface for people and keyboard shortcuts, a thin client of kojed where
  kojed is involved. Commands that need no running kojed stay local (state files,
  `image build`, `setup`, `doctor`, `shell`).
- **kojed's only API is D-Bus**, `io.koje.Daemon1` with native D-Bus types. Its
  interface XML in the repository is the contract: a test checks it against
  kojed, and the TS types are generated from it. The UIs, the CLI and scripts
  use it; scripts use `busctl` or `gdbus` (`busctl --json=short` prints JSON).
  kojed gets no HTTP API.
  - Reads: contexts with their look and status, memory, states, a context's
    config, the live state's windows.
  - Changes: lifecycle, states, new, config, remove, recreate, limits. Config
    goes as `a{sv}` and is checked in kojed through `koje-core`. Errors are named
    D-Bus errors (`io.koje.Error.*`).
  - Long operations such as `up` return at once and report through signals,
    because D-Bus calls time out after 25 s.
- **The CLI loses its `--json` flags.** They are easy to add back if someone
  needs them.
- **The prototype's koje-shell code is not ported.** The settings app is written
  anew.

## Consequences

+ One UI language, one toolchain, one API client. The extension and the
  settings app share types generated from the same interface file.
+ The settings app gets short UI code: GLib async on the main loop, Blueprint
  for its layout, no thread and no clone boilerplate.
+ Scripts get a stable interface without a second API to keep in sync.
- The UI moves outside `cargo test`: TS needs its own build, lint and tests in
  `just`, and the .deb ships the built JS.
- kojed takes over the lifecycle code the CLI has today (`crates/koje/src/runtime.rs`).
- Some outputs of `koje` that people used in scripts change; `busctl` replaces
  `--json`.
- ADR 0042 no longer applies to the settings app on GNOME. Removing the
  Hyprland path, and with it koje-shell, still needs its own ADR (ADR 0059, G.14).
