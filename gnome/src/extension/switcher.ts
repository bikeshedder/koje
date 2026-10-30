// Context workspaces apart from host ones wherever GNOME shows workspaces
// (decided 2026-10-30): in the context's color, with its name where there is
// room. Host workspaces stay as GNOME draws them.
// - the top bar's workspace dots (Activities button): the context's color;
// - the overview's workspace thumbnails: a frame in its color and its name;
// - the popup when switching with the keyboard: dots in color, the target's
//   name in front of them.
// Colors are kojed's `context-rule`s. Internals: the Activities button's
// dots, ThumbnailsBox's `addThumbnails` and `_thumbnails` (WorkspaceThumbnail's
// `_init` cannot be patched) and MonitorWorkspaceSwitcherPopup's `redisplay`
// (docs/upstream/gnome-apis.md).

import Clutter from 'gi://Clutter';
import type GObject from 'gi://GObject';
import type Meta from 'gi://Meta';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as WorkspaceSwitcherPopup from 'resource:///org/gnome/shell/ui/workspaceSwitcherPopup.js';
import * as WorkspaceThumbnail from 'resource:///org/gnome/shell/ui/workspaceThumbnail.js';

import {textOn} from './colors.js';
import type {Service} from './service.js';
import {isHost, type Workspaces} from './workspaces.js';

/** GNOME's internals this module reads (docs/upstream/gnome-apis.md). */
type Thumbnail = St.Widget & {metaWorkspace: Meta.Workspace; _kojeMark?: St.Widget | null};
type Box = {_thumbnails: Thumbnail[]};
type Dot = Clutter.Actor & {destroying?: boolean; _dot?: St.Widget};
type AddThumbnails = {addThumbnails(...args: unknown[]): void};

function pill(text: string, color: string): St.Label {
    return new St.Label({
        text,
        y_align: Clutter.ActorAlign.CENTER,
        style: `background-color: ${color}; color: ${textOn(color)}; border-radius: 999px; ` +
            'padding: 1px 8px; font-size: 9pt; font-weight: bold;',
    });
}

export class Switcher {
    #service: Service;
    #workspaces: Workspaces;
    /** thumbnails decorated while the overview is open */
    #thumbnails = new Set<Thumbnail>();
    #box: Box | null;
    #dots: Clutter.Actor | null;
    #orig;
    #ids: [GObject.Object, number][];

    constructor(service: Service, workspaces: Workspaces) {
        this.#service = service;
        this.#workspaces = workspaces;

        const self = this;
        const Box = WorkspaceThumbnail.ThumbnailsBox.prototype as unknown as AddThumbnails;
        const Popup = WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup.prototype;
        this.#orig = {add: Box.addThumbnails, redisplay: Popup.redisplay};
        const orig = this.#orig;
        Box.addThumbnails = function (this: WorkspaceThumbnail.ThumbnailsBox, ...args: unknown[]) {
            orig.add.apply(this, args);
            self.#box = this as unknown as Box;
            self.update();
        };
        Popup.redisplay = function (this: WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup,
            activeIndex: number, ...args: unknown[]) {
            orig.redisplay.call(this, activeIndex, ...args);
            self.#popup(this, activeIndex);
        };

        this.#dots = Main.panel.statusArea.activities?.get_first_child() ?? null;
        // The overview's thumbnails exist before the extension starts.
        const overview = Main.overview as unknown as {_overview?: {_controls?: {_thumbnailsBox?: Box}}};
        this.#box = overview._overview?._controls?._thumbnailsBox ?? null;
        const manager = global.workspace_manager;
        this.#ids = [
            [manager, manager.connect('workspace-added', () => this.update())],
            [manager, manager.connect('workspace-removed', () => this.update())],
            [manager, manager.connect('workspaces-reordered', () => this.update())],
            ...this.#dots ? [[this.#dots, this.#dots.connect('child-added', () => this.update())] as [GObject.Object, number]] : [],
        ];
        this.update();
    }

    destroy(): void {
        this.#ids.forEach(([o, id]) => o.disconnect(id));
        (WorkspaceThumbnail.ThumbnailsBox.prototype as unknown as AddThumbnails).addThumbnails = this.#orig.add;
        WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup.prototype.redisplay = this.#orig.redisplay;
        for (const t of this.#thumbnails) {
            t._kojeMark?.destroy();
            t._kojeMark = null;
        }
        this.#thumbnails.clear();
        this.#colorDots(true);
    }

    /** The context of a workspace and its color, or null for a host one. */
    #context(ws: Meta.Workspace | null): {name: string; color: string} | null {
        const name = ws ? this.#workspaces.name(ws) : null;
        if (!name || isHost(name))
            return null;
        return {name, color: this.#service.rules.get(`koje-${name}`)?.active ?? '#808080'};
    }

    /** Colors and names again (kojed's colors or the workspaces changed). */
    update(): void {
        try {
            this.#colorDots(false);
            for (const t of this.#box?._thumbnails ?? [])
                this.#thumbnail(t);
        } catch (e) {
            logError(e as Error, 'koje: marking workspaces');
        }
    }

    #colorDots(reset: boolean): void {
        const dots = (this.#dots?.get_children() ?? []) as Dot[];
        dots.filter(d => !d.destroying).forEach((dot, i) => {
            const c = reset ? null : this.#context(global.workspace_manager.get_workspace_by_index(i));
            if (dot._dot)
                dot._dot.style = c ? `background-color: ${c.color};` : null;
        });
    }

    #thumbnail(thumbnail: Thumbnail): void {
        thumbnail._kojeMark?.destroy();
        thumbnail._kojeMark = null;
        const c = this.#context(thumbnail.metaWorkspace);
        if (!c)
            return;
        if (!this.#thumbnails.has(thumbnail)) {
            this.#thumbnails.add(thumbnail);
            thumbnail.connect('destroy', () => this.#thumbnails.delete(thumbnail));
        }
        const mark = new St.Widget({
            reactive: false,
            style: `border: 3px solid ${c.color}; border-radius: 6px;`,
        });
        mark.add_constraint(new Clutter.BindConstraint({
            source: thumbnail, coordinate: Clutter.BindCoordinate.SIZE,
        }));
        const label = pill(c.name, c.color);
        label.set_position(6, 6);
        mark.add_child(label);
        thumbnail.add_child(mark);
        thumbnail._kojeMark = mark;
    }

    #popup(popup: WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup, activeIndex: number): void {
        try {
            const list = popup._list;
            (list.get_children() as St.Widget[]).forEach((indicator, i) => {
                const c = this.#context(global.workspace_manager.get_workspace_by_index(i));
                if (c)
                    indicator.style = `background-color: ${c.color};`;
            });
            const active = this.#context(global.workspace_manager.get_workspace_by_index(activeIndex));
            if (active)
                list.insert_child_at_index(pill(active.name, active.color), 0);
        } catch (e) {
            logError(e as Error, 'koje: marking the workspace switcher');
        }
    }
}
