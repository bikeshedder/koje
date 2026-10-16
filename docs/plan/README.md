# Plan

The plan is a set of Markdown files that humans and agents keep up to date in the
same change as the code.

- [roadmap.md](roadmap.md) – milestones, order, exit criteria
- `M<n>-<topic>.md` – one file per milestone with tasks

## Task format

```markdown
- [ ] **M1.3 Load and save states**
  - Done when: `state::load`/`save` round-trip the concept example; atomic write; tests.
  - Notes: depends on M1.2.
```

| Mark | Meaning |
|---|---|
| `- [ ]` | open |
| `- [~]` | in progress (one task at a time per person/agent) |
| `- [x]` | done – add a short note if something is worth knowing later |
| `- [-]` | dropped – say why (and link an ADR if a decision caused it) |

Task IDs never change and are never reused; new tasks get the next free number at
the end of the milestone. Commit messages start with the task ID: `M1.3: …`.

## Kinds of tasks

- **Build** – code + tests, verified with `just check`.
- **Spike** – an experiment on the host (real Hyprland, real Podman). Output is a
  report in `docs/spikes/` (template there) and, if something got decided, an ADR.
  Spikes are marked *(host)*; agents in the dev container prepare them but the user
  runs them.

Later milestones are deliberately coarse. Break them into concrete tasks when the
milestone starts, informed by the spike results.
