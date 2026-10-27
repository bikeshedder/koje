#!/usr/bin/env python3
"""A fake kojed for koje-shell smoke tests: the parts of io.koje.Daemon1 the
shell calls, with the contexts `kundenportal` (running), `privat` (paused)
and `alt` (stopped). ACTIVE (env) names the active context; calls that
change something are logged to LOG (env)."""
import os
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

XML = """<node><interface name="io.koje.Daemon1">
<method name="RunningContexts"><arg type="a(ssub)" direction="out"/></method>
<method name="Contexts"><arg type="a(sss)" direction="out"/></method>
<method name="Memory"><arg type="s" direction="in"/><arg type="(stttt)" direction="out"/></method>
<method name="MemoryHistory"><arg type="s" direction="in"/><arg type="(suat)" direction="out"/></method>
<method name="State"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
<method name="ContextOfPid"><arg type="u" direction="in"/><arg type="s" direction="out"/></method>
<method name="Focus"><arg type="s" direction="in"/></method>
<method name="Unstash"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
<method name="KeepDisplayChange"><arg type="b" direction="out"/></method>
<method name="RevertDisplayChange"><arg type="b" direction="out"/></method>
<signal name="ContextsChanged"/><signal name="MemoryChanged"/>
<signal name="DisplayChangePending"><arg type="u"/></signal>
</interface></node>"""

GIB = 1 << 30
ACTIVE = os.environ.get("ACTIVE", "kundenportal")
LOG = os.environ.get("LOG", "/dev/null")
COLORS = {"kundenportal": "#26a269", "privat": "#c64600", "alt": "#9141ac"}
MEM = {"kundenportal": (int(5.6 * GIB), 6 * GIB, 8 * GIB), "privat": (int(1.2 * GIB), 0, 4 * GIB)}


# A running context's live state (the settings app's Layouts and Windows).
STATE = """active_layout = "code"
windows = [
  { id = "editor", integration = "editor", path = "~/src/kundenportal" },
  { id = "browser", integration = "browser" },
  { id = "term", command = "foot" },
]
[layout.code.windows]
editor = { monitor = "HEADLESS-1", x = 0, y = 32, w = 960, h = 1048 }
browser = { monitor = "HEADLESS-1", x = 960, y = 32, w = 960, h = 1048 }
[layout.review]
autosave = false
[layout.review.windows]
browser = { monitor = "HEADLESS-1", x = 0, y = 32, w = 1920, h = 1048 }
"""


def call(conn, sender, path, iface, method, params, inv):
    if method == "RunningContexts":
        inv.return_value(GLib.Variant("(a(ssub))", ([
            ("kundenportal", COLORS["kundenportal"], 3, ACTIVE == "kundenportal"),
            ("privat", COLORS["privat"], 1, ACTIVE == "privat")],)))
    elif method == "Contexts":
        inv.return_value(GLib.Variant("(a(sss))", ([
            ("kundenportal", COLORS["kundenportal"], "running"),
            ("privat", COLORS["privat"], "paused"),
            ("alt", COLORS["alt"], "stopped")],)))
    elif method == "Memory":
        name = params[0] or ACTIVE
        used, soft, hard = MEM.get(name, (0, 0, 0))
        inv.return_value(GLib.Variant("((stttt))", ((name if name in MEM else "", used, used, soft, hard),)))
    elif method == "State":
        inv.return_value(GLib.Variant("(s)", (STATE,)))
    elif method == "ContextOfPid":
        inv.return_value(GLib.Variant("(s)", (os.environ.get("NOTIFY_CONTEXT", ""),)))
    elif method == "MemoryHistory":
        name = params[0] or ACTIVE
        used = MEM.get(name, (0, 0, 0))[0]
        inv.return_value(GLib.Variant("((suat))", ((name, 5, [used - (i % 7) * (50 << 20) for i in range(60)]),)))
    else:
        with open(LOG, "a") as f:
            f.write(f"{method} {params.unpack()}\n")
        if method == "Unstash":
            inv.return_value(GLib.Variant("(s)", ("window",)))
        elif method in ("KeepDisplayChange", "RevertDisplayChange"):
            inv.return_value(GLib.Variant("(b)", (True,)))
        else:
            inv.return_value(None)


def acquired(conn, name):
    conn.register_object("/io/koje/Daemon", Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, "io.koje.Daemon", 0, acquired, None, None)
GLib.MainLoop().run()
