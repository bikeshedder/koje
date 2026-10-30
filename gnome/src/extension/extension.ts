// koje's GNOME Shell extension (ADR 0059). Thin: it serves io.koje.Extension1
// for kojed (service.ts) and draws what is koje's own. Every GNOME internal it
// touches is listed in docs/upstream/gnome-apis.md. Written in TypeScript
// (ADR 0060); `just ui` bundles it into build/extension/koje@koje.io.

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

Gio._promisify(Gio.DBusConnection.prototype, 'call');

// kojed's user unit: in a GNOME session nothing else starts it.
function startKojed(): void {
    Gio.DBus.session.call('org.freedesktop.systemd1', '/org/freedesktop/systemd1',
        'org.freedesktop.systemd1.Manager', 'StartUnit',
        new GLib.Variant('(ss)', ['kojed.service', 'replace']), null,
        Gio.DBusCallFlags.NONE, -1, null)
        .catch((e: Error) => log(`koje: starting kojed.service: ${e.message}`));
}

/** What runs while the extension is enabled. */
interface Parts {
    workspaces: Workspaces;
    service: Service;
    windows: Windows;
    foreign: Foreign;
    sleep: Sleep;
    switcher: Switcher;
    apps: Apps;
    topBar: TopBar;
}

export default class KojeExtension extends Extension {
    #parts: Parts | null = null;

    override enable(): void {
        startKojed();
        const workspaces = new Workspaces();
        const service = new Service(this.dir, workspaces);
        const windows = new Windows(workspaces, service);
        service.windows = windows;
        const foreign = new Foreign(service, workspaces);
        const sleep = new Sleep(service);
        service.sleep = sleep;
        // Foreign and sleeping windows change with kojed's commands (tags,
        // colors, moves, sleep) and when the user moves a window.
        const switcher = new Switcher(service, workspaces);
        const update = () => {
            foreign.update();
            sleep.update();
            switcher.update();
        };
        service.changed = update;
        windows.changed = update;
        this.#parts = {
            workspaces, service, windows, foreign, sleep, switcher,
            apps: new Apps(workspaces),
            topBar: new TopBar(workspaces),
        };
    }

    override disable(): void {
        const p = this.#parts;
        this.#parts = null;
        if (!p)
            return;
        p.topBar.destroy();
        p.apps.destroy();
        p.switcher.destroy();
        p.sleep.destroy();
        p.foreign.destroy();
        p.windows.destroy();
        p.service.destroy();
        p.workspaces.destroy();
    }
}
