# Roadmap

```
M0 Foundation ─► M1 Model & files ─────────────┐
                 M2 Spikes (host) ─► M3 Context containers ─► M4 kojed + Hyprland ─► M5 States & layouts ─► M7 Desktop extras
                                                └────────────────────────────────► M6 Memory            └► M8 UI
```

M1 and M2 can run in parallel: M1 is pure code in the dev container, M2 is
experiments on the host.

| Milestone | Goal | Exit criteria |
|---|---|---|
| [M0 Foundation](M0-foundation.md) | Repo, tooling, dev container, docs, plan | `just check` green in dev container and CI; Claude Code works in the container |
| [M1 Model & files](M1-model-and-files.md) | Contexts and states as files, no containers yet | `koje new/edit/rm/list` and `koje state …` work on files; layouts/state logic complete and tested |
| [M2 Spikes](M2-spikes.md) | Prove the risky parts on the host | Reports + ADRs for Hyprland setup, context container, nested Podman, D-Bus filter, memory sharing, file DnD |
| [M3 Context containers](M3-context-containers.md) | Real contexts with Podman | `koje up -s empty`, `down`, `shell`, `open`, `sleep`, `wake` work on the host |
| [M4 kojed + Hyprland](M4-kojed-hyprland.md) | Service that sees and colors windows | Every context window gets its border color; `koje focus` works |
| [M5 States & layouts](M5-states-and-layouts.md) | Save/restore, layouts, stash, recovery | `up`/`down` restore windows and geometry; layout switch + stash; crash recovery |
| [M6 Memory](M6-memory.md) | Limits and visibility | soft/hard limits with notifications; `koje stats`, `koje limit` |
| [M7 Desktop extras](M7-desktop-extras.md) | Overlays, pinning, snap zones | `mail`/`chat` overlays, `pin`/`unpin`, `snap` |
| [M8 UI](M8-ui.md) | Launcher, bar module, panel | Stage 1 and 2 usable daily; panel prototype |

Outlook (not scheduled): CRIU hibernation, Hyprland plugins for animations,
drag-into-zone and colored title bars – see `docs/architecture.md`.
