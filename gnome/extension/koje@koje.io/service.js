// io.koje.Extension1 on the session bus (dbus/io.koje.Extension1.xml): what
// kojed's adapter crates/koje-gnome calls. Only the user's own session bus
// reaches it; contexts never see that bus (ADR 0019).

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import * as Monitors from './monitors.js';

const BUS_NAME = 'io.koje.Extension';
const OBJECT_PATH = '/io/koje/Extension';
const ERROR = 'io.koje.Extension1.Error.Failed';

export class Service {
    constructor(dir, workspaces) {
        this._workspaces = workspaces;
        /** Set by the extension once the windows are tracked. */
        this.windows = null;
        /** Set by the extension: the sleeping contexts. */
        this.sleep = null;
        /** Called after every `Apply`. */
        this.changed = () => {};
        /** Colors of a tag (`context-rule`), tags and colors of windows: drawn in G.9. */
        this.rules = new Map();
        this.tags = new Map();
        this.colors = new Map();

        const [, bytes] = dir.get_child('dbus').get_child('io.koje.Extension1.xml').load_contents(null);
        this._dbus = Gio.DBusExportedObject.wrapJSObject(new TextDecoder().decode(bytes), this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);
    }

    destroy() {
        Gio.bus_unown_name(this._nameId);
        this._dbus.unexport();
        this._dbus = null;
    }

    /** A window is gone: its tag and colors too. */
    forget(id) {
        this.tags.delete(id);
        this.colors.delete(id);
    }

    emit(signal, type, values) {
        this._dbus?.emit_signal(signal, new GLib.Variant(type, values));
    }

    Windows() {
        return this.windows.list();
    }

    ActiveWorkspace() {
        return this._workspaces.activeName();
    }

    Monitors() {
        return Monitors.list();
    }

    ApplyAsync([commands], invocation) {
        const failed = [];
        for (const [verb, args] of commands) {
            try {
                this._run(verb, Object.fromEntries(
                    Object.entries(args).map(([k, v]) => [k, v.recursiveUnpack()])));
            } catch (e) {
                failed.push(`${verb}: ${e.message}`);
            }
        }
        this.changed();
        if (failed.length > 0)
            invocation.return_dbus_error(ERROR, failed.join('; '));
        else
            invocation.return_value(null);
    }

    _run(verb, a) {
        const time = global.get_current_time();
        const window = () => this.windows.get(a.window);
        switch (verb) {
        case 'context-rule':
            this.rules.set(a.tag, {active: a.active, inactive: a.inactive});
            break;
        case 'border-colors':
            this.colors.set(a.window, {active: a.active, inactive: a.inactive});
            break;
        case 'tag-window':
            this.tags.set(a.window, a.tag);
            break;
        case 'place': {
            const w = window();
            // A frozen program must get no configure events (G.3).
            if (this.sleep?.asleep(w))
                break;
            const index = Monitors.indexOf(a.monitor);
            if (index < 0)
                throw new Error(`unknown monitor ${a.monitor}`);
            const g = global.display.get_monitor_geometry(index);
            if (w.is_fullscreen())
                w.unmake_fullscreen();
            if (w.is_maximized())
                w.unmaximize();
            w.move_resize_frame(true, g.x + a.x, g.y + a.y, a.width, a.height);
            break;
        }
        case 'move-to-workspace':
            window().change_workspace(this._workspaces.get(a.workspace));
            break;
        case 'focus-window':
            Main.activateWindow(window(), time);
            break;
        case 'close-window':
            window().delete(time);
            break;
        case 'focus-workspace':
            this._workspaces.get(a.workspace).activate(time);
            break;
        case 'carry-to-workspace': {
            const w = window();
            const ws = this._workspaces.get(a.workspace);
            w.change_workspace(ws);
            ws.activate_with_focus(w, time);
            break;
        }
        case 'keep-workspaces':
            this._workspaces.keep(a.workspaces);
            break;
        case 'sleep':
            this.sleep.set(a.context, a.asleep);
            break;
        case 'toggle-special':
            throw new Error('not available on GNOME');
        default:
            throw new Error('unknown command');
        }
    }
}
