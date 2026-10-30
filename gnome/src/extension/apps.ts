// A context's programs in GNOME's app grid and search (ADR 0059, L4/L5). On
// a context's workspace the grid and search show that context's programs
// (kojed's `Apps`) instead of the host's and start them in the context
// (`LaunchApp`). The dash keeps GNOME's favorites; activating one prefers
// its windows on the current workspace and, on a context's workspace,
// starts the context's program with the same desktop file id if it has one.
// koje Settings belongs to no context: started again, its window comes to
// the workspace on screen instead of taking the user to its own.
//
// A context's program is a Shell.App made from an in-memory entry, with the
// id `koje-<context>-<desktop file id>`; GNOME's window → app mapping never
// sees it (L3). Internals: Shell.AppSystem's `lookup_app`, `get_installed`
// and `search` and Shell.App's `activate` and `open_new_window` as JS calls
// them (docs/upstream/gnome-apis.md).

import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import type Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {Kojed} from '../api/kojed.js';
import {isHost, type Workspaces} from './workspaces.js';

const SETTINGS = 'io.koje.Settings';

/** A context's program: its Shell.App and what the grid and search see. */
interface Program {
    app: Shell.App;
    listed: Listed;
    id: string;
    name: string;
    keywords: string[];
}

/** What `get_installed` lists for the grid. */
interface Listed {
    get_id(): string;
    should_show(): boolean;
    get_categories(): null;
}

/** Whether every word of `query` is in the program's name or keywords. */
function matches({name, keywords}: Program, query: string): boolean {
    const text = [name, ...keywords].join(' ').toLowerCase();
    return query.toLowerCase().split(/\s+/).filter(Boolean).every(t => text.includes(t));
}

/** The AppSystem's methods as JS calls them, replaced while enabled. */
interface Patched {
    lookup_app(id: string): Shell.App | null;
    get_installed(): unknown[];
}

export class Apps {
    #workspaces: Workspaces;
    #kojed = new Kojed();
    /** context → koje id → program */
    #contexts = new Map<string, Map<string, Program>>();
    #appSys: Shell.AppSystem;
    #orig;
    #activeId: number;
    #attentionId: number;

