# G.5 – App identity across contexts

- Date:
- Host: Ubuntu 26.04, GNOME Shell version

## Question

GNOME maps windows to apps by app id (`Shell.WindowTracker`). What happens
with the same program in two contexts and on the host, and do exported
`.desktop` files with distinct ids help (L3, L5 in ADR 0059)?

## Setup

```sh
docs/spikes/setup/run-context.sh a wayland-0
docs/spikes/setup/run-context.sh b wayland-0
podman exec -d koje-spike-a foot; podman exec -d koje-spike-b foot
foot &    # host
# 1. Note: dash icons, running dots, right-click window list, Alt+Tab, Super+Tab grouping.
# 2. Export entries and repeat:
mkdir -p ~/.local/share/applications
for c in a b; do
  sed -e "s|^Exec=.*|Exec=podman exec -d koje-spike-$c foot|" \
      -e "s|^Name=.*|Name=Foot ($c)|" \
      -e "\$aStartupWMClass=foot" \
      /usr/share/applications/foot.desktop > ~/.local/share/applications/koje-spike-$c-foot.desktop
done
# 3. In Looking Glass (Alt+F2, lg):
#    global.get_window_actors().map(a => [a.meta_window.get_wm_class(), a.meta_window.get_pid(), Shell.WindowTracker.get_default().get_window_app(a.meta_window).get_id()])
rm ~/.local/share/applications/koje-spike-*-foot.desktop
```

## Result

Which app id each window gets in steps 1 and 2. What the dash/Alt+Tab show.

## Conclusion

How bad is L3 in practice? Does `app-switcher current-workspace-only` plus
the activation patch suffice for the experiment?
