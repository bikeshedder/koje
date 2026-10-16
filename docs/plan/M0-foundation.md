# M0 – Foundation

Goal: a repository in which humans and agents can start implementing right away.

- [x] **M0.1 Cargo workspace with `koje-core`, `koje`, `kojed`**
  - Done when: workspace builds; lints configured (clippy pedantic, no unwrap outside tests).
- [x] **M0.2 Core domain types with tests**
  - Done when: names, color, byte sizes, context config, states, layouts, memory levels, paths parse/validate the concept examples.
  - Notes: `layout::plan_switch`, `state::resolve_load`, `memory::classify` already implement concept rules.
- [x] **M0.3 CLI command tree**
  - Done when: all commands from the concept parse; flag conflicts (`down -s` vs `--no-save`) enforced; unimplemented commands name their milestone.
- [x] **M0.4 Dev container, justfile, CI, Claude Code setup**
- [x] **M0.5 AGENTS.md, architecture summary, ADRs, open questions, plan**
- [x] **M0.6 Verify the dev container on the real host**
  - Done when: "Reopen in Container" works with rootless Podman; files created in the container are owned by the host user; `just check` passes; `claude` login survives a rebuild.
  - Notes: if `--userns=keep-id:uid=1000,gid=1000` fails on the host's Podman version, fall back to `--userns=keep-id` and adjust `remoteUser`.
  - Verified 2026-10-16: development runs inside the dev container on the host.
- [~] **M0.7 Push to a Git host and get CI green**
  - Done when: repository URL set in `Cargo.toml`; CI workflow passes (check + MSRV job).
  - Notes: first CI run failed on `clippy::assert_is_empty` (new in clippy 1.99; CI and the dev container track stable). Fixed in `layout.rs` tests.
