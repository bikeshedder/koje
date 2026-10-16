# Spike reports

One file per spike, named after the plan task: `M2.3-context-container.md`.
Spikes run on the host; scripts, the spike image and the runbook are in
[setup/](setup/README.md). Keep reports factual – they are the input for ADRs and for
breaking down later milestones.

## Template

```markdown
# M2.x – Title

- Date: YYYY-MM-DD
- Host: distribution, kernel, Hyprland version, Podman version, GPU

## Question

What did we want to find out?

## Setup

Exact commands and config snippets, so the result can be reproduced.

## Result

What happened. Measurements with units. Screenshots in `docs/spikes/img/` if useful.

## Conclusion

What this means for koje. Link the ADR(s) written, the plan tasks added or changed,
and the open questions answered.
```
