# G.5 – App identity across contexts

- Date: 2026-10-29
- Host: **GNOME lab** in the dev container ([gnome-lab](setup/gnome-lab/README.md)): GNOME Shell 50.1 (Ubuntu 26.04 packages), software rendering; contexts from `localhost/koje-spike/base` (Fedora 44, foot 1.x)

## Question

GNOME maps windows to apps by app id (`Shell.WindowTracker`). What happens
with the same program in two contexts and on the host, and do exported
`.desktop` files with distinct ids help (L3, L5 in ADR 0059)?

## Setup

In the lab instead (dev container, [gnome-lab](setup/gnome-lab/README.md)): `./lab.sh start`, then use
`./lab.sh context a` for `run-context.sh a wayland-0`, and `./lab.sh run foot` for the host's `foot &`. Exported `.desktop` files go into the lab: `./lab.sh exec sh -c 'mkdir -p ~/.local/share/applications && cat > …'`. Use `./lab.sh eval '…'` instead of Looking Glass. Note in the
report that it ran in the lab.

```sh
docs/spikes/setup/run-context.sh a wayland-0
docs/spikes/setup/run-context.sh b wayland-0
podman exec -d koje-spike-a foot; podman exec -d koje-spike-b foot
foot &    # host
# 1. Note: dash icons, running dots, right-click window list, Alt+Tab, Super+Tab grouping.
# 2. Export entries and repeat:
mkdir -p ~/.local/share/applications
for c in a b; do
  sed -e "s|^Exec=.*|Exec=podman exec -d koje-spike-$c foot|" \
      -e "s|^Name=.*|Name=Foot ($c)|" \
      -e "\$aStartupWMClass=foot" \
      /usr/share/applications/foot.desktop > ~/.local/share/applications/koje-spike-$c-foot.desktop
done
# 3. In Looking Glass (Alt+F2, lg):
#    global.get_window_actors().map(a => [a.meta_window.get_wm_class(), a.meta_window.get_pid(), Shell.WindowTracker.get_default().get_window_app(a.meta_window).get_id()])
rm ~/.local/share/applications/koje-spike-*-foot.desktop
```

## Result

Run by the agent with `lab.sh`; titles mark where each window comes from
(`foot -T ctx-a`). "App" is `Shell.WindowTracker.get_window_app()`. "Running" is
`Shell.AppSystem.get_running()`, which is what the dash's running apps and
Alt+Tab (`app-switcher current-workspace-only` is `false` by default) are
built from. "Search" is `Shell.AppSystem.search("foot")`.

| Step | Windows (app id → app) | Running apps | Search "foot" |
|---|---|---|---|
| 1. Nothing exported | ctx-a, ctx-b, host: `foot` → `foot.desktop` | one: Foot with 3 windows | `foot.desktop`, … |
| 2. `koje-spike-{a,b}-foot.desktop`, `StartupWMClass=foot` | ctx-a, ctx-b, host: `foot` → `foot.desktop` | one: Foot with 3 windows | Foot (a), Foot (b), Foot, … |
| 3. Context foot started with `-a koje-<c>-foot`, `StartupWMClass=koje-<c>-foot` | ctx-a → `koje-spike-a-foot.desktop`, ctx-b → `koje-spike-b-foot.desktop`, host `foot` → `foot.desktop` | three: Foot (a), Foot (b), Foot – one window each | Foot (a), Foot (b), Foot, … |

- **Step 2 does not help:** an app id that equals a `.desktop` file's name
  (`foot` → `foot.desktop`) wins over `StartupWMClass`.
- **Step 3 separates the contexts** in the window tracker, the running apps
  and the dash (`Main.overview.dash`: Files, Foot, Foot (b), Foot (a)).
- **Stale mapping:** right after an exported file's `StartupWMClass` changed
  from `foot` to `koje-a-foot`, a new host foot (`foot`) went to
  **Foot (a)** while Foot (a) was running (2 of 2 tries; 0 of 8 tries without
  that change). GNOME keeps the old `StartupWMClass` mapping for a while.
- Seen in the lab window (user, step 3): the dash shows three foot icons
  with running dots, and Alt+Tab shows three entries (Foot, Foot (a), Foot (b)).
- Not checked: clicking the exported entries (their `Exec=foot` would start a
  lab foot, the lab has no `koje run`).

## Conclusion

- L3 confirmed: by default GNOME treats the same program in several contexts
  and on the host as one app. Exported `.desktop` files alone do not change
  that.
- It is solved **when a context's program gets its own app id** and koje
  exports an entry with that `StartupWMClass`. Few programs let the caller
  set the Wayland app id (foot `-a`; Firefox `--name`, others to check).
  GTK/libadwaita apps use their application id. Changing it would mean
  modifying the program, which koje does not do. So for most apps this is no
  general answer.
- Options for the rest, to decide before G.10:
  1. accept the grouping and patch activation to prefer the current workspace (ADR 0059's plan);
  2. a koje-owned window → app mapping in the extension, replacing what the dash, Alt+Tab and the overview read from `Shell.WindowTracker` (more internals);
  3. the upstream ask (a window → app hook, or a per-connection app id from security-context-v1, `docs/upstream/gnome-apis.md`).
- koje must write an exported entry's `StartupWMClass` once and not change
  it, or GNOME may map windows to the wrong app until it settles.
