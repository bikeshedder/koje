#!/usr/bin/env python3
"""A fake system bus for koje-shell smoke tests: the parts of NetworkManager,
BlueZ, power-profiles-daemon and UPower that Quick Settings reads – Wi-Fi
with three networks (one saved and active), Bluetooth with two paired
devices, the power mode, a battery. Calls that change something are logged
to LOG (env). Run on its own bus; the shell finds it through
DBUS_SYSTEM_BUS_ADDRESS."""
import os
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

LOG = os.environ.get("LOG", "/dev/null")
NM = "/org/freedesktop/NetworkManager"


def log(text):
    with open(LOG, "a") as f:
        f.write(text + "\n")


def ay(text):
    return GLib.Variant("ay", text.encode())


APS = {f"{NM}/AccessPoint/{i}": (ssid, strength, secured) for i, (ssid, strength, secured) in
       enumerate([("home", 82, True), ("cafe", 64, False), ("neighbour", 35, True)], 1)}
state = {"profile": "balanced", "powered": True, "connected": {"/org/bluez/hci0/dev_1": True}}

OBJECTS = {
    NM: ("org.freedesktop.NetworkManager", {
        "Devices": GLib.Variant("ao", [f"{NM}/Devices/1"]),
        "WirelessEnabled": GLib.Variant("b", True),
        "PrimaryConnection": GLib.Variant("o", f"{NM}/ActiveConnection/1"),
    }),
    f"{NM}/Devices/1": ("org.freedesktop.NetworkManager.Device", {"DeviceType": GLib.Variant("u", 2)}),
    f"{NM}/ActiveConnection/1": ("org.freedesktop.NetworkManager.Connection.Active", {
        "Type": GLib.Variant("s", "802-11-wireless"), "Id": GLib.Variant("s", "home"),
        "SpecificObject": GLib.Variant("o", f"{NM}/AccessPoint/1")}),
    "/org/freedesktop/UPower/devices/DisplayDevice": ("org.freedesktop.UPower.Device", {
        "IsPresent": GLib.Variant("b", True), "Percentage": GLib.Variant("d", 87.0),
        "State": GLib.Variant("u", 2), "IconName": GLib.Variant("s", "battery-good-symbolic")}),
}
for path, (ssid, strength, secured) in APS.items():
    OBJECTS[path] = ("org.freedesktop.NetworkManager.AccessPoint", {
        "Ssid": ay(ssid), "Strength": GLib.Variant("y", strength), "Flags": GLib.Variant("u", 1 if secured else 0),
        "WpaFlags": GLib.Variant("u", 0), "RsnFlags": GLib.Variant("u", 0x188 if secured else 0)})


def get(path, iface, name):
    if path == f"{NM}/Devices/1" and iface == "org.freedesktop.NetworkManager.Device.Wireless":
        return {"AccessPoints": GLib.Variant("ao", list(APS)),
                "ActiveAccessPoint": GLib.Variant("o", f"{NM}/AccessPoint/1")}.get(name)
    if path == "/org/freedesktop/UPower/PowerProfiles":
        if name == "ActiveProfile":
            return GLib.Variant("s", state["profile"])
        if name == "Profiles":
            return GLib.Variant("aa{sv}", [{"Profile": GLib.Variant("s", p)} for p in
                                           ("power-saver", "balanced", "performance")])
    if path == "/org/bluez/hci0" and name == "Powered":
        return GLib.Variant("b", state["powered"])
    obj = OBJECTS.get(path)
    if obj and obj[0] == iface:
        return obj[1].get(name)
    return None


def bluez_objects():
    def dev(path, name, icon):
        return {"org.bluez.Device1": {"Alias": GLib.Variant("s", name), "Icon": GLib.Variant("s", icon),
                                      "Paired": GLib.Variant("b", True),
                                      "Connected": GLib.Variant("b", state["connected"].get(path, False))}}
    return {"/org/bluez/hci0": {"org.bluez.Adapter1": {"Powered": GLib.Variant("b", state["powered"])}},
            "/org/bluez/hci0/dev_1": dev("/org/bluez/hci0/dev_1", "WH-1000XM4", "audio-headphones"),
            "/org/bluez/hci0/dev_2": dev("/org/bluez/hci0/dev_2", "MX Master", "input-mouse")}


