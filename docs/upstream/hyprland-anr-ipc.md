# Draft: Hyprland feature request – report unresponsive windows over IPC

Status: draft, not posted yet. Post at https://github.com/hyprwm/Hyprland
(discussion or issue, per the project's contribution guidelines).

---

**Title:** Report unresponsive windows over IPC (socket2 events), independent of the ANR dialog

**Problem**

Hyprland detects unresponsive clients with `xdg_wm_base` ping/pong (and
`_NET_WM_PING` for XWayland) and shows the "Application Not Responding" dialog.
Tools that manage windows from the outside cannot see this state:

- `hyprctl -j clients` has no field for it; the only trace is the dialog itself,
  which appears as a separate `hyprland-dialog` window.
- With `misc:enable_anr_dialog = false` the information is gone completely.
- There is no window rule to suppress the dialog for specific windows.

A concrete case: a tool that freezes a group of applications on purpose
(cgroup freezer, e.g. a paused workspace or project) wants to show its own
"paused – resume" overlay instead of one ANR dialog per frozen window, while
still noticing real hangs of other applications.

**Proposal**

Emit socket2 events when a window becomes unresponsive and when it responds
again, regardless of `misc:enable_anr_dialog`:

```
windowunresponsive>>ADDRESS
windowresponsive>>ADDRESS
```

Optionally also expose the state in `j/clients` (e.g. `"responding": false`).
With these, tools can set `misc:enable_anr_dialog = false` and present the
state their own way. Existing behaviour stays unchanged.

**Alternatives considered**

- A window rule like `no_anr on` – helps against the dialog, but tools still
  cannot learn about real hangs.
- A plugin – works, but has to be rebuilt for every Hyprland version.

We would be happy to contribute a pull request if the maintainers agree with the
approach.
