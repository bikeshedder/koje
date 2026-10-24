# 0038. Dotfiles presets: shared, read-only configuration for contexts

- Status: Accepted
- Date: 2026-10-24
- Source: discussion (one foot configuration for all contexts)

## Context

Every context has its own home volume (ADR 0009), so each program's
configuration (foot's font size, shell, editor) has to be set up once per
context. Mounting the host's own config folders (`~/.config/foot`, with
`unsafe = true`) would share them, but read-write: a context could change the
host's configuration – for foot even the program it starts (`shell=`), an
escape from the context – or the other contexts'.

## Decision

- **Presets** are folders in `~/.config/koje/dotfiles/<name>/` (a preset may be
  a link to a repository), laid out like a home: `.config/foot/foot.ini`,
  `.bashrc`, …
- A context lists the presets it uses in `context.toml`:
  `dotfiles = ["base", "dev"]`. Each is mounted **read-only** at
  `~/.dotfiles/<name>`, live like the other mounts (ADR 0033): a bind made
  read-only in a private staging folder next to the home volume and bound
  from there (spike M2.12 round 2 – a remount would not reach the running
  container). Changes on the host are visible at once; the context cannot
  change the presets.
- koje **links** the presets into the home like GNU Stow: a folder only one
  preset provides becomes one link (`~/.config/foot → ~/.dotfiles/base/.config/foot`),
  a folder several presets provide – or that the home has as a real folder –
  becomes a real folder with links inside. **Later presets win.** Top-level
  folders (`.config`, `.local`) always stay real, because programs write their
  own files there and the presets are read-only. A top-level `.git` is
  ignored.
- **The home's own files win:** where the context already has a real file or
  folder (or a link of its own) and a link should go, koje keeps it and says
  so. Links koje made and no preset wants any more are removed; koje keeps
  their list in `~/.local/state/koje/<context>/dotfiles-links`.
- Linking happens when the context starts (`koje up`, before the state's
  programs start), after `koje edit` of a running context, and with
  `koje dotfiles` (e.g. after adding a file to a preset): in every running
  context, or one with `-c <context>`; stopped ones link when they start.
- The decisions are pure (`koje_core::dotfiles`); koje probes the home and
  runs the steps with `sh` in the context.

## Consequences

+ One configuration for many contexts, changed in one place; the contexts
  cannot change it, for the host or for each other.
+ Contexts still differ where they want to: a real file in the home wins.
- koje creates links in the context's home – the first thing koje changes
  inside a context. It only links the user's own files, never configures a
  program itself.
- A program that writes into a linked folder (e.g. a lock file next to its
  config) fails there: such a folder belongs in a preset as single files or
  not at all.
- New files in a preset need `koje dotfiles` (or the next start) to be linked;
  changed files are seen at once.
