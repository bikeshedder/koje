# G.2 – Wayland globals for a context client on Mutter

- Date: 2026-10-29
- Host: **GNOME lab** in the dev container ([gnome-lab](setup/gnome-lab/README.md)): GNOME Shell / Mutter 50.1 (Ubuntu 26.04 packages), `--no-x11`, software rendering; context from `localhost/koje-spike/base` (Fedora 44)

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

Run by the agent in the lab. The screen-sharing check through the portal was
**not** run: it needs the filtered host bus (ADR 0019), which is unchanged by
the switch to GNOME.

**Context and host client see the same 38 globals** (`diff` empty), as
expected without security contexts:

```
gtk_shell1 v7                      wp_viewporter v1                   zwp_pointer_gestures_v1 v3
wl_compositor v6                   xdg_activation_v1 v1               zwp_primary_selection_device_manager_v1 v1
wl_data_device_manager v3          xdg_system_bell_v1 v1              zwp_relative_pointer_manager_v1 v1
wl_fixes v1                        xdg_toplevel_drag_manager_v1 v1    zwp_tablet_manager_v2 v2
wl_output v4                       xdg_toplevel_tag_manager_v1 v1     zwp_text_input_manager_v3 v1
wl_seat v10                        xdg_wm_base v7                     zxdg_exporter_v1 v1
wl_shm v2                          xdg_wm_dialog_v1 v1                zxdg_exporter_v2 v1
wl_subcompositor v1                zwp_idle_inhibit_manager_v1 v1     zxdg_importer_v1 v1
wp_color_manager_v1 v2             zwp_keyboard_shortcuts_inhibit_manager_v1 v1   zxdg_importer_v2 v1
wp_color_representation_manager_v1 v1   zwp_linux_dmabuf_v1 v3        zxdg_output_manager_v1 v3
wp_commit_timing_manager_v1 v1     zwp_pointer_constraints_v1 v1
wp_cursor_shape_manager_v1 v2      wp_fifo_manager_v1 v1
wp_fractional_scale_manager_v1 v1  wp_pointer_warp_v1 v1
wp_presentation v2                 wp_single_pixel_buffer_manager_v1 v1
```

**Capture fails:** `grim` → "compositor doesn't support the screen capture
protocol" (exit 1); `wf-recorder` → "compositor doesn't support
wlr-screencopy-unstable-v1" (exit 1).

What each group allows (by the protocols' definitions; not probed one by one):

| Globals | Reach |
|---|---|
| `wl_compositor`, `wl_subcompositor`, `wl_shm`, `zwp_linux_dmabuf_v1`, `wp_viewporter`, `wp_presentation`, `wp_fractional_scale`, `wp_single_pixel_buffer`, `wp_color_*`, `wp_commit_timing`, `wp_fifo`, `wl_fixes`, `wl_output`, `zxdg_output_manager`, `xdg_wm_base`, `xdg_wm_dialog`, `xdg_toplevel_tag`, `xdg_toplevel_drag`, `xdg_system_bell`, `wp_cursor_shape`, `gtk_shell1` | own surfaces only |
| `wl_seat`, `zwp_pointer_constraints`, `zwp_relative_pointer`, `zwp_pointer_gestures`, `wp_pointer_warp`, `zwp_tablet`, `zwp_text_input_v3` | input only while the client's own surface has focus |
| `wl_data_device_manager`, `zwp_primary_selection` | clipboard only on keyboard focus (no `data-control`: no reading the clipboard in the background) |
| `zxdg_exporter`/`importer` v1, v2 | parent a dialog onto another client's window, given its handle (a random token that client hands out, e.g. through the portal) |
| `xdg_activation_v1` | ask for focus with a token from a user action; GNOME may show "ready" instead |
| `zwp_keyboard_shortcuts_inhibit` | GNOME asks the user before granting |
| `zwp_idle_inhibit` | keep the screen on while the surface is visible |

**Not offered** (as on Hyprland only with a security context): screen copy
(`wlr-screencopy`, `ext-image-copy-capture`), window lists
(`foreign-toplevel`), virtual keyboard/pointer, `input-method-v2`,
`data-control`, layer shell, compositor-specific control protocols.

X11: the lab runs without Xwayland. On the host, GNOME starts Xwayland on
demand. Contexts get no `DISPLAY`, no `/tmp/.X11-unix`, and always their own
network namespace (`--network none` or their own), so Xwayland's abstract
socket is out of reach too.

### On the host (2026-10-30)

Run by the user in a context of an installed koje (Ubuntu 26.04, GNOME Shell
50, NVIDIA GPU) **with Xwayland running**: 43 globals. Compared with the lab:

| New on the host | What it gives a client | For a context |
|---|---|---|
| `wl_drm` v2, `wl_eglstream_display` v1 | older GPU buffer paths (Mesa, NVIDIA EGLStreams) | own surfaces only |
| `wp_linux_drm_syncobj_manager_v1` | explicit sync of its own GPU buffers | own surfaces only |
| `zwp_linux_dmabuf_v1` v5 (lab v3) | the same, newer | own surfaces only |
| `wp_drm_lease_device_v1` (2×, one per GPU) | lease a non-desktop output (VR headset) for direct display | a context could take a headset; desktop monitors are never offered |

Nothing comes from Xwayland: `xwayland_shell_v1` is not offered to other
clients. Screen sharing from a context works through the portal (user).

## Conclusion

- **The shared socket is acceptable for the experiment (L1).** A context
  client on Mutter's normal socket can do what a sandboxed client on Hyprland
  could: no screen capture, no input injection, no window list, no background
  clipboard. Screen sharing stays a portal matter (ADR 0019).
- What security-context-v1 would still add on Mutter: koje could tell
  contexts apart per connection (an app id for L3) and withhold the few
  remaining cross-client globals (`zxdg_importer`, `xdg_activation`). That stays
  in `docs/upstream/gnome-apis.md`.
- For G.8: on GNOME, kojed binds Mutter's socket into each context (no
  per-context socket; the per-context directory of ADR 0022 can stay, holding a
  bind of `wayland-0`).
- Rechecked on the host with Xwayland running: nothing that captures,
  injects input or reads the clipboard; the additions are GPU paths and DRM
  leases of non-desktop outputs (see above).
