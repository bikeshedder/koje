// koje's one top-bar button (ADR 0059): the context of the workspace on
// screen in its color, its memory against its limits, and koje Settings.
// Colors and memory from kojed (`ListContexts`, `Memory`), refreshed on its
// `ContextsChanged` and `MemoryChanged` and when the workspace changes.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');

const KOJED = ['io.koje.Daemon', '/io/koje/Daemon', 'io.koje.Daemon1'];

async function kojed(method, params, replyType) {
    const reply = await Gio.DBus.session.call(...KOJED, method, params,
        new GLib.VariantType(replyType), Gio.DBusCallFlags.NONE, -1, null);
    return reply.deepUnpack();
}

/** "1.2 GB", "1.2 GB of 6.0 GB" with a soft limit, both limits if set. */
export function memoryText(used, soft, hard) {
    const size = n => GLib.format_size(n);
    const limits = [soft, hard].filter(n => n > 0).map(size);
    return limits.length > 0 ? `${size(used)} of ${limits.join(' / ')}` : size(used);
}

class Indicator extends PanelMenu.Button {
    _init() {
        super._init(0.5, 'koje');
        const box = new St.BoxLayout({style: 'spacing: 6px;'});
        this._dot = new St.Widget({
            y_align: Clutter.ActorAlign.CENTER,
            style: 'width: 10px; height: 10px; border-radius: 5px;',
        });
        this._label = new St.Label({y_align: Clutter.ActorAlign.CENTER, text: 'koje'});
        box.add_child(this._dot);
        box.add_child(this._label);
        this.add_child(box);

        this._memory = new PopupMenu.PopupMenuItem('', {reactive: false});
        this.menu.addMenuItem(this._memory);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction('koje Settings…', () => this._settings());
        this._context = '';
    }

    /** `memory`: `[used, soft, hard]`, or null before kojed's first reading. */
    show(context, color, memory) {
        this._context = context;
        this._dot.visible = context !== '';
        this._dot.style = `width: 10px; height: 10px; border-radius: 5px; background-color: ${color};`;
        this._label.text = context || 'koje';
        this._memory.visible = context !== '';
        this._memory.label.text = memory
            ? `Memory: ${memoryText(...memory)}` : 'Memory: not measured yet';
    }

    _settings() {
        const argv = ['koje', 'settings', ...this._context ? [this._context] : []];
        try {
            Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
        } catch (e) {
            Main.notifyError('Could not open koje Settings', e.message);
        }
    }
}
GObject.registerClass(Indicator);

export class TopBar {
    constructor(workspaces) {
        this._workspaces = workspaces;
        this._button = new Indicator();
        Main.panel.addToStatusArea('koje', this._button, 0, 'right');
        this._signalIds = ['ContextsChanged', 'MemoryChanged'].map(signal =>
            Gio.DBus.session.signal_subscribe(KOJED[0], KOJED[2], signal, KOJED[1], null,
                Gio.DBusSignalFlags.NONE, () => this.refresh()));
        this._activeId = global.workspace_manager.connect('active-workspace-changed',
            () => this.refresh());
        this.refresh();
    }

    destroy() {
        this._signalIds.forEach(id => Gio.DBus.session.signal_unsubscribe(id));
        global.workspace_manager.disconnect(this._activeId);
        this._button.destroy();
    }

    async refresh() {
        const shown = this._workspaces.activeName();
        const context = /^[0-9]+$/.test(shown) ? '' : shown;
        let color = '#808080';
        let memory = null;
        try {
            const [contexts] = await kojed('ListContexts', null, '(a(ssu))');
            color = contexts.find(([name]) => name === context)?.[1] ?? color;
            if (context) {
                const [measured, used, , soft, hard] = await kojed('Memory',
                    new GLib.Variant('(s)', [context]), '(stttt)');
                memory = measured ? [used, soft, hard] : null;
            }
        } catch {
            // kojed not running: name only.
        }
        this._button?.show(context, color, memory);
    }
}
