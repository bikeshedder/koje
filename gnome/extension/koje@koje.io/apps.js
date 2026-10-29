// A context's programs in GNOME's app grid and search (ADR 0059, L4/L5). On
// a context's workspace the grid and search show that context's programs
// (kojed's `Apps`) instead of the host's and start them in the context
// (`LaunchApp`). The dash keeps GNOME's favorites; activating one prefers
// its windows on the current workspace and, on a context's workspace,
// starts the context's program with the same desktop file id if it has one.
//
// A context's program is a Shell.App made from an in-memory entry, with the
// id `koje-<context>-<desktop file id>`; GNOME's window → app mapping never
// sees it (L3). Internals: Shell.AppSystem's `lookup_app`, `get_installed`
// and `search` and Shell.App's `activate` and `open_new_window` as JS calls
// them (docs/upstream/gnome-apis.md).

import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');

const KOJED = ['io.koje.Daemon', '/io/koje/Daemon', 'io.koje.Daemon1'];

function kojed(method, params, replyType) {
    return Gio.DBus.session.call(...KOJED, method, params,
        replyType ? new GLib.VariantType(replyType) : null,
        Gio.DBusCallFlags.NONE, -1, null);
}

/** Whether every word of `query` is in the program's name or keywords. */
function matches({name, keywords}, query) {
    const text = [name, ...keywords].join(' ').toLowerCase();
    return query.toLowerCase().split(/\s+/).filter(Boolean).every(t => text.includes(t));
}

export class Apps {
    constructor(workspaces) {
        this._workspaces = workspaces;
        /** context → Map(koje id → {app, listed, id, name, keywords}) */
        this._contexts = new Map();
        this._appSys = Shell.AppSystem.get_default();

        const appSys = this._appSys;
        this._orig = {
            lookup: appSys.lookup_app,
            installed: appSys.get_installed,
            search: Shell.AppSystem.search,
            activate: Shell.App.prototype.activate,
            openNew: Shell.App.prototype.open_new_window,
        };
        const orig = this._orig;
        const self = this;
        appSys.lookup_app = id => self._lookup(id)?.app ?? orig.lookup.call(appSys, id);
        appSys.get_installed = () => {
            const programs = self._current();
            return programs
                ? [...programs.values()].map(p => p.listed)
                : orig.installed.call(appSys);
        };
        Shell.AppSystem.search = query => {
            const programs = self._current();
            if (!programs)
                return orig.search(query);
            const found = [...programs.entries()].filter(([, p]) => matches(p, query));
            return found.length > 0 ? [found.map(([id]) => id)] : [];
        };
        Shell.App.prototype.activate = function (...args) {
            if (!self._activateHere(this))
                orig.activate.apply(this, args);
        };
        Shell.App.prototype.open_new_window = function (...args) {
            if (!self._launchContexts(this))
                orig.openNew.apply(this, args);
        };

        this._activeId = global.workspace_manager.connect('active-workspace-changed',
            () => this._load());
        this._load();
    }

    destroy() {
        global.workspace_manager.disconnect(this._activeId);
        const appSys = this._appSys;
        delete appSys.lookup_app;
        delete appSys.get_installed;
        Shell.AppSystem.search = this._orig.search;
        Shell.App.prototype.activate = this._orig.activate;
        Shell.App.prototype.open_new_window = this._orig.openNew;
        this._contexts.clear();
        appSys.emit('installed-changed');
    }

    /** The programs of the context whose workspace is shown, or null. */
    _current() {
        const name = this._workspaces.activeName();
        return /^[0-9]+$/.test(name) ? null : this._contexts.get(name) ?? null;
    }

    _lookup(kojeId) {
        for (const programs of this._contexts.values()) {
            const p = programs.get(kojeId);
            if (p)
                return p;
        }
        return null;
    }

    // Ask kojed for the shown context's programs; GNOME's grid and search
    // follow `installed-changed`.
    async _load() {
        const context = this._workspaces.activeName();
        if (/^[0-9]+$/.test(context)) {
            this._appSys.emit('installed-changed');
            return;
        }
        let list;
        try {
            const reply = await kojed('Apps', new GLib.Variant('(s)', [context]),
                '(a(sssas))');
            [list] = reply.deepUnpack();
        } catch (e) {
            log(`koje: the programs of ${context}: ${e.message}`);
            list = [];
        }
        const programs = new Map();
        for (const [id, name, icon, keywords] of list)
            programs.set(`koje-${context}-${id}`, this._program(context, id, name, icon, keywords));
        this._contexts.set(context, programs);
        this._appSys.emit('installed-changed');
    }

    _program(context, id, name, icon, keywords) {
        const kojeId = `koje-${context}-${id}`;
        const entry = new GLib.KeyFile();
        const group = 'Desktop Entry';
        entry.set_string(group, 'Type', 'Application');
        entry.set_string(group, 'Name', name);
        entry.set_string(group, 'Exec', 'true');
        if (icon)
            entry.set_string(group, 'Icon', icon);
        const app = new Shell.App({app_info: GioUnix.DesktopAppInfo.new_from_keyfile(entry)});
        const launch = () => this._launch(context, id);
        Object.assign(app, {
            get_id: () => kojeId,
            activate: launch,
            open_new_window: launch,
            launch,
            can_open_new_window: () => true,
        });
        // What `get_installed` lists for the grid, which asks it only for
        // its id, whether to show it and its categories (app folders); GJS
        // keeps no JS `get_id` on the C object.
        const listed = {get_id: () => kojeId, should_show: () => true, get_categories: () => null};
        return {app, listed, id, name, keywords};
    }

    _launch(context, id) {
        kojed('LaunchApp', new GLib.Variant('(ss)', [context, id]), null).catch(e =>
            Main.notifyError(`Could not start ${id} in ${context}`, e.message));
    }

    // A running app's windows on the current workspace first (the same
    // program in several contexts is one app, L3); then the context's own
    // program of that id.
    _activateHere(app) {
        const workspace = global.workspace_manager.get_active_workspace();
        const here = app.get_windows().filter(w => w.get_workspace() === workspace);
        if (here.length > 0) {
            Main.activateWindow(here[0]);
            return true;
        }
        return this._launchContexts(app);
    }

    /** Start the shown context's program with `app`'s id instead, if it has one. */
    _launchContexts(app) {
        const context = this._workspaces.activeName();
        const p = this._contexts.get(context)?.get(`koje-${context}-${app.get_id()}`);
        if (!p)
            return false;
        this._launch(context, p.id);
        return true;
    }
}
