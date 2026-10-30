// Sleeping contexts (ADR 0024; ADR 0059, L8): kojed's `sleep` command. A
// sleeping context's windows stay where they are under an overlay naming
// the context, with a button that wakes it (kojed's `Wake`).
//
// Its programs are frozen. Mutter queues what it sends a frozen program, at
// most 1 MiB; when that is full it drops the program at the thaw, with all
// its windows (G.3). So no input reaches a sleeping window:
// - the overlay takes the pointer;
// - it is never activated (Alt+Tab, dash, overview: its workspace is shown
//   instead), and a focus it gets anyway goes to another window;
// - kojed's moves and resizes skip it (service.ts);
// - GNOME's "not responding" dialog for it is answered "Wait" at once.
// Internals: Meta.Window/Meta.Workspace activation and CloseDialog's
// `_initDialog` patched (docs/upstream/gnome-apis.md).

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';

import * as CloseDialog from 'resource:///org/gnome/shell/ui/closeDialog.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {Kojed} from '../api/kojed.js';
import {follow, frameIn, offsets, restack} from './follow.js';
import type {Service} from './service.js';

const CENTER = Clutter.ActorAlign.CENTER;

interface Cover {
    window: Meta.Window;
    actor: Clutter.Actor;
    cover: St.BoxLayout;
    ids: number[];
    actorId: number;
}

export class Sleep {
    #service: Service;
    #kojed = new Kojed();
    /** sleeping contexts */
    #asleep = new Set<string>();
    /** window id → its cover */
    #covers = new Map<number, Cover>();
    #orig;
    #displayIds: number[];

    constructor(service: Service) {
        this.#service = service;

        const self = this;
        const time = () => global.get_current_time();
        const Dialog = CloseDialog.CloseDialog.prototype;
        this.#orig = {
            activate: Meta.Window.prototype.activate,
            activateWithWorkspace: Meta.Window.prototype.activate_with_workspace,
            withFocus: Meta.Workspace.prototype.activate_with_focus,
            initDialog: Dialog._initDialog,
        };
        const orig = this.#orig;
        // Show a sleeping window's workspace instead of activating it.
        Meta.Window.prototype.activate = function (this: Meta.Window, ...args) {
            if (self.asleep(this))
                this.get_workspace()?.activate(time());
            else
                orig.activate.apply(this, args);
        };
        Meta.Window.prototype.activate_with_workspace = function (this: Meta.Window, ...args) {
            if (self.asleep(this))
                this.get_workspace()?.activate(time());
            else
                orig.activateWithWorkspace.apply(this, args);
        };
        Meta.Workspace.prototype.activate_with_focus = function (this: Meta.Workspace, window, ...args) {
            if (self.asleep(window))
                this.activate(time());
            else
                orig.withFocus.call(this, window, ...args);
        };
        Dialog._initDialog = function (this: CloseDialog.CloseDialog) {
            orig.initDialog.call(this);
            if (self.asleep(this._window)) {
                GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                    this._onWait();
                    return GLib.SOURCE_REMOVE;
                });
            }
        };

        this.#displayIds = [
            global.display.connect('notify::focus-window', () => this.#keepFocusAway()),
            global.display.connect('restacked', () => this.#covers.forEach(
                c => restack(c.actor, [], [c.cover]))),
        ];
    }

    destroy(): void {
        this.#displayIds.forEach(id => global.display.disconnect(id));
        Meta.Window.prototype.activate = this.#orig.activate;
        Meta.Window.prototype.activate_with_workspace = this.#orig.activateWithWorkspace;
        Meta.Workspace.prototype.activate_with_focus = this.#orig.withFocus;
        CloseDialog.CloseDialog.prototype._initDialog = this.#orig.initDialog;
        for (const id of [...this.#covers.keys()])
            this.#uncover(id);
    }

    /** kojed's `sleep` command. */
    set(context: string, asleep: boolean): void {
        if (asleep)
            this.#asleep.add(context);
        else
            this.#asleep.delete(context);
        this.update();
    }

    /** The context of a window when it sleeps, else null. */
    contextAsleep(w: Meta.Window | null): string | null {
        const tag = w ? this.#service.tags.get(w.get_id()) : undefined;
        const context = tag?.startsWith('koje-') ? tag.slice('koje-'.length) : null;
        return context && this.#asleep.has(context) ? context : null;
    }

    asleep(w: Meta.Window | null): boolean {
        return this.contextAsleep(w) !== null;
    }

    /** Cover exactly the sleeping windows; a failure is logged, not thrown. */
    update(): void {
        try {
            const covered = new Set<number>();
            for (const w of this.#service.windows?.all() ?? []) {
                const context = this.contextAsleep(w);
                if (context) {
                    covered.add(w.get_id());
                    this.#cover(w, context);
                }
            }
            for (const id of [...this.#covers.keys()]) {
                if (!covered.has(id))
                    this.#uncover(id);
            }
            this.#keepFocusAway();
        } catch (e) {
            logError(e as Error, 'koje: covering sleeping windows');
        }
    }

    #cover(w: Meta.Window, context: string): void {
        const id = w.get_id();
        if (this.#covers.has(id))
            return;
        const actor = w.get_compositor_private<Meta.WindowActor>();
        if (!actor)
            return;
        // Reactive: it takes the pointer, nothing reaches the window.
        const cover = new St.BoxLayout({
            name: 'koje-sleep',
            reactive: true,
            orientation: Clutter.Orientation.VERTICAL,
            x_align: CENTER,
            y_align: CENTER,
            style: 'background-color: rgba(0, 0, 0, 0.6); spacing: 12px;',
        });
        cover.add_child(new St.Label({
            text: `${context} is asleep`,
            style: 'color: white; font-size: 14pt; font-weight: bold;',
            x_align: CENTER,
        }));
        const wake = new St.Button({label: 'Wake up', style_class: 'button', x_align: CENTER});
        wake.connect('clicked', () => this.#wake(context));
        cover.add_child(wake);
        follow(cover, actor);
        const place = () => {
            const {x, y, frame} = frameIn(w);
            offsets(cover, x, y);
            cover.set_size(frame.width, frame.height);
        };
        place();
        restack(actor, [], [cover]);
        this.#covers.set(id, {
            window: w, actor, cover,
            ids: [w.connect('size-changed', place)],
            actorId: actor.connect('destroy', () => this.#uncover(id)),
        });
    }

    #uncover(id: number): void {
        const c = this.#covers.get(id);
        if (!c)
            return;
        this.#covers.delete(id);
        c.ids.forEach(i => c.window.disconnect(i));
        c.actor.disconnect(c.actorId);
        c.cover.destroy();
    }

    // A sleeping window got the focus anyway (Mutter chose it): the topmost
    // awake window of the workspace gets it, else nothing has it.
    #keepFocusAway(): void {
        const focused = global.display.focus_window;
        if (!this.asleep(focused))
            return;
        const time = global.get_current_time();
        const workspace = global.workspace_manager.get_active_workspace();
        const awake = global.display.sort_windows_by_stacking(workspace.list_windows())
            .filter(w => !this.asleep(w) && !w.minimized);
        const top = awake[awake.length - 1];
        if (top)
            this.#orig.activate.call(top, time);
        else
            global.display.unset_input_focus(time);
    }

    #wake(context: string): void {
        this.#kojed.call('Wake', context)
            .catch((e: Error) => Main.notifyError(`Could not wake ${context}`, e.message));
    }
}
