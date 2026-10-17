#!/usr/bin/env bash
# M2.4: a filtered view of the HOST's session bus for one spike context.
# Run on the host, in its own terminal (foreground, --log shows every decision):
#
#   host-bus-proxy.sh <name>
#
# The socket lives in a directory ($XDG_RUNTIME_DIR/koje-spike/<name>/), which
# run-context.sh mounts (KOJE_SPIKE_HOSTBUS=1); restarting the proxy then does
# not leave a stale socket in the context.
#
# Allowed: notifications; the file chooser portal and its Request objects.
# Everything else – OpenURI, other portals, other services – is denied.
set -euo pipefail
name=${1:?usage: host-bus-proxy.sh <name>}
dir="${XDG_RUNTIME_DIR:?}/koje-spike/$name"
mkdir -p "$dir" && chmod 700 "$dir"
rm -f "$dir/bus"

portal=org.freedesktop.portal.Desktop
desktop=/org/freedesktop/portal/desktop
exec xdg-dbus-proxy "${DBUS_SESSION_BUS_ADDRESS:?}" "$dir/bus" --filter --log \
  --talk=org.freedesktop.Notifications \
  --call="$portal=org.freedesktop.portal.FileChooser.*@$desktop" \
  --call="$portal=org.freedesktop.DBus.Properties.Get@$desktop" \
  --call="$portal=org.freedesktop.portal.Request.*@$desktop/request/*" \
  --broadcast="$portal=org.freedesktop.portal.Request.Response@$desktop/request/*"
