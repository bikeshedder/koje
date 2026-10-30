// Workspace names for kojed: a context's workspace is named after the
// context, every other one by its number from 1 among those others, so a
// host workspace keeps its number when context workspaces come and go
// (context names never are numbers). A context's workspace is made when a
// command names it. GNOME removes empty workspaces; the ones kojed keeps
// (`keep-workspaces`) stay.

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

// A `_keepAliveId` that is no GLib source: set only while GNOME's
// WorkspaceTracker checks, so its own keep-alive timers are never touched.
const KEPT = -1;

export class Workspaces {
    constructor() {
        this._manager = global.workspace_manager;
        /** context name → Meta.Workspace */
        this._contexts = new Map();
        /** context names whose workspace stays while empty */
        this._kept = new Set();
        this._removedId = this._manager.connect('workspace-removed', () => this._forgetRemoved());

        this._tracker = Main.wm._workspaceTracker;
        const check = this._tracker._checkWorkspaces;
        const kept = () => [...this._kept].map(n => this._contexts.get(n)).filter(Boolean);
        this._tracker._checkWorkspaces = function () {
            const marked = kept().filter(ws => !ws._keepAliveId);
            marked.forEach(ws => (ws._keepAliveId = KEPT));
            try {
                return check.call(this);
            } finally {
                marked.filter(ws => ws._keepAliveId === KEPT).forEach(ws => (ws._keepAliveId = 0));
            }
        };
    }

    destroy() {
        delete this._tracker._checkWorkspaces;
        this._manager.disconnect(this._removedId);
        this._tracker._queueCheckWorkspaces();
    }

    /** The name kojed knows a workspace by. */
    name(workspace) {
        for (const [name, ws] of this._contexts) {
            if (ws === workspace)
                return name;
        }
        return `${this._others().indexOf(workspace) + 1}`;
    }

    activeName() {
        return this.name(this._manager.get_active_workspace());
    }

    /** The workspace of a name; a context's is made if it has none. */
    get(name) {
        if (/^[0-9]+$/.test(name)) {
            const ws = this._others()[Number(name) - 1];
            if (!ws)
                throw new Error(`no workspace ${name}`);
            return ws;
        }
        let ws = this._contexts.get(name);
        if (!ws) {
            ws = this._manager.append_new_workspace(false, global.get_current_time());
            this._contexts.set(name, ws);
        }
        return ws;
    }

    /** Keep exactly these contexts' workspaces while they are empty. */
    keep(names) {
        this._kept = new Set(names);
        this._tracker._queueCheckWorkspaces();
    }

    _all() {
        const all = [];
        for (let i = 0; i < this._manager.n_workspaces; i++)
            all.push(this._manager.get_workspace_by_index(i));
        return all;
    }

    /** The workspaces that are no context's, in order. */
    _others() {
        const contexts = new Set(this._contexts.values());
        return this._all().filter(ws => !contexts.has(ws));
    }

    _forgetRemoved() {
        const live = new Set(this._all());
        for (const [name, ws] of this._contexts) {
            if (!live.has(ws))
                this._contexts.delete(name);
        }
    }
}
