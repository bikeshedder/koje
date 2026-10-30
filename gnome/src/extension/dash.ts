// A dash per context (ADR 0062, G.32). On a context's workspace GNOME's dash
// holds that context's favorites and running programs; on a host workspace
// the host's. Nothing crosses: a window belongs to the context of its
// `koje-<context>` tag (service.ts), untagged windows are the host's.
//
// Only the dash changes; Alt+Tab and the overview keep GNOME's apps. Each
// icon of a context's dash is the context's instance of the program (apps.ts'
// in-memory Shell.App) with that context's windows: its running dot, window
// list, "New Window" and "Quit" act there. In the host's dash an app that
// also has context windows is shown by a stand-in with the host's windows
// only. A context window that matches none of its programs (match.ts) shows
// as an app of its own with a generic icon.
//
// Favorites: on a context's workspace `AppFavorites` answers with the
// context's list (kojed's `Favorites`; unset, GNOME's favorites it has) and
// pinning, unpinning and moving go to kojed (`SetFavorites`); on the host
// it is GNOME's. Internals: the `AppFavorites` singleton's methods, the
// dash's `_appSystem` (its redisplay, bound when the dash was made, reads
// only `get_running()` from it), and `state`, `get_windows`,
// `activate`, `open_new_window` and `request_quit` of the Shell.Apps the dash
// gets, as JS calls them (docs/upstream/gnome-apis.md).

import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as AppFavorites from 'resource:///org/gnome/shell/ui/appFavorites.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {Kojed} from '../api/kojed.js';
import {badge} from './badge.js';
import type {Apps, Program} from './apps.js';
import * as Match from './match.js';
import type {Service} from './service.js';
import type {Windows} from './windows.js';
import {isHost, type Workspaces} from './workspaces.js';

/** The `AppFavorites` singleton's methods as JS calls them. */
interface Favorites {
    getFavoriteMap(): Record<string, Shell.App>;
    getFavorites(): Shell.App[];
    isFavorite(id: string): boolean;
    addFavorite(id: string): void;
    addFavoriteAtPos(id: string, pos: number): void;
    removeFavorite(id: string): void;
    moveFavoriteToPos(id: string, pos: number): void;
    emit(signal: string): void;
}
const PATCHED = ['getFavoriteMap', 'getFavorites', 'isFavorite', 'addFavorite',
    'addFavoriteAtPos', 'removeFavorite', 'moveFavoriteToPos'] as const;

/** GNOME's dash: what this module replaces on it. */
type ShellDash = {_appSystem: Shell.AppSystem; _queueRedisplay(): void};

/** A dash icon's app with the windows it stands for. */
interface Instance {
    app: Shell.App;
    windows: Meta.Window[];
}

/** Most recently used first, as GNOME's apps list their windows. */
function byUse(windows: Meta.Window[]): Meta.Window[] {
    const order = global.display.get_tab_list(Meta.TabList.NORMAL_ALL, null);
    return [...windows].sort((a, b) => order.indexOf(a) - order.indexOf(b));
}

export class Dash {
    #service: Service;
    #workspaces: Workspaces;
    #apps: Apps;
    #windows: Windows;
    #kojed = new Kojed();
    #favorites: Favorites;
    #orig;
    /** context → its own favorites (`null`: none of its own), as kojed last said */
    #own = new Map<string, string[] | null>();
    /** koje id → the instance for a context's program or unmatched app id */
    #instances = new Map<string, Instance>();
    /** host app id → its stand-in while it also has context windows */
    #standIns = new Map<string, Instance>();
    #ids: [{disconnect(id: number): void}, number][];
    #signalId: number;

