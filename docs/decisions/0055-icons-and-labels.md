# 0055. Workspaces, contexts and overlays have an icon and a label

- Status: Accepted
- Date: 2026-10-27
- Source: discussion after M8.80 and M8.34 ("the switcher with just the
  colored circles and pills doesn't help much"); revises the default of
  ADR 0047's workspace styles

## Context

ADR 0047 made GNOME's workspace indicator the bar's default: one dot per
workspace, a context's dot in its color. In practice the dots do not say
which workspace is which – a context is only recognizable by its color, a
host workspace not at all, an overlay (M8.34) by its place after the gap.
The `names` style shows workspace names, but host workspaces are numbers
and context names are lower-case identifiers (`kundenportal`).

## Decision

- **Everything the bar switches to has an icon and a label**:
  - host workspaces in `config.toml`, `[workspace.<number>]` with `label`
    and `icon`; unset: the number and `computer-symbolic`;
  - contexts in `context.toml`, `label` and `icon`; unset: the context's
    name and `folder-symbolic`. `prefix` stays the short label for compact
    places;
  - overlays in their `[overlay.<name>]`, `icon` beside `label`; unset:
    koje's own for `mail` and `chat`, else `view-dual-symbolic`.
  Icons are names from the icon theme (symbolic ones look right in the
  bar), never paths.
- **New default bar style `labels`**: a panel button per workspace with its
  icon and label; a context's icon in its color, the active context filled
  with its color, the active host workspace lit as a GNOME panel button; a
  running context without windows dimmed. Overlays show only their icon,
  after a gap, lit while open. `colored-dots`, `dots` and `names` stay
  selectable in `[bar] workspaces`.
- **The settings app edits them** (M8.90): a Workspaces page for the bar's
  style, the host workspaces and the overlays, and label and icon on a
  context's page, both with an icon chooser from the icon theme.
  `config.toml` is saved through the new `koje config --stdin` (checked as
  the editor path `koje config` checks it), like `koje edit --stdin` for a
  context; comments in the files are kept.

## Consequences

+ Workspaces are recognizable at a glance, without memorizing colors.
+ One model for all three kinds; the shell follows the files live
  (`config.toml` watched, the contexts' files read every 2 s).
- The bar is wider than with dots; with many workspaces labels are cut
  (16 characters) and may need a compact mode later (only the active one
  labelled).
- Host workspaces are configured by number: a Hyprland workspace created
  under another name shows that name with the host icon.
