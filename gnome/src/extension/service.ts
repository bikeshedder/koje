// io.koje.Extension1 on the session bus (dbus/io.koje.Extension1.xml): what
// kojed's adapter crates/koje-gnome calls. Only the user's own session bus
// reaches it; contexts never see that bus (ADR 0019).

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import * as Monitors from './monitors.js';
import type {Sleep} from './sleep.js';
import type {Window, Windows} from './windows.js';
import type {Workspaces} from './workspaces.js';

const BUS_NAME = 'io.koje.Extension';
const OBJECT_PATH = '/io/koje/Extension';
const ERROR = 'io.koje.Extension1.Error.Failed';

/** A color pair kojed sends: focused, unfocused. */
export interface Colors {
    active: string;
    inactive: string;
}

/** A command's arguments, unpacked. */
type Args = Record<string, unknown>;

export class Service {
    /** Set by the extension once the windows are tracked. */
    windows: Windows | null = null;
    /** Set by the extension: the sleeping contexts. */
    sleep: Sleep | null = null;
    /** Called after every `Apply`. */
    changed: () => void = () => {};
    /** Colors of a tag (`context-rule`). */
    readonly rules = new Map<string, Colors>();
    /** Tags and colors of windows by id. */
    readonly tags = new Map<number, string>();
    readonly colors = new Map<number, Colors>();

    #workspaces: Workspaces;
    #dbus: Gio.DBusExportedObject | null;
    #nameId: number;

    constructor(dir: Gio.File, workspaces: Workspaces) {
        this.#workspaces = workspaces;
        const [, bytes] = dir.get_child('dbus').get_child('io.koje.Extension1.xml').load_contents(null);
        this.#dbus = Gio.DBusExportedObject.wrapJSObject(new TextDecoder().decode(bytes), this);
        this.#dbus.export(Gio.DBus.session, OBJECT_PATH);
        this.#nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);
    }

    destroy(): void {
        Gio.bus_unown_name(this.#nameId);
        this.#dbus?.unexport();
        this.#dbus = null;
    }

    /** A window is gone: its tag and colors too. */
    forget(id: number): void {
        this.tags.delete(id);
        this.colors.delete(id);
    }

    emit(signal: string, type: string, values: unknown[]): void {
        this.#dbus?.emit_signal(signal, new GLib.Variant(type, values));
    }

    // ----- io.koje.Extension1 -----

    Windows(): Window[] {
        return this.windows?.list() ?? [];
    }

    ActiveWorkspace(): string {
        return this.#workspaces.activeName();
    }

    Monitors(): Monitors.Monitor[] {
        return Monitors.list();
    }

    ApplyAsync([commands]: [[string, Record<string, GLib.Variant>][]], invocation: Gio.DBusMethodInvocation): void {
        const failed = [];
        for (const [verb, args] of commands) {
            try {
                this.#run(verb, Object.fromEntries(
                    Object.entries(args).map(([k, v]) => [k, v.recursiveUnpack()])));
            } catch (e) {
                failed.push(`${verb}: ${e instanceof Error ? e.message : String(e)}`);
            }
        }
        this.changed();
        if (failed.length > 0)
            invocation.return_dbus_error(ERROR, failed.join('; '));
        else
            invocation.return_value(null);
    }

    #run(verb: string, a: Args): void {
        const time = global.get_current_time();
        const window = () => this.windows!.get(a.window as number);
        switch (verb) {
        case 'context-rule':
            this.rules.set(a.tag as string, {active: a.active as string, inactive: a.inactive as string});
            break;
        case 'border-colors':
            this.colors.set(a.window as number, {active: a.active as string, inactive: a.inactive as string});
            break;
        case 'tag-window':
            this.tags.set(a.window as number, a.tag as string);
            break;
        case 'place': {
            const w = window();
            // A frozen program must get no configure events (G.3).
            if (this.sleep?.asleep(w))
                break;
            const index = Monitors.indexOf(a.monitor as string);
            if (index < 0)
                throw new Error(`unknown monitor ${a.monitor}`);
            const g = global.display.get_monitor_geometry(index);
            if (w.is_fullscreen())
                w.unmake_fullscreen();
            if (w.is_maximized())
                w.unmaximize();
            w.move_resize_frame(true, g.x + (a.x as number), g.y + (a.y as number),
                a.width as number, a.height as number);
            break;
        }
        case 'move-to-workspace':
            window().change_workspace(this.#workspaces.get(a.workspace as string));
            break;
        case 'focus-window':
            Main.activateWindow(window(), time);
            break;
        case 'close-window':
            window().delete(time);
            break;
        case 'focus-workspace':
            this.#workspaces.get(a.workspace as string).activate(time);
            break;
        case 'carry-to-workspace': {
            const w = window();
            const ws = this.#workspaces.get(a.workspace as string);
            w.change_workspace(ws);
            ws.activate_with_focus(w, time);
            break;
        }
        case 'keep-workspaces':
            this.#workspaces.keep(a.workspaces as string[]);
            break;
        case 'sleep':
            this.sleep?.set(a.context as string, a.asleep as boolean);
            break;
        case 'toggle-special':
            throw new Error('not available on GNOME');
        default:
            throw new Error('unknown command');
        }
    }
}
