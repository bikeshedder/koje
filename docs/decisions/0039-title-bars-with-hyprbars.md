# 0039. Title bars from the hyprbars plugin

- Status: Accepted
- Date: 2026-10-24
- Source: discussion (title bars to move windows)

## Context

Hyprland draws borders but no title bars. Apps that support `xdg-decoration`
(foot, Firefox, Qt) are told the compositor decorates, so they draw none
either; only GTK apps always draw their own header bar. Moving a window then
needs Super+drag. Hyprland's official `hyprbars` plugin draws title bars, but
plugins are built against the running Hyprland's exact version with `hyprpm`,
which needs Hyprland's headers (installed with sudo) and a rebuild after every
Hyprland update.

## Decision

- Title bars come from **hyprbars**, set up by the user with
  `scripts/setup-hyprbars.sh` (build dependencies with apt, `hyprpm update`,
  `add`, `enable`) – koje itself never runs sudo (ADR 0021).
- koje's settings for it are a **second snippet**,
  `$XDG_DATA_HOME/koje/hyprland/hyprbars.conf`, installed by `koje setup`
  but sourced from `hyprland.conf` only by the script: without the plugin its
  options are config errors. It loads the plugins (`exec-once = hyprpm reload
  -n`), sets a neutral dark bar with the title, close and maximize buttons and
  maximize on double click; the context's border goes around the bar.
- Apps that draw their own header bar get none (`hyprbars:no_bar`), matched by
  class: GNOME apps, the GTK file chooser, keyring prompts. Users add others in
  `hyprland.conf`.

## Consequences

+ Every window can be moved by its title bar; the title is visible.
- After a Hyprland update the bars are off until `hyprpm update`.
- The list of apps with their own header bar is maintained by hand.
- The bar does not show the context color yet (a `hyprbars:bar_color` rule per
  context tag could, once the plugin is known to be loaded).
