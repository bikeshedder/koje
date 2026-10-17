#!/usr/bin/env python3
"""M2.4/M2.10 prototype: forward one D-Bus name from the context's session bus
to a (filtered) host bus.

    bus-forward.py <name> <host-bus-address>
    bus-forward.py org.freedesktop.portal.Desktop unix:path=/run/user/1000/koje-host/bus

- Owns <name> on the context's session bus ($DBUS_SESSION_BUS_ADDRESS).
- **One host connection per client**: each program in the context that calls
  <name> gets its own connection to the host bus. Its requests are its own, and
  when it exits the connection closes, so the host service releases what it held
  (inhibits, screen cast sessions).
- **Path translation**: portal clients expect request/session objects at
  `/org/freedesktop/portal/desktop/{request,session}/<own unique name>/<token>`.
  On the host that segment is the host connection's name. The forwarder maps it
  in both directions – in object paths and inside message bodies.
- Signals from <name>'s owner on a client's host connection go to that client.
- Exits when the proxy goes away (a loop around the script restarts it).

Throwaway: the real forwarder will be part of koje (Rust). Run inside a context.
"""

import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

NONE = Gio.DBusSendMessageFlags.NONE
PORTAL = "/org/freedesktop/portal/desktop/"
OBJECT_KINDS = ("request", "session")


def log(*parts: object) -> None:
    print(*parts, flush=True)


def segment(unique_name: str) -> str:
    """`:1.20` → `1_20`, as portal clients build their object paths."""
    return unique_name.lstrip(":").replace(".", "_")


def path_mapper(src: str, dst: str):
    """Replace the name segment `src` by `dst` in portal request/session paths."""
    def f(s: str) -> str:
        if not s.startswith(PORTAL):
            return s
        for kind in OBJECT_KINDS:
            prefix = f"{PORTAL}{kind}/{src}/"
            if s.startswith(prefix) or s == prefix[:-1]:
                return f"{PORTAL}{kind}/{dst}/" + s[len(prefix):]
        return s
    return f


def rewrite(v: GLib.Variant, f) -> GLib.Variant:
    """Apply `f` to every string and object path in `v`."""
    t = v.get_type_string()
    if not any(c in t for c in "sov"):
        return v
    if t in ("s", "o"):
        s = v.get_string()
        n = f(s)
        return v if n == s else GLib.Variant(t, n)
    if t == "v":
        return GLib.Variant.new_variant(rewrite(v.get_variant(), f))
    kids = [rewrite(v.get_child_value(i), f) for i in range(v.n_children())]
    if t[0] == "(":
        return GLib.Variant.new_tuple(*kids)
    if t[0] == "{":
        return GLib.Variant.new_dict_entry(kids[0], kids[1])
    if t[0] == "a":
        return GLib.Variant.new_array(GLib.VariantType(t[1:]), kids)
    if t[0] == "m":
        return GLib.Variant.new_maybe(GLib.VariantType(t[1:]), kids[0] if kids else None)
    return v


