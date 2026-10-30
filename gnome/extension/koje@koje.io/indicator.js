// koje's one top-bar button (ADR 0059), on the left next to the workspace
// indicator: the context of the workspace on screen in its color, a meter
// of its memory against its limits (as the Hyprland bar's, M8.21), and a
// menu with the numbers and koje Settings.
// Colors and memory from kojed (`Contexts`, `Memory`), refreshed on its
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

// The meter: width, line thickness, soft-limit tick (px); amber from this
// share of the soft limit (koje_core::memory::WARN_PERCENT).
const METER_WIDTH = 64;
const TRACK = 4;
const SOFT_MARK = 10;
const WARN = 0.9;

/** The used part's color by level (GNOME's palette, as koje-shell's). */
export function levelColor(used, soft, hard) {
    if (hard > 0 && used >= hard)
        return [0xe0, 0x1b, 0x24];
    if (soft > 0 && used >= soft)
        return [0xe6, 0x61, 0x00];
    if (soft > 0 && used >= soft * WARN)
        return [0xe5, 0xa5, 0x0a];
    return [0x2e, 0xc2, 0x7e];
}

function paintMeter(area, [used, soft, hard]) {
    const cr = area.get_context();
    const [width, height] = area.get_surface_size();
    const max = Math.max(hard || soft, 1);
    const middle = height / 2;
    const text = area.get_theme_node().get_foreground_color();
    const line = (to, rgba) => {
        cr.setSourceRGBA(...rgba);
        cr.rectangle(0, middle - TRACK / 2, to, TRACK);
        cr.fill();
    };
    line(width, [text.red / 255, text.green / 255, text.blue / 255, 0.2]);
    const fill = Math.min(used / max, 1) * width;
    line(fill, [...levelColor(used, soft, hard).map(c => c / 255), 1]);
    if (hard > 0 && soft > 0) {
        const at = Math.min(Math.max(width * soft / max, 1), width - 1);
        cr.setSourceRGBA(text.red / 255, text.green / 255, text.blue / 255, 0.7);
        cr.rectangle(at - 1, middle - SOFT_MARK / 2, 2, SOFT_MARK);
        cr.fill();
    }
    cr.$dispose();
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
        this._meter = new St.DrawingArea({
            y_align: Clutter.ActorAlign.CENTER,
            width: METER_WIDTH,
            height: SOFT_MARK + 2,
            visible: false,
        });
        this._meter.connect('repaint', a => paintMeter(a, this._memoryValues));
        box.add_child(this._dot);
        box.add_child(this._label);
        box.add_child(this._meter);
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
        // A meter needs a limit to fill up to.
        this._memoryValues = memory;
        this._meter.visible = Boolean(memory && (memory[1] > 0 || memory[2] > 0));
        if (this._meter.visible)
            this._meter.queue_repaint();
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
        // Right after the workspace indicator (Activities).
        Main.panel.addToStatusArea('koje', this._button, 1, 'left');
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
            const [contexts] = await kojed('Contexts', null, '(a(sssssu))');
            color = contexts.find(([name]) => name === context)?.[3] || color;
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
