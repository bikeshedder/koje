# 0007. Terminology: context, no project object

- Status: Accepted
- Date: 2026-10-16
- Source: concept document (`docs/concept/koje-konzept.pdf`)

## Context

"Project", "context", "container" and "koje" were used interchangeably.

## Decision

- **Context**: the unit koje manages (CLI `<context>`, `context.toml`).
- **Project**: the user's work; koje has no project object – a context mounts folders.
  Usually one project per context; related projects may share one.
- **Container**: only the technical implementation (context container, dev container).
- **koje**: the tool only. The pun "each project in its own Koje" stays in marketing text.

## Consequences

+ Unambiguous CLI and docs. See the glossary in `AGENTS.md`.
