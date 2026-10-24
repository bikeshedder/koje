# 0039. Title bars from the hyprbars plugin

- Status: Accepted
- Date: 2026-10-24
- Source: discussion (title bars to move windows)

## Context

Hyprland draws borders but no title bars. Apps that support `xdg-decoration`
(foot, Firefox, Qt) are told the compositor decorates, so they draw none
either; only GTK apps always draw their own header bar. Moving a window then
needs Super+drag. Hyprland's official `hyprbars` plugin draws title bars, but
plugins must be built against the running Hyprland's exact version and rebuilt
after every Hyprland update.

## Decision

- Title bars come from **hyprbars**, set up by the user with
  `scripts/setup-hyprbars.sh` – koje itself never runs sudo (ADR 0021). The
  script builds the plugin against the distribution's Hyprland headers
  (Debian/Ubuntu: `hyprland-dev`), from the hyprland-plugins commit the
  repository pins for the running Hyprland's commit (`hyprpm.toml`), into
  `~/.local/lib/koje/hyprbars.so`, and adds `plugin = <that file>` and the
  `source` line to `hyprland.conf`.
- **Not hyprpm:** Ubuntu's `hyprpm` (0.53.3) uses the system headers and
  never records them, so `hyprpm add` always fails with "Headers outdated"
  (host test). Building against `hyprland-dev` needs no sudo for headers and
  no Hyprland checkout.
- koje's settings for it are a **second snippet**,
  `$XDG_DATA_HOME/koje/hyprland/hyprbars.conf`, installed by `koje setup`
  but sourced only after the `plugin =` line: without the plugin its options
  are config errors. A neutral dark bar with the title, close and maximize
  buttons and maximize on double click; the context's border goes around it.
- Apps that draw their own header bar get none (`hyprbars:no_bar`), matched by
  class: GNOME apps, the GTK file chooser, keyring prompts. Users add others in
  `hyprland.conf`.

## Consequences

+ Every window can be moved by its title bar; the title is visible.
- After a Hyprland update the plugin no longer matches: run the script again
  and log in again (until then Hyprland may refuse to load it).
- The list of apps with their own header bar is maintained by hand.
- The bar does not show the context color yet (a `hyprbars:bar_color` rule per
  context tag could, once the plugin is known to be loaded).
