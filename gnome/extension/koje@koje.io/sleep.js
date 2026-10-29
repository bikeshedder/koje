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
// - kojed's moves and resizes skip it (service.js);
// - GNOME's "not responding" dialog for it is answered "Wait" at once.
// Internals: Meta.Window/Meta.Workspace activation and CloseDialog's
// `_initDialog` patched (docs/upstream/gnome-apis.md).

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';

import * as CloseDialog from 'resource:///org/gnome/shell/ui/closeDialog.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {follow, frameIn, offsets, restack} from './follow.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');
const CENTER = Clutter.ActorAlign.CENTER;

export class Sleep {
    constructor(service) {
        this._service = service;
        /** sleeping contexts */
        this._asleep = new Set();
        /** window id → {window, actor, cover, ids, actorId} */
        this._covers = new Map();

        const self = this;
        const time = () => global.get_current_time();
        this._orig = {
            activate: Meta.Window.prototype.activate,
            activateWithWorkspace: Meta.Window.prototype.activate_with_workspace,
            withFocus: Meta.Workspace.prototype.activate_with_focus,
            initDialog: CloseDialog.CloseDialog.prototype._initDialog,
        };
        const orig = this._orig;
        // Show a sleeping window's workspace instead of activating it.
        Meta.Window.prototype.activate = function (...args) {
            if (self.asleep(this))
                this.get_workspace()?.activate(time());
            else
                orig.activate.apply(this, args);
        };
        Meta.Window.prototype.activate_with_workspace = function (...args) {
            if (self.asleep(this))
                this.get_workspace()?.activate(time());
            else
                orig.activateWithWorkspace.apply(this, args);
        };
        Meta.Workspace.prototype.activate_with_focus = function (window, ...args) {
            if (self.asleep(window))
                this.activate(time());
            else
                orig.withFocus.call(this, window, ...args);
        };
        CloseDialog.CloseDialog.prototype._initDialog = function (...args) {
            orig.initDialog.apply(this, args);
            if (self.asleep(this._window))
                GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                    this._onWait();
                    return GLib.SOURCE_REMOVE;
                });
        };

        this._displayIds = [
            global.display.connect('notify::focus-window', () => this._keepFocusAway()),
            global.display.connect('restacked', () => this._covers.forEach(
                c => restack(c.actor, [], [c.cover]))),
        ];
    }

    destroy() {
        this._displayIds.forEach(id => global.display.disconnect(id));
        Meta.Window.prototype.activate = this._orig.activate;
        Meta.Window.prototype.activate_with_workspace = this._orig.activateWithWorkspace;
        Meta.Workspace.prototype.activate_with_focus = this._orig.withFocus;
        CloseDialog.CloseDialog.prototype._initDialog = this._orig.initDialog;
        for (const id of [...this._covers.keys()])
            this._uncover(id);
    }

    /** kojed's `sleep` command. */
    set(context, asleep) {
        if (asleep)
            this._asleep.add(context);
        else
            this._asleep.delete(context);
        this.update();
    }

    /** The context of a window when it sleeps, else null. */
    contextAsleep(w) {
        const tag = w ? this._service.tags.get(w.get_id()) : null;
        const context = tag?.startsWith('koje-') ? tag.slice('koje-'.length) : null;
        return context && this._asleep.has(context) ? context : null;
    }

    asleep(w) {
        return this.contextAsleep(w) !== null;
    }

    /** Cover exactly the sleeping windows; a failure is logged, not thrown. */
    update() {
        try {
            const covered = new Set();
            for (const w of this._service.windows?.all() ?? []) {
                const context = this.contextAsleep(w);
                if (context) {
                    covered.add(w.get_id());
                    this._cover(w, context);
                }
            }
            for (const id of [...this._covers.keys()]) {
                if (!covered.has(id))
                    this._uncover(id);
            }
            this._keepFocusAway();
        } catch (e) {
            logError(e, 'koje: covering sleeping windows');
        }
    }

    _cover(w, context) {
        const id = w.get_id();
        if (this._covers.has(id))
            return;
        const actor = w.get_compositor_private();
        if (!actor)
            return;
        // Reactive: it takes the pointer, nothing reaches the window.
        const cover = new St.BoxLayout({
            name: 'koje-sleep',
            reactive: true,
            vertical: true,
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
        wake.connect('clicked', () => this._wake(context));
        cover.add_child(wake);
        follow(cover, actor);
        const c = {window: w, actor, cover};
        const place = () => {
            const {x, y, frame} = frameIn(w);
            offsets(cover, x, y);
            cover.set_size(frame.width, frame.height);
        };
        place();
        restack(actor, [], [cover]);
        c.ids = [w.connect('size-changed', place)];
        c.actorId = actor.connect('destroy', () => this._uncover(id));
        this._covers.set(id, c);
    }

    _uncover(id) {
        const c = this._covers.get(id);
        if (!c)
            return;
        this._covers.delete(id);
        c.ids.forEach(i => c.window.disconnect(i));
        c.actor.disconnect(c.actorId);
        c.cover.destroy();
    }

    // A sleeping window got the focus anyway (Mutter chose it): the topmost
    // awake window of the workspace gets it, else nothing has it.
    _keepFocusAway() {
        const focused = global.display.focus_window;
        if (!this.asleep(focused))
            return;
        const time = global.get_current_time();
        const workspace = global.workspace_manager.get_active_workspace();
        const awake = global.display.sort_windows_by_stacking(workspace.list_windows())
            .filter(w => !this.asleep(w) && !w.minimized);
        const top = awake[awake.length - 1];
        if (top)
            this._orig.activate.call(top, time);
        else
            global.display.unset_input_focus(time);
    }

    _wake(context) {
        Gio.DBus.session.call('io.koje.Daemon', '/io/koje/Daemon', 'io.koje.Daemon1', 'Wake',
            new GLib.Variant('(s)', [context]), null, Gio.DBusCallFlags.NONE, -1, null)
            .catch(e => Main.notifyError(`Could not wake ${context}`, e.message));
    }
}
