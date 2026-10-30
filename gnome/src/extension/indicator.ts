// koje's one top-bar button (ADR 0059), on the left next to the workspace
// indicator: the context of the workspace on screen in its color, a meter
// of its memory against its limits (as the Hyprland bar's, M8.21), and a
// menu with the numbers and koje Settings.
// Colors and memory from kojed (`Contexts`, `Memory`), refreshed on its
// `ContextsChanged` and `MemoryChanged` and when the workspace changes.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {Kojed} from '../api/kojed.js';
import {levelColor} from './colors.js';
import {isHost, type Workspaces} from './workspaces.js';

// The meter: width, line thickness, soft-limit tick (px).
const METER_WIDTH = 64;
const TRACK = 4;
const SOFT_MARK = 10;

/** `[used, soft, hard]` in bytes; a limit of 0 is none. */
type Memory = [number, number, number];

function paintMeter(area: St.DrawingArea, [used, soft, hard]: Memory): void {
    const cr = area.get_context();
    const [width, height] = area.get_surface_size();
    const max = Math.max(hard || soft, 1);
    const middle = height / 2;
    const text = area.get_theme_node().get_foreground_color();
    const line = (to: number, [r, g, b, a]: [number, number, number, number]) => {
        cr.setSourceRGBA(r, g, b, a);
        cr.rectangle(0, middle - TRACK / 2, to, TRACK);
        cr.fill();
    };
    line(width, [text.red / 255, text.green / 255, text.blue / 255, 0.2]);
    const fill = Math.min(used / max, 1) * width;
    const [r, g, b] = levelColor(used, soft, hard);
    line(fill, [r / 255, g / 255, b / 255, 1]);
    if (hard > 0 && soft > 0) {
        const at = Math.min(Math.max(width * soft / max, 1), width - 1);
        cr.setSourceRGBA(text.red / 255, text.green / 255, text.blue / 255, 0.7);
        cr.rectangle(at - 1, middle - SOFT_MARK / 2, 2, SOFT_MARK);
        cr.fill();
    }
    cr.$dispose();
}

/** "1.2 GB", "1.2 GB of 6.0 GB" with a soft limit, both limits if set. */
function memoryText([used, soft, hard]: Memory): string {
    const size = (n: number) => GLib.format_size(n);
    const limits = [soft, hard].filter(n => n > 0).map(size);
    return limits.length > 0 ? `${size(used)} of ${limits.join(' / ')}` : size(used);
}

class Indicator extends PanelMenu.Button {
    static {
        GObject.registerClass(this);
    }

    declare private _dot: St.Widget;
    declare private _label: St.Label;
    declare private _meter: St.DrawingArea;
    declare private _memory: PopupMenu.PopupMenuItem;
    declare private _memoryValues: Memory | null;
    declare private _context: string;

    // GJS passes the constructor's arguments (alignment, name) on.
    override _init(...args: unknown[]): void {
        super._init(...(args as [number, string]));
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
        this._memoryValues = null;
        this._meter.connect('repaint', a => {
            if (this._memoryValues)
                paintMeter(a, this._memoryValues);
        });
        box.add_child(this._dot);
        box.add_child(this._label);
        box.add_child(this._meter);
        this.add_child(box);

        this._memory = new PopupMenu.PopupMenuItem('', {reactive: false});
        const menu = this.menu as PopupMenu.PopupMenu;
        menu.addMenuItem(this._memory);
        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        menu.addAction('koje Settings…', () => this._settings());
        this._context = '';
    }

    /** `memory` null before kojed's first reading. (Not `show`: that is
     * Clutter.Actor's.) */
    display(context: string, color: string, memory: Memory | null): void {
        this._context = context;
        this._dot.visible = context !== '';
        this._dot.style = `width: 10px; height: 10px; border-radius: 5px; background-color: ${color};`;
        this._label.text = context || 'koje';
        this._memory.visible = context !== '';
        this._memory.label.text = memory ? `Memory: ${memoryText(memory)}` : 'Memory: not measured yet';
        // A meter needs a limit to fill up to.
        this._memoryValues = memory;
        this._meter.visible = Boolean(memory && (memory[1] > 0 || memory[2] > 0));
        if (this._meter.visible)
            this._meter.queue_repaint();
    }

    private _settings(): void {
        const argv = ['koje', 'settings', ...this._context ? [this._context] : []];
        try {
            Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
        } catch (e) {
            Main.notifyError('Could not open koje Settings', e instanceof Error ? e.message : String(e));
        }
    }
}

export class TopBar {
    #workspaces: Workspaces;
    #kojed = new Kojed();
    #button: Indicator | null;
    #signalIds: number[];
    #activeId: number;

    constructor(workspaces: Workspaces) {
        this.#workspaces = workspaces;
        this.#button = new Indicator(0.5, 'koje');
        // Right after the workspace indicator (Activities).
        Main.panel.addToStatusArea('koje', this.#button, 1, 'left');
        this.#signalIds = [
            this.#kojed.connect('ContextsChanged', () => void this.refresh()),
            this.#kojed.connect('MemoryChanged', () => void this.refresh()),
        ];
        this.#activeId = global.workspace_manager.connect('active-workspace-changed',
            () => void this.refresh());
        void this.refresh();
    }

    destroy(): void {
        this.#signalIds.forEach(id => this.#kojed.disconnect(id));
        global.workspace_manager.disconnect(this.#activeId);
        this.#button?.destroy();
        this.#button = null;
    }

    async refresh(): Promise<void> {
        const shown = this.#workspaces.activeName();
        const context = isHost(shown) ? '' : shown;
        let color = '#808080';
        let memory: Memory | null = null;
        try {
            const [contexts] = await this.#kojed.call('Contexts');
            color = contexts.find(([name]) => name === context)?.[3] || color;
            if (context) {
                const [measured, used, , soft, hard] = await this.#kojed.call('Memory', context);
                memory = measured ? [used, soft, hard] : null;
            }
        } catch {
            // kojed not running: name only.
        }
        this.#button?.display(context, color, memory);
    }
}
