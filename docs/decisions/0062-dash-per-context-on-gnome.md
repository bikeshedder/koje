# 0062. GNOME's dash shows the current context only

- Status: Accepted
- Date: 2026-10-30
- Source: discussion with the user (plan [G](../plan/G-gnome.md), G.32);
  ADR 0053's dash rules, superseded with the Hyprland path (ADR 0061)

## Context

G.10 put a context's programs into GNOME's app grid and search on its
workspace, but the dash stayed GNOME's own: the host's favorites
(`org.gnome.shell favorite-apps`) and every running app of the desktop. In a
context's dash, host apps and other contexts' apps are noise, and a host
favorite the context lacks starts nothing there. ADR 0053 solved this for the
Hyprland dash; it went with that path (ADR 0061), but its `favorites` in
`context.toml` and `koje favorites` stayed.

GNOME draws the dash from `AppFavorites` and `Shell.AppSystem.get_running()`.
A context's window maps to the host's app of the same app id (ADR 0059, L3),
so filtering GNOME's running apps by workspace alone would show host apps in a
context's dash and context windows in the host's.

## Decision

- **A dash per context.** On a context's workspace the dash shows that
  context's favorites and its running apps; on a host workspace the host's.
- **No crossing.** A context's dash holds only that context's programs (its
  `koje-<context>-<id>` apps, G.10), never a host app or another context's.
  The host's dash holds only host apps, never a context's program or an app
  that is running only because of a context's windows. A window belongs to the
  context of its `koje-<context>` tag (ADR 0059, L2); untagged windows are the
  host's.
- **Running apps are the windows of the dash's owner.** Other contexts'
  running apps are hidden. In a context's dash a context window is shown as the
  context program it matches (its `StartupWMClass`, its id or the id's last
  part, its name or program, as ADR 0053), and its running indicator and window
  list count only that context's windows. The host's dash counts only host
  windows. A context window that matches no program shows with a generic icon.
- **Favorites per context.** `favorites` in `context.toml`: desktop file ids
  of the context's programs, in order, pinned, unpinned and moved from the dash
  (and `koje favorites`). The first change from a context's dash writes its
  list; from then on it is the context's own. On the host, pinning stays
  GNOME's `favorite-apps`.
- **Unset, GNOME's favorites filtered by what the context has installed.**
  Each of GNOME's favorites maps to the context's program by the same matching
  as running apps; favorites the context does not have are left out. A pinned
  id the context no longer has is left out too, and kept in the file.

- **Only the dash** (added 2026-10-30, user): Alt+Tab and the overview keep
  GNOME's apps; Alt+Tab's "current workspace only" setting already does what
  is right there. A host app's window on a context's workspace is left out of
  that dash. A context's dash icon is the context's instance: its menu's "New
  Window", window list and "Quit" act in that context only.

## Consequences

+ A click in a context's dash starts or shows what is in that context.
+ Each context has a useful dash without setting anything up.
- The dash cannot use GNOME's app objects as they are: running state and
  window lists are per app, and L3 puts the windows of several contexts and the
  host into one app. The extension has to give the dash per-context apps with
  their own windows, which reaches further into GNOME internals than G.10
  (`AppFavorites`, the dash's running apps, `AppIcon`'s windows). They go into
  `docs/upstream/gnome-apis.md`; a window → app hook (L3) would make most of it
  unnecessary.
- The matching is a heuristic: a program with an unusual id, name and binary
  is left out of the fallback favorites and its windows show a generic icon.
- kojed's `io.koje.Daemon1` gains the favorites (read and write).
- Follow-up: G.32.
