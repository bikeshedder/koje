@AGENTS.md

## Claude Code specifics

- Project skills: `/next-task` (implement the next plan task end to end) and
  `/record-decision` (write an ADR). Prefer them over ad-hoc workflows.
- You run inside the dev container. Host-only work (Hyprland, real context
  containers, spikes in `docs/plan/M2-spikes.md`) cannot be done here: prepare
  commands and report skeletons and ask the user to run them on the host.
- `.claude/settings.json` pre-approves build, test and lint commands. Ask before
  anything that touches Podman or Hyprland state.
