// The windows kojed sees, and their events. A window is named by its Mutter
// id; kojed sees normal windows and dialogs not attached to a parent.

import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import * as Monitors from './monitors.js';

function interesting(w) {
    const type = w.get_window_type();
    return !w.is_override_redirect() && !w.is_skip_taskbar() &&
        (type === Meta.WindowType.NORMAL ||
         (type === Meta.WindowType.DIALOG && !w.is_attached_dialog()));
}

export class Windows {
    constructor(workspaces, service) {
        this._workspaces = workspaces;
        this._service = service;
        /** id → {window, ids: signal handler ids} */
        this._tracked = new Map();
        this._idles = new Set();
        for (const actor of global.get_window_actors()) {
            if (interesting(actor.meta_window))
                this._track(actor.meta_window);
        }
        this._displayIds = [
            global.display.connect('window-created', (_d, w) => this._created(w)),
            global.display.connect('notify::focus-window', () => this._focused()),
        ];
        // Mutter reports the active workspace again when its index shifts
        // (GNOME removed one before it): kojed hears of real changes only.
        this._active = this._workspaces.activeName();
        this._activeId = global.workspace_manager.connect('active-workspace-changed', () => {
            const name = this._workspaces.activeName();
            if (name !== this._active) {
                this._active = name;
                this._service.emit('WorkspaceFocused', '(s)', [name]);
            }
        });
    }

    destroy() {
        this._idles.forEach(id => GLib.source_remove(id));
        this._displayIds.forEach(id => global.display.disconnect(id));
        global.workspace_manager.disconnect(this._activeId);
        for (const {window, ids} of this._tracked.values())
            ids.forEach(id => window.disconnect(id));
        this._tracked.clear();
    }

    /** A tracked window by id; throws if there is none. */
    get(id) {
        const t = this._tracked.get(id);
        if (!t)
            throw new Error(`no window ${id}`);
        return t.window;
    }

    /** Every tracked window as `(tussssiiuu)`. */
    list() {
        return [...this._tracked.values()].map(({window}) => this._entry(window));
    }

    _entry(w) {
        const monitor = w.get_monitor();
        const g = global.display.get_monitor_geometry(monitor);
        const r = w.get_frame_rect();
        return [
            w.get_id(), Math.max(w.get_pid(), 0), w.get_wm_class() ?? '', w.get_title() ?? '',
            this._workspaceName(w), Monitors.connector(monitor),
            r.x - g.x, r.y - g.y, r.width, r.height,
        ];
    }

    _workspaceName(w) {
        const ws = w.is_on_all_workspaces() ? null : w.get_workspace();
        return ws ? this._workspaces.name(ws) : this._workspaces.activeName();
    }

    // A Wayland window gets its app id after it is created (mutter#157): it
    // is reported once the client's requests so far are handled.
    _created(w) {
        if (!interesting(w))
            return;
        const id = GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
            this._idles.delete(id);
            if (w.get_compositor_private() && !this._tracked.has(w.get_id())) {
                this._track(w);
                this._service.emit('WindowOpened', '(tsss)', [
                    w.get_id(), this._workspaceName(w), w.get_wm_class() ?? '', w.get_title() ?? '',
                ]);
            }
            return GLib.SOURCE_REMOVE;
        });
        this._idles.add(id);
    }

    _track(w) {
        const id = w.get_id();
        const ids = [
            w.connect('unmanaged', () => {
                ids.forEach(i => w.disconnect(i));
                this._tracked.delete(id);
                this._service.emit('WindowClosed', '(t)', [id]);
            }),
            w.connect('notify::title', () =>
                this._service.emit('WindowTitle', '(ts)', [id, w.get_title() ?? ''])),
        ];
        this._tracked.set(id, {window: w, ids});
    }

    _focused() {
        const w = global.display.focus_window;
        const id = w && this._tracked.has(w.get_id()) ? w.get_id() : 0;
        this._service.emit('WindowFocused', '(t)', [id]);
    }
}