def translated(msg: Gio.DBusMessage, f) -> Gio.DBusMessage:
    out = msg.copy()
    out.set_sender(None)
    if out.get_path():
        out.set_path(f(out.get_path()))
    body = out.get_body()
    if body is not None:
        out.set_body(rewrite(body, f))
    return out


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    name, host_address = sys.argv[1], sys.argv[2]
    ctx = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def connect_host() -> Gio.DBusConnection:
        conn = Gio.DBusConnection.new_for_address_sync(
            host_address,
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
            | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
            None, None,
        )
        conn.set_exit_on_close(False)
        return conn

    # A control connection: name owner lookups, and noticing that the proxy is gone.
    control = connect_host()

    def on_control_closed(_conn, vanished: bool, error) -> None:
        log("host connection closed", "(proxy gone)" if vanished else "", error.message if error else "")
        sys.exit(1)

    control.connect("closed", on_control_closed)
    owner = {"unique": None}

    def update_owner() -> None:
        try:
            owner["unique"] = control.call_sync(
                "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                "GetNameOwner", GLib.Variant("(s)", (name,)), None,
                Gio.DBusCallFlags.NONE, -1, None,
            ).unpack()[0]
        except GLib.Error as e:
            owner["unique"] = None
            log("no owner of", name, "on the host bus yet:", e.message)

    # --- one host connection per client ---------------------------------------
    clients: dict[str, dict] = {}

    def upstream(client: str) -> dict:
        if client in clients:
            return clients[client]
        conn = connect_host()
        ctx_seg, host_seg = segment(client), segment(conn.get_unique_name())
        up = {
            "conn": conn,
            "to_host": path_mapper(ctx_seg, host_seg),
            "to_ctx": path_mapper(host_seg, ctx_seg),
        }

        def forward_signal(msg: Gio.DBusMessage) -> bool:
            if msg.get_sender() != owner["unique"]:
                update_owner()
            if msg.get_sender() == owner["unique"]:
                out = translated(msg, up["to_ctx"])
                out.set_destination(client)
                log(f"signal {msg.get_interface()}.{msg.get_member()} {out.get_path()} → {client}")
                ctx.send_message(out, NONE)
            return False

        def host_filter(_conn, msg: Gio.DBusMessage, incoming: bool):
            # Filters run on GDBus' worker thread: no blocking calls here.
            if incoming and msg.get_message_type() == Gio.DBusMessageType.SIGNAL:
                GLib.idle_add(forward_signal, msg)
            return msg

        # The subscription installs the match rule so broadcast signals reach us.
        conn.signal_subscribe(name, None, None, None, None, Gio.DBusSignalFlags.NONE, lambda *a: None)
        conn.add_filter(host_filter)
        clients[client] = up
        log(f"client {client}: host connection {conn.get_unique_name()} ({ctx_seg} ↔ {host_seg})")
        return up

    def on_client_gone(_conn, _sender, _path, _iface, _signal, params) -> None:
        gone, _old, new = params.unpack()
        if not new and gone in clients:
            log(f"client {gone} gone: closing its host connection")
            clients.pop(gone)["conn"].close(None, None, None)

    ctx.signal_subscribe(
        "org.freedesktop.DBus", "org.freedesktop.DBus", "NameOwnerChanged",
        "/org/freedesktop/DBus", None, Gio.DBusSignalFlags.NONE, on_client_gone,
    )

    # --- context → host: method calls -----------------------------------------
    def on_host_reply(conn, res, ctx_and_call) -> None:
        call, up = ctx_and_call
        try:
            reply = translated(conn.send_message_with_reply_finish(res), up["to_ctx"])
            reply.set_reply_serial(call.get_serial())
            reply.set_destination(call.get_sender())
            ctx.send_message(reply, NONE)
        except GLib.Error as e:  # host bus or proxy refused / no reply
            log("  error:", e.message)
            ctx.send_message(call.new_method_error_literal("org.freedesktop.DBus.Error.Failed", e.message), NONE)

    def forward_call(msg: Gio.DBusMessage) -> bool:
        try:
            up = upstream(msg.get_sender())
            out = translated(msg, up["to_host"])
            out.set_destination(name)
            log(f"call  {msg.get_interface()}.{msg.get_member()} {out.get_path()} from {msg.get_sender()}")
            up["conn"].send_message_with_reply(out, NONE, -1, None, on_host_reply, (msg, up))
        except Exception as e:  # never leave a caller waiting for a timeout
            log("  forwarder error:", e)
            ctx.send_message(msg.new_method_error_literal("org.freedesktop.DBus.Error.Failed", str(e)), NONE)
        return False

    def ctx_filter(_conn, msg: Gio.DBusMessage, incoming: bool):
        # Filters run on GDBus' worker thread: opening a host connection there
        # deadlocks, so the work goes to the main loop.
        if not incoming or msg.get_message_type() != Gio.DBusMessageType.METHOD_CALL:
            return msg
        if msg.get_destination() not in (name, ctx.get_unique_name()):
            return msg
        GLib.idle_add(forward_call, msg)
        return None  # handled; GDBus must not answer "unknown method"

    update_owner()
    ctx.add_filter(ctx_filter)
    Gio.bus_own_name_on_connection(
        ctx, name, Gio.BusNameOwnerFlags.NONE,
        lambda *_: log(f"owning {name} on the context bus, forwarding to {host_address}"),
        lambda *_: sys.exit(f"could not own {name} on the context bus"),
    )
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
