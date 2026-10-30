@AGENTS.md

## Claude Code specifics

- Project skills: `/next-task` (implement the next plan task end to end) and
  `/record-decision` (write an ADR). Prefer them over ad-hoc workflows.
- You run inside the dev container. Host-only work (the real GNOME session,
  real context containers, host spikes) cannot be done here: prepare commands
  and report skeletons and ask the user to run them on the host. The GNOME lab
  (`docs/spikes/setup/gnome-lab`) and a headless GNOME Shell run here.
- `.claude/settings.json` pre-approves build, test and lint commands. Ask before
  anything that touches Podman or GNOME session state.
