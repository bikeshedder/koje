// The windows kojed sees, and their events. A window is named by its Mutter
// id; kojed sees normal windows and dialogs not attached to a parent.

import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import * as Monitors from './monitors.js';
import type {Service} from './service.js';
import type {Workspaces} from './workspaces.js';

/** A window as `Windows` reports it: `(tussssiiuu)`. */
export type Window = [number, number, string, string, string, string, number, number, number, number];

function interesting(w: Meta.Window): boolean {
    const type = w.get_window_type();
    return !w.is_override_redirect() && !w.is_skip_taskbar() &&
        (type === Meta.WindowType.NORMAL ||
         (type === Meta.WindowType.DIALOG && !w.is_attached_dialog()));
}

export class Windows {
    /** Called when a window changed its workspace. */
    changed: () => void = () => {};

    #workspaces: Workspaces;
    #service: Service;
    /** id → the window and its signal handler ids */
    #tracked = new Map<number, {window: Meta.Window; ids: number[]}>();
    #idles = new Set<number>();
    #displayIds: number[];
    #active: string;
    #activeId: number;

    constructor(workspaces: Workspaces, service: Service) {
        this.#workspaces = workspaces;
        this.#service = service;
        for (const actor of global.get_window_actors()) {
            const w = actor.meta_window;
            if (w && interesting(w))
                this.#track(w);
        }
        this.#displayIds = [
            global.display.connect('window-created', (_d, w) => this.#created(w)),
            global.display.connect('notify::focus-window', () => this.#focused()),
        ];
        // Mutter reports the active workspace again when its index shifts
        // (GNOME removed one before it): kojed hears of real changes only.
        this.#active = this.#workspaces.activeName();
        this.#activeId = global.workspace_manager.connect('active-workspace-changed', () => {
            const name = this.#workspaces.activeName();
            if (name !== this.#active) {
                this.#active = name;
                this.#service.emit('WorkspaceFocused', '(s)', [name]);
            }
        });
    }

    destroy(): void {
        this.#idles.forEach(id => GLib.source_remove(id));
        this.#displayIds.forEach(id => global.display.disconnect(id));
        global.workspace_manager.disconnect(this.#activeId);
        for (const {window, ids} of this.#tracked.values())
            ids.forEach(id => window.disconnect(id));
        this.#tracked.clear();
    }

    /** A tracked window by id; throws if there is none. */
    get(id: number): Meta.Window {
        const t = this.#tracked.get(id);
        if (!t)
            throw new Error(`no window ${id}`);
        return t.window;
    }

    /** Every tracked window. */
    all(): Meta.Window[] {
        return [...this.#tracked.values()].map(t => t.window);
    }

    list(): Window[] {
        return [...this.#tracked.values()].map(({window}) => this.#entry(window));
    }

    #entry(w: Meta.Window): Window {
        const monitor = w.get_monitor();
        const g = global.display.get_monitor_geometry(monitor);
        const r = w.get_frame_rect();
        return [
            w.get_id(), Math.max(w.get_pid(), 0), w.get_wm_class() ?? '', w.get_title() ?? '',
            this.#workspaceName(w), Monitors.connector(monitor),
            r.x - g.x, r.y - g.y, r.width, r.height,
        ];
    }

    #workspaceName(w: Meta.Window): string {
        const ws = w.is_on_all_workspaces() ? null : w.get_workspace();
        return ws ? this.#workspaces.name(ws) : this.#workspaces.activeName();
    }

    // A Wayland window gets its app id after it is created (mutter#157): it
    // is reported once the client's requests so far are handled.
    #created(w: Meta.Window): void {
        if (!interesting(w))
            return;
        const id = GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
            this.#idles.delete(id);
            if (w.get_compositor_private() && !this.#tracked.has(w.get_id())) {
                this.#track(w);
                this.#service.emit('WindowOpened', '(tsss)', [
                    w.get_id(), this.#workspaceName(w), w.get_wm_class() ?? '', w.get_title() ?? '',
                ]);
            }
            return GLib.SOURCE_REMOVE;
        });
        this.#idles.add(id);
    }

    #track(w: Meta.Window): void {
        const id = w.get_id();
        const ids: number[] = [
            w.connect('unmanaged', () => {
                ids.forEach(i => w.disconnect(i));
                this.#tracked.delete(id);
                this.#service.forget(id);
                this.#service.emit('WindowClosed', '(t)', [id]);
            }),
            w.connect('notify::title', () =>
                this.#service.emit('WindowTitle', '(ts)', [id, w.get_title() ?? ''])),
            w.connect('workspace-changed', () => this.changed()),
        ];
        this.#tracked.set(id, {window: w, ids});
    }

    #focused(): void {
        const w = global.display.focus_window;
        const id = w && this.#tracked.has(w.get_id()) ? w.get_id() : 0;
        this.#service.emit('WindowFocused', '(t)', [id]);
    }
}
