#!/usr/bin/env python3
"""M2.11: small portal clients that stay connected (run inside a context).

    portal-check.py inhibit <seconds>   hold an idle/suspend inhibit, then exit
    portal-check.py watch-settings      read color-scheme, then print every SettingChanged
"""

import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

N, P = "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop"
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(iface: str, method: str, args: GLib.Variant) -> GLib.Variant:
    return bus.call_sync(N, P, f"org.freedesktop.portal.{iface}", method, args, None, 0, 10000, None)


if sys.argv[1:2] == ["inhibit"]:
    seconds = int(sys.argv[2])
    opts = {"reason": GLib.Variant("s", "koje M2.11"), "handle_token": GLib.Variant("s", "m211")}
    print("inhibit:", call("Inhibit", "Inhibit", GLib.Variant("(sua{sv})", ("", 12, opts))).unpack()[0], flush=True)
    print(f"holding for {seconds} s – check the host's inhibitors now", flush=True)
    GLib.timeout_add_seconds(seconds, lambda: sys.exit(0))
elif sys.argv[1:2] == ["watch-settings"]:
    value = call("Settings", "ReadOne", GLib.Variant("(ss)", ("org.freedesktop.appearance", "color-scheme")))
    print("color-scheme now:", value.unpack()[0], "(0 default, 1 dark, 2 light) – toggle it on the host", flush=True)

    def changed(_c, _s, _p, _i, _sig, params):
        print("SettingChanged:", params.unpack(), flush=True)

    bus.signal_subscribe(N, "org.freedesktop.portal.Settings", "SettingChanged", P, None, 0, changed)
else:
    sys.exit(__doc__)
GLib.MainLoop().run()
