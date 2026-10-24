# 0018. Window colors by tag rule

- Status: Accepted
- Date: 2026-10-17
- Source: spike [M2.2](../spikes/M2.2-hyprland-ipc.md), Hyprland v0.53.3

## Context

Every window of a context gets the context's border color, for focused and
unfocused windows (concept "Kontextfarbe am Fensterrahmen"). Hyprland 0.53 offers
two ways: per-window properties (`dispatch setprop address:<a>
active_border_color …` and `inactive_border_color …`) and window rules matching a
tag (`windowrule = match:tag <t>, border_color rgb(<active>) rgb(<inactive>)`
plus `dispatch tagwindow +<t> address:<a>`). Both work.

## Decision

kojed colors windows with **one window rule per context**, matching the tag
`koje-<context>`, and **tags** every window it maps to a context (on
`openwindow`, after pid → cgroup → context). The rule carries both colors.

- kojed adds the rules at start, when a context is created or its color changes,
  and again after Hyprland's `configreloaded` event (runtime keywords are lost on
  a reload).
- A color change is one rule update; windows are not touched again.

## Consequences

+ The color lives in one place per context; new windows only need a tag.
+ Tags are visible in `hyprctl clients`, which helps debugging.
- kojed must re-add rules after config reloads and after reconnecting to
  Hyprland.
- Rules added at runtime accumulate; replacing a context's color has to remove
  or override the old rule (to check in M4.5).
- Update (M4.5 host test): a rule colors only windows opened after it. On a
  color change kojed adds the rule again (for new windows) and sets both
  border properties on the context's open windows (`setprop`, the fallback
  below).
- Fallback if tags or rules change again: per-window `setprop` for both colors.
- Update (2026-10-24): the focused window's border is a gradient from the
  context color to a lighter shade (`rgb(a) rgb(b) 45deg`, in rules and with
  `setprop`); in a rule the angle ends the active gradient, the inactive color
  follows. To check on the host: Hyprland 0.53 accepts both forms.
