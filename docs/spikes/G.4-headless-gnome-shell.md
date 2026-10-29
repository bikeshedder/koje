# G.4 – Headless gnome-shell for tests

- Date: 2026-10-29
- Host / container: the **GNOME lab** image (`docs/spikes/setup/gnome-lab/Containerfile`, Ubuntu 26.04, GNOME Shell 50.1) in the dev container's Podman; no GPU

## Question

Can GNOME Shell run headless with koje's extension loaded, so the extension's
D-Bus interface can be tested automatically (L14)?

## Setup

On the host first, then in the build container
(`podman run --rm -it ubuntu:26.04` + `apt install gnome-shell dbus-daemon foot`):

```sh
export XDG_RUNTIME_DIR=$(mktemp -d) && chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- sh -c '
  gnome-shell --headless --wayland --virtual-monitor 1280x800 &
  sleep 5
  gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Get org.gnome.Shell ShellVersion
  WAYLAND_DISPLAY=wayland-0 foot & sleep 2
  gdbus introspect --session -d org.gnome.Shell -o /org/gnome/Shell
'
# Later, with the extension (G.7): copy it to ~/.local/share/gnome-shell/extensions/,
# start with `gsettings set org.gnome.shell enabled-extensions "['koje@koje.io']"`,
# then `gdbus call ... -d io.koje.Shell1 ... Windows`.
```

## Result

Run by the agent while building the lab (G.15).

- `gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800`
  starts in about 5 s with software rendering ("Created surfaceless renderer
  without GPU"). It needs a system bus (a second `dbus-daemon` as a stand-in
  is enough). Without one, the shell dies at start
  (`timeLimitsManager.js`: "Could not connect").
- An extension in `/usr/share/gnome-shell/extensions/<uuid>` loads when it is
  in `org.gnome.shell enabled-extensions` before the shell starts.
- With unsafe mode (switched on by a lab-only extension), `org.gnome.Shell.Eval`
  answers. Through it a test can read windows, workspaces and apps, and create
  virtual pointer and keyboard devices (used for G.3).
- Wayland clients connect (foot); a context container on the socket works,
  with real PIDs when the shell runs in an ancestor PID namespace
  (`--pid=host --cgroups=enabled`, see the lab README).
- **Screenshots don't work:** `org.gnome.Shell.Screenshot` and
  `Shell.Screenshot`/`composite_to_stream` never return on the
  software-rendered virtual monitor, although the stage paints.
- The same with `--devkit` (the lab): the devkit window shows the session on
  the desktop; it needs PipeWire with WirePlumber started after PipeWire.

## Conclusion

- **Automated tests of the extension are possible** in the dev container:
  start the lab image headless, load the extension under test, drive
  `io.koje.Shell1` with `gdbus`/zbus and check through `Eval`. A separate
  `just` target (needs the dev container's Podman, ~10 s), not part of `just check`.
- No pixel checks: what the extension draws (glow, badge, overlay) is checked
  through its actors (`Eval`) and by the user in the devkit window.