    constructor(service: Service, workspaces: Workspaces, apps: Apps, windows: Windows) {
        this.#service = service;
        this.#workspaces = workspaces;
        this.#apps = apps;
        this.#windows = windows;

        const self = this;
        const favorites = AppFavorites.getAppFavorites() as unknown as Favorites;
        this.#favorites = favorites;
        const dash = Main.overview.dash as unknown as ShellDash;
        const orig = {
            appSystem: dash._appSystem,
            favorites: Object.fromEntries(PATCHED.map(m => [m, favorites[m].bind(favorites)])) as
                Pick<Favorites, typeof PATCHED[number]>,
        };
        this.#orig = orig;

        // The dash's running apps are its owner's. Its redisplay was bound
        // when it was made (a patched prototype method is never called);
        // after that it reads only `get_running()` from `_appSystem`.
        const real = orig.appSystem;
        dash._appSystem = new Proxy(real, {
            get(target, key) {
                if (key === 'get_running')
                    return () => self.#running();
                const value = Reflect.get(target, key) as unknown;
                return typeof value === 'function' ? value.bind(target) : value;
            },
        });
        favorites.getFavoriteMap = () => self.#favoriteMap();
        favorites.getFavorites = () => Object.values(self.#favoriteMap());
        favorites.isFavorite = id => {
            const context = self.#context();
            return context ? id in self.#favoriteMap() : orig.favorites.isFavorite(id);
        };
        favorites.addFavorite = id => favorites.addFavoriteAtPos(id, -1);
        favorites.addFavoriteAtPos = (id, pos) => self.#edit(id, pos, true) || orig.favorites.addFavoriteAtPos(id, pos);
        favorites.moveFavoriteToPos = (id, pos) => self.#edit(id, pos, true) || orig.favorites.moveFavoriteToPos(id, pos);
        favorites.removeFavorite = id => self.#edit(id, -1, false) || orig.favorites.removeFavorite(id);

        this.#ids = [
            [global.workspace_manager, global.workspace_manager.connect('active-workspace-changed',
                () => this.#ownerChanged())],
        ];
        this.#signalId = this.#kojed.connect('FavoritesChanged', context => void this.#loadFavorites(context));
        this.#ownerChanged();
    }

    destroy(): void {
        this.#ids.forEach(([o, id]) => o.disconnect(id));
        this.#kojed.disconnect(this.#signalId);
        (Main.overview.dash as unknown as ShellDash)._appSystem = this.#orig.appSystem;
        for (const m of PATCHED)
            delete (this.#favorites as Partial<Favorites>)[m];
        this.#instances.clear();
        this.#standIns.clear();
        this.#favorites.emit('changed');
        this.#redisplay();
    }

    /** Windows, tags or a context's programs changed: running apps again. */
    update(): void {
        try {
            this.#updateInstances();
            this.#redisplay();
        } catch (e) {
            logError(e as Error, 'koje: the dash');
        }
    }

    /** A context's programs were loaded (apps.ts). */
    programsLoaded(context: string): void {
        if (context === this.#context()) {
            this.#favorites.emit('changed');
            this.update();
        }
    }

    /** The context whose workspace is shown, null on a host workspace. */
    #context(): string | null {
        const name = this.#workspaces.activeName();
        return isHost(name) ? null : name;
    }

    #ownerChanged(): void {
        const context = this.#context();
        if (context && !this.#own.has(context))
            void this.#loadFavorites(context);
        this.#favorites.emit('changed');
        this.update();
    }

    async #loadFavorites(context: string): Promise<void> {
        try {
            const [own, ids] = await this.#kojed.call('Favorites', context);
            this.#own.set(context, own ? ids : null);
        } catch (e) {
            log(`koje: the favorites of ${context}: ${e instanceof Error ? e.message : String(e)}`);
            this.#own.set(context, null);
        }
        if (context === this.#context())
            this.#favorites.emit('changed');
    }

    #programs(context: string): Program[] {
        return [...this.#apps.programs(context).values()];
    }

    /** The shown context's favorites as desktop file ids. */
    #favoriteIds(context: string): string[] {
        const gnome = global.settings.get_strv('favorite-apps').map(id => ({
            id, name: Shell.AppSystem.get_default().lookup_app(id)?.get_name() ?? '',
        }));
        return Match.favorites(this.#own.get(context) ?? null, gnome, this.#programs(context));
    }

    #favoriteMap(): Record<string, Shell.App> {
        const context = this.#context();
        const map: Record<string, Shell.App> = {};
        if (context) {
            const programs = this.#apps.programs(context);
            for (const id of this.#favoriteIds(context)) {
                const kojeId = `koje-${context}-${id}`;
                const app = this.#instances.get(kojeId)?.app ?? programs.get(kojeId)?.app;
                if (app)
                    map[kojeId] = app;
            }
            return map;
        }
        for (const [id, app] of Object.entries(this.#orig.favorites.getFavoriteMap()))
            map[id] = this.#standIns.get(id)?.app ?? app;
        return map;
    }

    /**
     * Pin (`add`, at `pos`; -1 last), move or unpin a program of the shown
     * context: kojed writes its list. False on a host workspace (GNOME's).
     */
    #edit(kojeId: string, pos: number, add: boolean): boolean {
        const context = this.#context();
        if (!context)
            return false;
        const prefix = `koje-${context}-`;
        const id = kojeId.startsWith(prefix) ? kojeId.slice(prefix.length) : null;
        // Only the context's programs can be pinned (not a window's own app).
        if (!id || !this.#apps.programs(context).has(kojeId))
            return true;
        const ids = this.#favoriteIds(context).filter(i => i !== id);
        if (add)
            ids.splice(pos < 0 ? ids.length : Math.min(pos, ids.length), 0, id);
        this.#own.set(context, ids);
        this.#favorites.emit('changed');
        this.#kojed.call('SetFavorites', context, true, ids).catch((e: Error) =>
            Main.notifyError(`Could not change the favorites of ${context}`, e.message));
        return true;
    }

    /** The dash's running apps: its owner's, most recently used first. */
    #running(): Shell.App[] {
        const context = this.#context();
        const out: Shell.App[] = [];
        const add = (app: Shell.App | undefined) => {
            if (app && !out.includes(app))
                out.push(app);
        };
        if (context) {
            for (const w of byUse(this.#windowsOf(context)))
                add([...this.#instances.values()].find(i => i.windows.includes(w))?.app);
            return out;
        }
        const appSys = Shell.AppSystem.get_default();
        for (const app of appSys.get_running()) {
            if (this.#hostWindows(app).length > 0)
                add(this.#standIns.get(app.get_id())?.app ?? app);
        }
        return out;
    }

    /** The windows of a context (by tag), or of the host (`null`: untagged). */
    #windowsOf(context: string | null): Meta.Window[] {
        return this.#windows.all().filter(w => {
            const tag = this.#service.tags.get(w.get_id());
            const owner = tag?.startsWith('koje-') ? tag.slice('koje-'.length) : null;
            return owner === context && !w.is_skip_taskbar();
        });
    }

    #hostWindows(app: Shell.App): Meta.Window[] {
        const host = new Set(this.#windowsOf(null));
        return app.get_windows().filter(w => host.has(w));
    }

    // The instances of the shown context's running programs and the
    // stand-ins for host apps that also have context windows.
    #updateInstances(): void {
        const context = this.#context();
        const seen = new Set<string>();
        if (context) {
            const programs = this.#programs(context);
            const groups = new Map<string, {program: Program | null; appId: string; windows: Meta.Window[]}>();
            for (const w of this.#windowsOf(context)) {
                const appId = w.get_wm_class() ?? '';
                const program = Match.programFor(appId, programs);
                const kojeId = program ? `koje-${context}-${program.id}` : `koje-${context}-window-${appId}`;
                const g = groups.get(kojeId) ?? {program, appId, windows: []};
                g.windows.push(w);
                groups.set(kojeId, g);
            }
            // Favorites without windows keep their (stopped) instance.
            for (const [kojeId, p] of this.#apps.programs(context)) {
                if (!groups.has(kojeId) && this.#instances.has(kojeId))
                    groups.set(kojeId, {program: p, appId: '', windows: []});
            }
            for (const [kojeId, g] of groups) {
                seen.add(kojeId);
                const app = g.program?.app ?? this.#instances.get(kojeId)?.app ?? this.#windowApp(kojeId, g.appId);
                this.#setWindows(this.#instances, kojeId, app, g.windows,
                    g.program ? () => this.#apps.launch(context, g.program!.id) : null);
            }
        }
        for (const id of [...this.#instances.keys()]) {
            if (!seen.has(id))
                this.#instances.delete(id);
        }
        // Host apps with context windows: the host's dash shows a stand-in.
        const standIns = new Set<string>();
        if (!context) {
            const tagged = new Set(this.#windows.all().filter(w => !this.#windowsOf(null).includes(w)));
            for (const app of Shell.AppSystem.get_default().get_running()) {
                const info = app.get_app_info();
                if (!info || !app.get_windows().some(w => tagged.has(w)))
                    continue;
                const id = app.get_id();
                standIns.add(id);
                const standIn = this.#standIns.get(id)?.app ?? this.#standIn(app, info as GioUnix.DesktopAppInfo);
                this.#setWindows(this.#standIns, id, standIn, this.#hostWindows(app), null);
            }
        }
        for (const id of [...this.#standIns.keys()]) {
            if (!standIns.has(id))
                this.#standIns.delete(id);
        }
    }

    /**
     * Give `app` (a context's program, a window's own app or a stand-in) the
     * windows it shows; `launch` starts it anew (null: it cannot).
     */
    #setWindows(map: Map<string, Instance>, id: string, app: Shell.App, windows: Meta.Window[],
        launch: (() => void) | null): void {
        const before = map.get(id);
        const instance: Instance = before ?? {app, windows: []};
        const changed = !before || before.windows.length !== windows.length ||
            before.windows.some((w, i) => w !== windows[i]);
        instance.windows = windows;
        map.set(id, instance);
        if (before)
            return void (changed && this.#notify(app));
        const own = (w: Meta.Window[]) => byUse(w);
        Object.defineProperty(app, 'state', {
            configurable: true,
            get: () => instance.windows.length > 0 ? Shell.AppState.RUNNING : Shell.AppState.STOPPED,
        });
        const show = () => {
            const [w] = own(instance.windows);
            if (w)
                Main.activateWindow(w);
            else
                launch?.();
        };
        Object.assign(app, {
            get_windows: () => own(instance.windows),
            get_n_windows: () => instance.windows.length,
            is_window_backed: () => false,
            activate: show,
            activate_full: show,
            request_quit: () => {
                instance.windows.forEach(w => w.delete(global.get_current_time()));
                return true;
            },
            ...launch ? {} : {can_open_new_window: () => false},
        });
        this.#notify(app);
    }

    #notify(app: Shell.App): void {
        app.notify('state');
        app.emit('windows-changed');
    }

    /** An app of its own for a context's windows that match no program. */
    #windowApp(kojeId: string, appId: string): Shell.App {
        const entry = new GLib.KeyFile();
        const group = 'Desktop Entry';
        entry.set_string(group, 'Type', 'Application');
        entry.set_string(group, 'Name', appId || 'Window');
        entry.set_string(group, 'Exec', 'true');
        const app = new Shell.App({app_info: GioUnix.DesktopAppInfo.new_from_keyfile(entry)!});
        Object.assign(app, {get_id: () => kojeId, open_new_window: () => {}, launch: () => {}});
        const context = this.#context();
        if (context)
            badge(app, context, () => this.#apps.color(context));
        return app;
    }

    /** A host app shown with its host windows only; it starts as the app does. */
    #standIn(app: Shell.App, info: GioUnix.DesktopAppInfo): Shell.App {
        const standIn = new Shell.App({app_info: info});
        Object.assign(standIn, {
            get_id: () => app.get_id(),
            open_new_window: (workspace: number) => app.open_new_window(workspace),
            launch: (...args: unknown[]) => (app.launch as (...a: unknown[]) => boolean)(...args),
            launch_action: (...args: unknown[]) => (app.launch_action as (...a: unknown[]) => void)(...args),
        });
        return standIn;
    }

    #redisplay(): void {
        (Main.overview.dash as unknown as ShellDash)._queueRedisplay();
    }
}
