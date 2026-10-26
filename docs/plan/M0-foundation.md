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
- [x] **M0.7 Push to a Git host and get CI green**
  - Done when: repository URL set in `Cargo.toml`; CI workflow passes (check + MSRV job).
  - Notes: first CI run failed on `clippy::assert_is_empty` (new in clippy 1.99; CI and the dev container track stable). Fixed in `layout.rs` tests; CI green since run 38919341671.
- [x] **M0.8 Podman inside the dev container**
  - Done when: `podman info` works in the dev container, so `cargo test -p koje-podman -- --ignored` runs there too. Same cause as M2.5: `newuidmap` lacks its file capabilities in the image (restore them in the Dockerfile), plus `/dev/fuse` (commented in `devcontainer.json`).
  - Verified 2026-10-26 after a rebuild: native overlay storage on the volume; `lifecycle`, the spec test and the base image test pass (`koje image build` takes under a minute here). Changes: file capabilities instead of setuid, sub-ids `1–999,1001–65535` (Debian's `100000+` lies outside the dev container's id range), `catatonit` for `--init`, `/dev/fuse` and `/dev/net/tun`, ADR 0014's inner `containers.conf` without `utsns = "host"` (conflicts with koje's `--hostname`), a volume for Podman's storage. The dev container's cgroups are read-only: the memory, pid mapping and pause checks moved into `cgroups_memory_pid_mapping_and_pause`, host only (`--skip cgroups`). The isolated-network check reads `/proc/net/dev`: nested, `/sys` is the outer one's.
- [~] **M0.9 Debian package** (ADR 0045): `just deb` in the dev container builds `koje`, `kojed`, `koje-shell` and the static `koje-forward` and packages them with `cargo-deb` (`target/debian/koje_<version>_amd64.deb`; version `0.1.0+git<date>.<commit>`); `/usr/bin`, `/usr/lib/koje`, `/usr/lib/systemd/user`, the font in `/usr/share/fonts`; `Depends` from `dpkg-shlibdeps` plus Podman's, `Recommends` the session; the snippet, Waybar config and units call `koje` and `/usr/lib/koje/…`; `koje setup` no longer installs binaries, font, helpers and unit, removes koje's old files in `~/.local` and `~/.config/systemd/user/kojed.service`; `koje doctor` checks the package; `install.sh` and `just install` removed; docs updated
  - Done when: `just check` green; the package installs on the host with `apt install ./…deb`, `koje setup` cleans up the old layout, kojed starts from `/usr/bin`.
  - Done in code (awaiting the host test): `[package.metadata.deb]` in `crates/koje/Cargo.toml`, `just deb`, `scripts/deb-version.sh`; cargo-deb refuses paths into other target folders, so `just deb` stages the musl forwarder in `target/deb-staging/`. `koje setup`: `Mark::Remove` for `koje_core::setup::old_install` (a kojed unit only when it is koje's old one), `Refresh::Kojed` when the running kojed's executable is not `/usr/bin/kojed`; `koje doctor` checks the package. CLI tests use a fake `systemctl`, never the real user services. On trixie, apt resolves the package's dependencies (`--simulate`). To check on the host: `sudo apt install ./target/debian/koje_*.deb`, `koje setup` removes the `~/.local` files and restarts kojed from `/usr/bin`, `koje doctor`, Super+Tab/Super+D/Super+F1 run the helpers from `/usr/lib/koje/`, a context gets its host interfaces (forwarder).
- [ ] **M0.10 Release the package from CI** (ADR 0045): a job builds the package on tags and attaches it to a GitHub release
