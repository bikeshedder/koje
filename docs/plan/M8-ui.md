# M8 – UI

See the mockups in the concept (sections "Layouts" and "Oberfläche").

- [ ] **M8.1 Launcher menu** (fuzzel/rofi dmenu mode): contexts with status, actions per context
- [ ] **M8.2 Waybar module**: active context capsule, layouts, stash icons, memory; click actions
  - Notes: icons from the bundled Lucide font (ADR 0020), looked up by name in `codepoints.json`; ship a complete bar configuration, not only the module.
- [ ] **M8.3 Choose panel toolkit** (iced, egui, Slint) → ADR
- [ ] **M8.4 Panel prototype** (layer-shell overlay): contexts, states, layouts matrix, memory
- [ ] **M8.5 Layout overview** (Super+L): thumbnails, drag windows between layouts
  - Notes: until then (M2.1): Waybar `wlr/taskbar`, Alt+Tab, Super+Tab picker (`assets/bin/koje-window-picker`); replace or integrate them.
- [ ] **M8.6 Overlay on windows of sleeping contexts** (ADR 0024): "<context> is asleep – Wake up" on every window of a paused context (layer-shell, positioned from kojed's window list); click wakes the context
- [ ] **M8.7 Overlay for hanging windows** (ADR 0024): same overlay with "not responding – Wait / Quit", once Hyprland reports unresponsive windows (upstream request `docs/upstream/hyprland-anr-ipc.md`; fallback: plugin)
