// Workspace names for kojed: a context's workspace is named after the
// context, every other one by its number from 1 among those others, so a
// host workspace keeps its number when context workspaces come and go
// (context names never are numbers). A context's workspace is made when a
// command names it. GNOME removes empty workspaces; the ones kojed keeps
// (`keep-workspaces`) stay.

import type Meta from 'gi://Meta';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

// A `_keepAliveId` that is no GLib source: set only while GNOME's
// WorkspaceTracker checks, so its own keep-alive timers are never touched.
const KEPT = -1;

/** GNOME's WorkspaceTracker, internal (docs/upstream/gnome-apis.md). */
interface Tracker {
    _checkWorkspaces(): boolean;
    _queueCheckWorkspaces(): void;
}

type Kept = Meta.Workspace & {_keepAliveId?: number};

/** Whether a workspace name is a host workspace's (its number). */
export function isHost(name: string): boolean {
    return /^[0-9]+$/.test(name);
}

export class Workspaces {
    #manager = global.workspace_manager;
    /** context name → its workspace */
    #contexts = new Map<string, Meta.Workspace>();
    /** context names whose workspace stays while empty */
    #kept = new Set<string>();
    #removedId: number;
    #tracker: Tracker;

    constructor() {
        this.#removedId = this.#manager.connect('workspace-removed', () => this.#forgetRemoved());

        this.#tracker = (Main.wm as unknown as {_workspaceTracker: Tracker})._workspaceTracker;
        const check = this.#tracker._checkWorkspaces;
        const kept = () => [...this.#kept]
            .map(n => this.#contexts.get(n))
            .filter((ws): ws is Kept => ws !== undefined);
        this.#tracker._checkWorkspaces = function (this: Tracker) {
            const marked = kept().filter(ws => !ws._keepAliveId);
            marked.forEach(ws => (ws._keepAliveId = KEPT));
            try {
                return check.call(this);
            } finally {
                marked.filter(ws => ws._keepAliveId === KEPT).forEach(ws => (ws._keepAliveId = 0));
            }
        };
    }

    destroy(): void {
        delete (this.#tracker as Partial<Tracker>)._checkWorkspaces;
        this.#manager.disconnect(this.#removedId);
        this.#tracker._queueCheckWorkspaces();
    }

    /** The name kojed knows a workspace by. */
    name(workspace: Meta.Workspace): string {
        for (const [name, ws] of this.#contexts) {
            if (ws === workspace)
                return name;
        }
        return `${this.#others().indexOf(workspace) + 1}`;
    }

    activeName(): string {
        return this.name(this.#manager.get_active_workspace());
    }

    /** The workspace of a name; a context's is made if it has none. */
    get(name: string): Meta.Workspace {
        if (isHost(name)) {
            const ws = this.#others()[Number(name) - 1];
            if (!ws)
                throw new Error(`no workspace ${name}`);
            return ws;
        }
        let ws = this.#contexts.get(name);
        if (!ws) {
            ws = this.#manager.append_new_workspace(false, global.get_current_time());
            this.#contexts.set(name, ws);
        }
        return ws;
    }

    /** Keep exactly these contexts' workspaces while they are empty. */
    keep(names: string[]): void {
        this.#kept = new Set(names);
        this.#tracker._queueCheckWorkspaces();
    }

    #all(): Meta.Workspace[] {
        const all = [];
        for (let i = 0; i < this.#manager.n_workspaces; i++)
            all.push(this.#manager.get_workspace_by_index(i)!);
        return all;
    }

    /** The workspaces that are no context's, in order. */
    #others(): Meta.Workspace[] {
        const contexts = new Set(this.#contexts.values());
        return this.#all().filter(ws => !contexts.has(ws));
    }

    #forgetRemoved(): void {
        const live = new Set(this.#all());
        for (const [name, ws] of this.#contexts) {
            if (!live.has(ws))
                this.#contexts.delete(name);
        }
    }
}
