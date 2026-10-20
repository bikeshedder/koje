# M9 – Integrations & app tuning

Goal: once koje works end to end, programs restore as precisely as they can.
Integrations stay declarative (ADR 0031); each item is per-application tuning.

- [ ] **M9.1 `devcontainer` integration** (from M5.2): start the inner dev container before windows that run in it; windows in dev containers (`where = "dev"`) in states, captured and restored
- [ ] **M9.2 Nautilus tabs** (from M5.13, follow-up of M5.12): remember all tabs per window and reopen them; Nautilus has no command line option for tabs – spike whether its window actions on the context's bus (`org.gtk.Actions` on `/org/gnome/Nautilus/window/<n>`) can open a tab at a folder
