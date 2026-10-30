// koje's GNOME Shell extension (ADR 0059). Thin: it serves io.koje.Extension1
// for kojed (service.js) and draws what is koje's own. Every GNOME internal it
// touches is listed in docs/upstream/gnome-apis.md.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Apps} from './apps.js';
import {Foreign} from './foreign.js';
import {Service} from './service.js';
import {Sleep} from './sleep.js';
import {Switcher} from './switcher.js';
import {TopBar} from './indicator.js';
import {Windows} from './windows.js';
import {Workspaces} from './workspaces.js';

// kojed's user unit: in a GNOME session nothing else starts it (koje's own
// session does on Hyprland). GNOME gives systemd the session's environment.
function startKojed() {
    Gio.DBus.session.call('org.freedesktop.systemd1', '/org/freedesktop/systemd1',
        'org.freedesktop.systemd1.Manager', 'StartUnit',
        new GLib.Variant('(ss)', ['kojed.service', 'replace']), null,
        Gio.DBusCallFlags.NONE, -1, null, (conn, result) => {
            try {
                conn.call_finish(result);
            } catch (e) {
                log(`koje: starting kojed.service: ${e.message}`);
            }
        });
}

export default class KojeExtension extends Extension {
    enable() {
        startKojed();
        this._workspaces = new Workspaces();
        this._service = new Service(this.dir, this._workspaces);
        this._windows = new Windows(this._workspaces, this._service);
        this._service.windows = this._windows;
        this._foreign = new Foreign(this._service, this._workspaces);
        this._sleep = new Sleep(this._service);
        this._service.sleep = this._sleep;
        // Foreign and sleeping windows change with kojed's commands (tags,
        // colors, moves, sleep) and when the user moves a window.
        this._switcher = new Switcher(this._service, this._workspaces);
        const update = () => {
            this._foreign.update();
            this._sleep.update();
            this._switcher.update();
        };
        this._service.changed = update;
        this._windows.changed = update;
        this._apps = new Apps(this._workspaces);
        this._topBar = new TopBar(this._workspaces);
    }

    disable() {
        this._topBar.destroy();
        this._apps.destroy();
        this._switcher.destroy();
        this._sleep.destroy();
        this._foreign.destroy();
        this._windows.destroy();
        this._service.destroy();
        this._workspaces.destroy();
        this._topBar = null;
        this._apps = null;
        this._switcher = null;
        this._sleep = null;
        this._foreign = null;
        this._windows = null;
        this._service = null;
        this._workspaces = null;
    }
}