def call(conn, sender, path, iface, method, params, inv):
    if iface == "org.freedesktop.DBus.Properties":
        if method == "Get":
            v = get(path, params[0], params[1])
            if v is None:
                inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownProperty", params[1])
            else:
                inv.return_value(GLib.Variant("(v)", (v,)))
        elif method == "GetAll":
            inv.return_value(GLib.Variant("(a{sv})", ({},)))
        elif method == "Set":
            _, name, value = params
            log(f"Set {path} {name} {value}")
            if name == "ActiveProfile":
                state["profile"] = value
            elif name == "Powered":
                state["powered"] = value
            inv.return_value(None)
    elif method == "GetManagedObjects":
        inv.return_value(GLib.Variant("(a{oa{sa{sv}}})", (bluez_objects(),)))
    elif method == "ListConnections":
        inv.return_value(GLib.Variant("(ao)", ([f"{NM}/Settings/1"],)))
    elif method == "GetSettings":
        inv.return_value(GLib.Variant("(a{sa{sv}})", ({"802-11-wireless": {"ssid": ay("home")}},)))
    else:
        log(f"{method} {path} {params.unpack()}")
        if method in ("Connect", "Disconnect"):
            state["connected"][path] = method == "Connect"
        if method in ("ActivateConnection",):
            inv.return_value(GLib.Variant("(o)", (f"{NM}/ActiveConnection/1",)))
        elif method == "AddAndActivateConnection":
            inv.return_value(GLib.Variant("(oo)", (f"{NM}/Settings/2", f"{NM}/ActiveConnection/2")))
        else:
            inv.return_value(None)


XML = """<node>
<interface name="org.freedesktop.DBus.Properties">
 <method name="Get"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
 <method name="GetAll"><arg type="s" direction="in"/><arg type="a{sv}" direction="out"/></method>
 <method name="Set"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/></method>
</interface>
<interface name="org.freedesktop.DBus.ObjectManager">
 <method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method>
</interface>
<interface name="org.freedesktop.NetworkManager">
 <method name="ActivateConnection"><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/></method>
 <method name="AddAndActivateConnection"><arg type="a{sa{sv}}" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/><arg type="o" direction="out"/></method>
</interface>
<interface name="org.freedesktop.NetworkManager.Settings">
 <method name="ListConnections"><arg type="ao" direction="out"/></method>
</interface>
<interface name="org.freedesktop.NetworkManager.Settings.Connection">
 <method name="GetSettings"><arg type="a{sa{sv}}" direction="out"/></method>
</interface>
<interface name="org.bluez.Device1">
 <method name="Connect"/><method name="Disconnect"/>
</interface>
</node>"""


def register(conn):
    info = Gio.DBusNodeInfo.new_for_xml(XML)
    paths = set(OBJECTS) | {"/org/freedesktop/UPower/PowerProfiles", "/", f"{NM}/Settings",
                            f"{NM}/Settings/1", "/org/bluez/hci0", "/org/bluez/hci0/dev_1",
                            "/org/bluez/hci0/dev_2"}
    for path in paths:
        for iface in info.interfaces:
            conn.register_object(path, iface, call, None, None)
    for name in ("org.freedesktop.NetworkManager", "org.bluez", "org.freedesktop.UPower",
                 "org.freedesktop.UPower.PowerProfiles"):
        Gio.bus_own_name_on_connection(conn, name, 0, None, None)


conn = Gio.DBusConnection.new_for_address_sync(
    os.environ["DBUS_SYSTEM_BUS_ADDRESS"],
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
register(conn)
GLib.MainLoop().run()