    constructor(workspaces: Workspaces) {
        this.#workspaces = workspaces;
        this.#appSys = Shell.AppSystem.get_default();

        const appSys = this.#appSys as unknown as Patched;
        const search = Shell.AppSystem as unknown as {search(query: string): string[][]};
        const App = Shell.App.prototype;
        this.#orig = {
            lookup: appSys.lookup_app,
            installed: appSys.get_installed,
            search: search.search,
            activate: App.activate,
            openNew: App.open_new_window,
        };
        const orig = this.#orig;
        const self = this;
        appSys.lookup_app = id => self.#lookup(id)?.app ?? orig.lookup.call(appSys, id);
        appSys.get_installed = () => {
            const programs = self.#current();
            return programs
                ? [...programs.values()].map(p => p.listed)
                : orig.installed.call(appSys);
        };
        search.search = query => {
            const programs = self.#current();
            if (!programs)
                return orig.search(query);
            const found = [...programs.entries()].filter(([, p]) => matches(p, query));
            return found.length > 0 ? [found.map(([id]) => id)] : [];
        };
        App.activate = function (this: Shell.App, ...args) {
            if (!self.#activateHere(this))
                orig.activate.apply(this, args);
        };
        App.open_new_window = function (this: Shell.App, ...args) {
            if (!self.#launchContexts(this))
                orig.openNew.apply(this, args);
        };

        this.#activeId = global.workspace_manager.connect('active-workspace-changed',
            () => void this.#load());
        // A second start presents the window without an activation token
        // (the menu's `koje settings`, a terminal): Mutter only marks it.
        this.#attentionId = global.display.connect('window-demands-attention',
            (_d, w) => this.#bringSettings(w));
        void this.#load();
    }

    destroy(): void {
        global.workspace_manager.disconnect(this.#activeId);
        global.display.disconnect(this.#attentionId);
        const appSys = this.#appSys as unknown as Partial<Patched>;
        delete appSys.lookup_app;
        delete appSys.get_installed;
        (Shell.AppSystem as unknown as {search: unknown}).search = this.#orig.search;
        Shell.App.prototype.activate = this.#orig.activate;
        Shell.App.prototype.open_new_window = this.#orig.openNew;
        this.#contexts.clear();
        this.#appSys.emit('installed-changed');
    }

    /** The programs of the context whose workspace is shown, or null. */
    #current(): Map<string, Program> | null {
        const name = this.#workspaces.activeName();
        return isHost(name) ? null : this.#contexts.get(name) ?? null;
    }

    #lookup(kojeId: string): Program | null {
        for (const programs of this.#contexts.values()) {
            const p = programs.get(kojeId);
            if (p)
                return p;
        }
        return null;
    }

    // Ask kojed for the shown context's programs; GNOME's grid and search
    // follow `installed-changed`.
    async #load(): Promise<void> {
        const context = this.#workspaces.activeName();
        if (isHost(context)) {
            this.#appSys.emit('installed-changed');
            return;
        }
        let list: [string, string, string, string[]][];
        try {
            [list] = await this.#kojed.call('Apps', context);
        } catch (e) {
            log(`koje: the programs of ${context}: ${e instanceof Error ? e.message : String(e)}`);
            list = [];
        }
        const programs = new Map<string, Program>();
        for (const [id, name, icon, keywords] of list)
            programs.set(`koje-${context}-${id}`, this.#program(context, id, name, icon, keywords));
        this.#contexts.set(context, programs);
        this.#appSys.emit('installed-changed');
    }

    #program(context: string, id: string, name: string, icon: string, keywords: string[]): Program {
        const kojeId = `koje-${context}-${id}`;
        const entry = new GLib.KeyFile();
        const group = 'Desktop Entry';
        entry.set_string(group, 'Type', 'Application');
        entry.set_string(group, 'Name', name);
        entry.set_string(group, 'Exec', 'true');
        if (icon)
            entry.set_string(group, 'Icon', icon);
        const app = new Shell.App({app_info: GioUnix.DesktopAppInfo.new_from_keyfile(entry)!});
        const launch = () => this.#launch(context, id);
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

    #launch(context: string, id: string): void {
        this.#kojed.call('LaunchApp', context, id).catch((e: Error) =>
            Main.notifyError(`Could not start ${id} in ${context}`, e.message));
    }

    // A running app's windows on the current workspace first (the same
    // program in several contexts is one app, L3); then the context's own
    // program of that id.
    #activateHere(app: Shell.App): boolean {
        if (app.get_id() === `${SETTINGS}.desktop`) {
            const [w] = app.get_windows();
            if (w)
                this.#bringSettings(w);
            return Boolean(w);
        }
        const workspace = global.workspace_manager.get_active_workspace();
        const here = app.get_windows().filter(w => w.get_workspace() === workspace);
        if (here.length > 0) {
            Main.activateWindow(here[0]);
            return true;
        }
        return this.#launchContexts(app);
    }

    /** koje Settings' window `w` onto the workspace on screen, focused. */
    #bringSettings(w: Meta.Window): void {
        if (w.get_wm_class() !== SETTINGS)
            return;
        const workspace = global.workspace_manager.get_active_workspace();
        if (!w.is_on_all_workspaces() && w.get_workspace() !== workspace)
            w.change_workspace(workspace);
        Main.activateWindow(w);
    }

    /** Start the shown context's program with `app`'s id instead, if it has one. */
    #launchContexts(app: Shell.App): boolean {
        const context = this.#workspaces.activeName();
        const p = this.#contexts.get(context)?.get(`koje-${context}-${app.get_id()}`);
        if (!p)
            return false;
        this.#launch(context, p.id);
        return true;
    }
}
