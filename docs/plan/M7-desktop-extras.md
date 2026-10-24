# M7 – Desktop extras

- [ ] **M7.1 Overlays** from `config.toml`: special workspaces, key bindings, apps in context `global`
- [ ] **M7.2 `koje pin` / `koje unpin`** (pinned window semantics → open question)
- [ ] **M7.3 Snap zones**: `koje snap <zone>` from `config.toml`, key bindings
- [~] **M7.4 Pause with stash**: `sleep` moves windows to the stash first, `wake` restores the layout
  - Done in code (awaiting host test): D-Bus `Hide`/`Show` (`Live::hide_all` keeps the arrangement, `show_active` arranges the active layout); autosave takes positions only from windows on the context's workspace. To check on the host: does Hyprland still show "not responding" for a sleeping context's windows in the stash (ADR 0024)?
- [ ] **M7.5 Hyprland config snippet generator** (`koje hyprland-config`?) for bindings and window rules
- [ ] **M7.6 Context `global`** for overlays (from M3.10): created and started at login; how isolated it is stays an open question; whether kojed moves its windows to a workspace `global` (ADR 0028)
- [x] **M7.7 Dotfiles presets** (ADR 0038): `dotfiles = [...]` in `context.toml`, presets from `~/.config/koje/dotfiles/<name>` mounted read-only at `~/.dotfiles/<name>` (live, staging trick of M2.12) and linked stow-like into the home on `up`, after `edit`, with `koje dotfiles`
  - `koje_core::dotfiles` (scan, plan, probe/apply scripts), `Podman::live_mount_ro`. To check on the host: the mount is read-only inside (`touch ~/.dotfiles/base/x` fails), a foot preset takes effect, `koje edit` adding/removing a preset while running, `koje rm` still removes the home volume (staging unmounted).
