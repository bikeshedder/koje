// kojed's D-Bus API for koje's UI (ADR 0060): typed calls and signals of
// io.koje.Daemon1, generated from its interface file (./daemon1.ts).

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {BUS_NAME, INTERFACE, METHODS, OBJECT_PATH, type Methods, type Signals} from './daemon1.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');

/** A call kojed answered with an error: its D-Bus name and message. */
export class KojedError extends Error {
    /** `io.koje.Error.NotFound`, …; empty when the error is not D-Bus'. */
    readonly dbusName: string;

    constructor(dbusName: string, message: string) {
        super(message);
        this.dbusName = dbusName;
    }

    /** `io.koje.Error.NotFound` → `NotFound`. */
    get short(): string {
        return this.dbusName.split('.').pop() ?? this.dbusName;
    }
}

function kojedError(e: unknown): unknown {
    if (!(e instanceof GLib.Error))
        return e;
    const name = Gio.DBusError.get_remote_error(e) ?? '';
    const message = e.message.replace(/^GDBus\.Error:[\w.]+: /, '');
    return new KojedError(name, message);
}

export class Kojed {
    readonly #conn: Gio.DBusConnection;

    constructor(conn: Gio.DBusConnection = Gio.DBus.session) {
        this.#conn = conn;
    }

    /** Call `method`; kojed's errors become {@link KojedError}. */
    async call<M extends keyof Methods>(method: M, ...args: Methods[M]['in']): Promise<Methods[M]['out']> {
        const [inSig, outSig] = METHODS[method];
        try {
            const reply = await this.#conn.call(
                BUS_NAME, OBJECT_PATH, INTERFACE, method,
                inSig === '()' ? null : new GLib.Variant(inSig, args),
                new GLib.VariantType(outSig), Gio.DBusCallFlags.NONE, -1, null);
            return reply.recursiveUnpack() as Methods[M]['out'];
        } catch (e) {
            throw kojedError(e);
        }
    }

    /** Follow `signal`; returns the id for {@link Kojed.disconnect}. */
    connect<S extends keyof Signals>(signal: S, handler: (...args: Signals[S]) => void): number {
        return this.#conn.signal_subscribe(
            BUS_NAME, INTERFACE, signal, OBJECT_PATH, null, Gio.DBusSignalFlags.NONE,
            (_conn, _sender, _path, _iface, _signal, params) => {
                handler(...(params.recursiveUnpack() as Signals[S]));
            });
    }

    disconnect(id: number): void {
        this.#conn.signal_unsubscribe(id);
    }

    /** Whether kojed owns its name now. */
    async running(): Promise<boolean> {
        try {
            const reply = await this.#conn.call(
                'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                'NameHasOwner', new GLib.Variant('(s)', [BUS_NAME]), new GLib.VariantType('(b)'),
                Gio.DBusCallFlags.NONE, -1, null);
            return (reply.deepUnpack() as [boolean])[0];
        } catch {
            return false;
        }
    }

    /** Follow kojed starting and stopping. */
    watch(handler: (running: boolean) => void): number {
        return Gio.bus_watch_name_on_connection(
            this.#conn, BUS_NAME, Gio.BusNameWatcherFlags.NONE,
            () => handler(true), () => handler(false));
    }
}
