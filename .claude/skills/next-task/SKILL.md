---
name: next-task
description: Pick the next open task from the koje plan, implement it with tests, verify with `just check`, and update the plan. Use when asked to continue implementation or work on the next task.
---

# Work on the next plan task

1. Read `docs/plan/README.md` (conventions) and `docs/plan/roadmap.md` to find the
   current milestone: the first one whose file still has open tasks `- [ ]` and
   whose dependencies are done.
2. In that milestone file, pick the first task that is `- [ ]` and not blocked.
   If the user named a task ID (e.g. `M1.3`), take that one instead.
   If the task is a spike (needs the host's Hyprland or Podman), stop and tell the
   user it must run on the host — then help by preparing commands and the report
   skeleton in `docs/spikes/`.
3. Restate the task, its "Done when" criteria and your approach in a few lines.
   If the task needs a decision that is not covered by `docs/decisions/` or the
   concept, ask the user instead of guessing; record the answer with the
   `record-decision` skill.
4. Mark the task `- [~]` (in progress) in the milestone file.
5. Implement in small steps. Pure logic goes into `koje-core` with unit tests;
   I/O goes behind the traits in `koje-core/src/ports.rs`. Follow `AGENTS.md`.
6. Run `just check`. Fix everything until it passes. Do not disable lints or
   tests to get green.
7. Mark the task `- [x]`, add a one-line note under it if something is worth
   knowing later (follow-ups, surprises). Add new tasks you discovered at the
   end of the milestone with the next free ID.
8. Summarize what changed and propose a commit message in the form
   `M1.3: <imperative summary>`. Do not commit unless asked.
