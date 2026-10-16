# 0009. Refused mount targets

- Status: Accepted
- Date: 2026-10-16
- Source: plan task M1.11 (follow-up of M1.1), discussion

## Context

A context has its own home volume at the host's home path, and every entry in
`mounts` is bound at the same path as on the host (concept, "Dateien zwischen
Kontexten"). A mount therefore replaces whatever is at that path inside the
context. Some targets silently remove isolation or break the container:

- the home folder or a folder containing it: the host home hides the context's
  home volume, so all contexts share dotfiles, profiles and koje's files;
- koje's own files and the host's controls: a context could edit its own
  `context.toml` (limits, mounts), read other contexts' states, or reach the
  host's Podman and Hyprland sockets;
- system folders the image provides: the host's copy replaces the container's.

## Decision

After expansion (M1.1 rules: `~`/`$HOME`, absolute, no `..`), a mount
is refused when its path

1. **equals or contains the home folder** – `~`, `/home`, `/`;
2. **equals, contains or lies inside** a protected host path:
   koje's config, data and state dirs (as resolved by `Paths`), the host's
   container storage (`$XDG_DATA_HOME/containers`) and `$XDG_RUNTIME_DIR`
   (so also `~/.config`, `~/.local`, `/run/user/<uid>`);
3. **equals or lies inside** a system folder: `/usr`, `/etc`, `/bin`, `/sbin`,
   `/lib`, `/lib32`, `/lib64`, `/libx32`, `/proc`, `/sys`, `/dev`, `/run`, `/var`, `/boot`.
   Paths inside the home folder are exempt, so a home in `/var/home` (Fedora
   Atomic) works.

Folders below home (`~/src/kp`, `~/Austausch`) and outside it (`/srv`, `/opt`,
`/mnt`, `/media`, …) are allowed. Nested mounts are allowed.

A refused mount can be allowed per entry with the table form; the plain string
form stays the default:

```toml
mounts = [
  "~/src/kundenportal",
  { path = "~/.config/nvim", unsafe = true },
]
```

`unsafe = true` skips rules 1–3 for that entry only. The other M1.1 checks
(absolute, no `..`, no duplicates) still apply.

## Consequences

+ A typo or a too-broad folder cannot quietly undo a context's isolation.
+ Deliberate holes stay possible and are visible in the config.
- Checks are lexical: a symlink below an allowed path can still point into a
  protected one. The container adapter (M3.2) must canonicalize mount sources
  before binding and re-check the rules.
- `mounts` entries become "string or table"; `koje new`/`edit` must keep both forms.
