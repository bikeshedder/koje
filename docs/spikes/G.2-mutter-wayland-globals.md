# G.2 – Wayland globals for a context client on Mutter

- Date:
- Host: Ubuntu 26.04, kernel, GNOME Shell / Mutter version (`gnome-shell --version`), Podman version, GPU

## Question

Mutter has no `security-context-v1` (ADR 0059, L1), so contexts would use
Mutter's normal socket. What can a client in a context do with it? In
particular: is there any Wayland protocol that captures the screen, injects
input, or reads other clients' content without the portal?

## Setup

In the lab instead (dev container, [gnome-lab](setup/gnome-lab/README.md)): `./lab.sh start`, then use
`./lab.sh context a` for `run-context.sh a wayland-0`, and `./lab.sh exec wayland-info` for the host's `wayland-info`. Note in the
report that it ran in the lab.

In a plain GNOME session (`echo $XDG_CURRENT_DESKTOP` → `ubuntu:GNOME`):

```sh
docs/spikes/setup/run-context.sh a wayland-0
podman exec koje-spike-a sh -c 'command -v wayland-info || sudo dnf -y install wayland-utils'
podman exec koje-spike-a wayland-info | grep -E "^interface" > /tmp/g2-context.txt
wayland-info | grep -E "^interface" > /tmp/g2-host.txt     # host client, for comparison
diff /tmp/g2-host.txt /tmp/g2-context.txt
# Capture attempts from the context – expected to fail:
podman exec koje-spike-a sh -c 'grim /tmp/x.png; echo grim=$?'
podman exec koje-spike-a sh -c 'timeout 3 wf-recorder -f /tmp/x.mp4; echo wf=$?'
# Screen sharing through the portal must still work (filtered bus, ADR 0019):
# share a screen from Firefox in the context on https://mozilla.github.io/webrtc-landing/gum_test.html
```

## Result

Paste both interface lists. For each interface not in plain Wayland core/stable
(`gtk_shell1`, `xdg_foreign`, `zwp_*`, `wp_*`, …), say whether it gives access
beyond the client's own windows.

## Conclusion

Is the shared socket acceptable for the experiment (L1)? Update ADR 0059.
