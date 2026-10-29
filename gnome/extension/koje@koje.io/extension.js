// koje's GNOME Shell extension (ADR 0059). Thin: it serves io.koje.Extension1
// for kojed (service.js) and draws what is koje's own. Every GNOME internal it
// touches is listed in docs/upstream/gnome-apis.md.

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Service} from './service.js';
import {Windows} from './windows.js';
import {Workspaces} from './workspaces.js';

export default class KojeExtension extends Extension {
    enable() {
        this._workspaces = new Workspaces();
        this._service = new Service(this.dir, this._workspaces);
        this._windows = new Windows(this._workspaces, this._service);
        this._service.windows = this._windows;
    }

    disable() {
        this._windows.destroy();
        this._service.destroy();
        this._workspaces.destroy();
        this._windows = null;
        this._service = null;
        this._workspaces = null;
    }
}
