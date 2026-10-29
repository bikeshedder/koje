# G.4 – Headless gnome-shell for tests

- Date:
- Host / container: Ubuntu 26.04 build image (`build/Containerfile`), gnome-shell version

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

Does the shell start? Without a GPU (llvmpipe)? Does a client window appear?
Startup time.

## Conclusion

Smoke test in `just check`, in a separate `just` target, or host only?
