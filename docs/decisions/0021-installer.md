# 0021. Installer: what it may change

- Status: Accepted (snippet scope superseded by ADR 0037)
- Date: 2026-10-18
- Source: discussion during M2.1

## Context

Setting up koje touches several places: binaries, the icon font (ADR 0020),
Hyprland's configuration, a bar configuration, system packages. Users (and the
author on a new machine) need one command that does this safely and repeatably,
before koje itself can do it.

## Decision

- **Now:** `./install.sh` in the repository (`just install` runs it). Later the
  same steps move into `koje setup` (with `koje doctor` to check), and
  `install.sh` only builds koje and calls it (M4.9).
- **Dependencies are checked, never installed:** missing ones are listed with an
  `apt install …` line (Debian/Ubuntu names; other distributions get the names).
  The installer never runs `sudo`.
- **It may write without asking per file:**
  - binaries to `~/.local/bin`, the icon font to `$XDG_DATA_HOME/fonts/koje/`,
    koje's Hyprland snippet to `$XDG_DATA_HOME/koje/hyprland/koje.conf` (koje's
    files, replaced on every install);
  - one `source = …/koje.conf` line appended to `~/.config/hypr/hyprland.conf`
    if missing (the file is created if it does not exist, never rewritten);
  - koje's Waybar config, only if the user has none – earlier koje copies
    (recognized by their header) are updated, a foreign config is left alone.
- **Nothing happens unannounced:** it first prints the plan (new, overwrite,
  append, unchanged, skipped), warns about files that will be overwritten or
  changed, and asks once (`--yes` skips the question, `--dry-run` only shows the
  plan). Running it again is safe.
- koje's own Hyprland settings live in the snippet (`assets/hyprland/koje.conf`):
  floating windows, Super+mouse, snapping, overlays, stash. Personal and machine
  settings (monitor, keyboard layout, GPU) stay in the user's `hyprland.conf`.
  Superseded by ADR 0037: the snippet carries the whole session (bar,
  notifications, lock, basic bindings, workspaces, look); only the machine's
  settings stay in `hyprland.conf`.

- Update (M4.9): the steps now live in **`koje setup`** (plan in
  `koje_core::setup`, koje's files embedded in the binary) and **`koje doctor`**
  checks the host; it also installs kojed's systemd user unit. `install.sh` only
  builds and installs `koje`, `kojed` and `koje-forward`, restarts a running
  kojed, and runs `koje setup` with the same flags.

## Consequences

+ One command for a new machine; repeatable updates.
+ The user's own configuration is never rewritten.
- Package names are only known for Debian/Ubuntu.
- Changes to the snippet take effect on `hyprctl reload`; the installer only
  reminds the user. (Since ADR 0037 it reloads and starts the session's
  services itself when it runs inside Hyprland.)
