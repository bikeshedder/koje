// koje's GNOME Shell extension (ADR 0059). Thin: it serves io.koje.Extension1
// for kojed (service.js) and draws what is koje's own. Every GNOME internal it
// touches is listed in docs/upstream/gnome-apis.md.

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Apps} from './apps.js';
import {Foreign} from './foreign.js';
import {Service} from './service.js';
import {Sleep} from './sleep.js';
import {Windows} from './windows.js';
import {Workspaces} from './workspaces.js';

export default class KojeExtension extends Extension {
    enable() {
        this._workspaces = new Workspaces();
        this._service = new Service(this.dir, this._workspaces);
        this._windows = new Windows(this._workspaces, this._service);
        this._service.windows = this._windows;
        this._foreign = new Foreign(this._service, this._workspaces);
        this._sleep = new Sleep(this._service);
        this._service.sleep = this._sleep;
        // Foreign and sleeping windows change with kojed's commands (tags,
        // colors, moves, sleep) and when the user moves a window.
        const update = () => {
            this._foreign.update();
            this._sleep.update();
        };
        this._service.changed = update;
        this._windows.changed = update;
        this._apps = new Apps(this._workspaces);
    }

    disable() {
        this._apps.destroy();
        this._sleep.destroy();
        this._foreign.destroy();
        this._windows.destroy();
        this._service.destroy();
        this._workspaces.destroy();
        this._apps = null;
        this._sleep = null;
        this._foreign = null;
        this._windows = null;
        this._service = null;
        this._workspaces = null;
    }
}
