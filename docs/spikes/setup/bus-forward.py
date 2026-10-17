#!/usr/bin/env python3
"""M2.4 prototype: forward one D-Bus name from the context's session bus to a
(filtered) host bus.

    bus-forward.py <name> <host-bus-address>
    bus-forward.py org.freedesktop.Notifications unix:path=/run/user/1000/koje-host/bus

- Owns <name> on the context's session bus ($DBUS_SESSION_BUS_ADDRESS).
- Method calls to <name> go to <name> on the host bus; replies (and errors) come back.
- Signals that <name>'s owner emits on the host bus are re-emitted on the context bus.

Throwaway: the real forwarder will be part of koje (Rust). Run inside a context.
"""

import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

NONE = Gio.DBusSendMessageFlags.NONE


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    name, host_address = sys.argv[1], sys.argv[2]

    ctx = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    host = Gio.DBusConnection.new_for_address_sync(
        host_address,
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )

    def log(*parts: object) -> None:
        print(*parts, flush=True)

    # --- context → host: method calls --------------------------------------
    def reply_to(call: Gio.DBusMessage, reply: Gio.DBusMessage) -> None:
        out = reply.copy()
        out.set_reply_serial(call.get_serial())
        out.set_destination(call.get_sender())
        out.set_sender(None)
        ctx.send_message(out, NONE)

    def on_host_reply(conn: Gio.DBusConnection, res: Gio.AsyncResult, call: Gio.DBusMessage) -> None:
        try:
            reply_to(call, conn.send_message_with_reply_finish(res))
        except GLib.Error as e:  # host bus or proxy refused / no reply
            log("  error:", e.message)
            err = call.new_method_error_literal("org.freedesktop.DBus.Error.Failed", e.message)
            ctx.send_message(err, NONE)

    def ctx_filter(_conn, msg: Gio.DBusMessage, incoming: bool):
        if not incoming or msg.get_message_type() != Gio.DBusMessageType.METHOD_CALL:
            return msg
        if msg.get_destination() not in (name, ctx.get_unique_name()):
            return msg
        log(f"call  {msg.get_interface()}.{msg.get_member()} {msg.get_path()} from {msg.get_sender()}")
        out = msg.copy()
        out.set_destination(name)
        out.set_sender(None)
        host.send_message_with_reply(out, NONE, -1, None, on_host_reply, msg)
        return None  # handled; GDBus must not answer "unknown method"

    # --- host → context: signals ---------------------------------------------
    owner = {"unique": None}

    def update_owner() -> None:
        try:
            owner["unique"] = host.call_sync(
                "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                "GetNameOwner", GLib.Variant("(s)", (name,)), None,
                Gio.DBusCallFlags.NONE, -1, None,
            ).unpack()[0]
        except GLib.Error as e:
            owner["unique"] = None
            log("no owner of", name, "on the host bus yet:", e.message)

    def host_filter(_conn, msg: Gio.DBusMessage, incoming: bool):
        if incoming and msg.get_message_type() == Gio.DBusMessageType.SIGNAL:
            if owner["unique"] is None or msg.get_sender() != owner["unique"]:
                update_owner()
            if msg.get_sender() == owner["unique"]:
                log(f"signal {msg.get_interface()}.{msg.get_member()} {msg.get_path()}")
                out = msg.copy()
                out.set_sender(None)
                out.set_destination(None)  # unicast on the host; broadcast in the context
                ctx.send_message(out, NONE)
        return msg

    update_owner()
    # The subscription installs the match rule so broadcast signals reach us.
    host.signal_subscribe(name, None, None, None, None, Gio.DBusSignalFlags.NONE, lambda *a: None)
    host.add_filter(host_filter)
    ctx.add_filter(ctx_filter)

    Gio.bus_own_name_on_connection(
        ctx, name, Gio.BusNameOwnerFlags.NONE,
        lambda *_: log(f"owning {name} on the context bus, forwarding to {host_address}"),
        lambda *_: sys.exit(f"could not own {name} on the context bus"),
    )
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
