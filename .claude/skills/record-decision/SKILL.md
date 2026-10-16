---
name: record-decision
description: Record an architecture or product decision for koje as an ADR in docs/decisions and update open questions and plan. Use when a design question gets settled during work or the user says "let's decide/record that".
---

# Record a decision (ADR)

1. Check `docs/decisions/README.md` for the next free number and the index.
2. Copy `docs/decisions/template.md` to `docs/decisions/NNNN-short-slug.md`.
3. Fill in Context, Decision, Consequences in short, plain English. Link the
   concept section, spike report or plan task that led to it.
4. If the decision supersedes an older ADR, set the old one's status to
   `Superseded by NNNN` and link both ways.
5. Add the ADR to the index in `docs/decisions/README.md`.
6. If it answers an item in `docs/open-questions.md`, remove the item there
   and reference the ADR in the commit message.
7. If it changes planned work, adjust the affected tasks in `docs/plan/`.
8. If it changes something stated in `AGENTS.md` (terminology, rules,
   architecture), update `AGENTS.md` in the same change.
