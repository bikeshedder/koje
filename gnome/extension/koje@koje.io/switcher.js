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
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as WorkspaceSwitcherPopup from 'resource:///org/gnome/shell/ui/workspaceSwitcherPopup.js';
import * as WorkspaceThumbnail from 'resource:///org/gnome/shell/ui/workspaceThumbnail.js';

function textOn(color) {
    const n = parseInt(color.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return 0.299 * r + 0.587 * g + 0.114 * b < 150 ? '#ffffff' : 'rgba(0, 0, 0, 0.87)';
}

function pill(text, color) {
    return new St.Label({
        text,
        y_align: Clutter.ActorAlign.CENTER,
        style: `background-color: ${color}; color: ${textOn(color)}; border-radius: 999px; ` +
            'padding: 1px 8px; font-size: 9pt; font-weight: bold;',
    });
}

export class Switcher {
    constructor(service, workspaces) {
        this._service = service;
        this._workspaces = workspaces;
        /** thumbnails decorated while the overview is open */
        this._thumbnails = new Set();

        const self = this;
        const Box = WorkspaceThumbnail.ThumbnailsBox.prototype;
        const Popup = WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup.prototype;
        this._orig = {add: Box.addThumbnails, redisplay: Popup.redisplay};
        const orig = this._orig;
        Box.addThumbnails = function (...args) {
            orig.add.apply(this, args);
            self._box = this;
            self.update();
        };
        Popup.redisplay = function (activeIndex, ...args) {
            orig.redisplay.call(this, activeIndex, ...args);
            self._popup(this, activeIndex);
        };

        this._dots = Main.panel.statusArea.activities?.get_first_child() ?? null;
        // The overview's thumbnails exist before the extension starts.
        this._box = Main.overview._overview?._controls?._thumbnailsBox ?? null;
        this._ids = [
            [global.workspace_manager, global.workspace_manager.connect('workspace-added', () => this.update())],
            [global.workspace_manager, global.workspace_manager.connect('workspace-removed', () => this.update())],
            [global.workspace_manager, global.workspace_manager.connect('workspaces-reordered', () => this.update())],
            ...this._dots ? [[this._dots, this._dots.connect('child-added', () => this.update())]] : [],
        ];
        this.update();
    }

    destroy() {
        this._ids.forEach(([o, id]) => o.disconnect(id));
        WorkspaceThumbnail.ThumbnailsBox.prototype.addThumbnails = this._orig.add;
        WorkspaceSwitcherPopup.MonitorWorkspaceSwitcherPopup.prototype.redisplay = this._orig.redisplay;
        for (const t of this._thumbnails)
            t._kojeMark?.destroy();
        this._thumbnails.clear();
        this._colorDots(true);
    }

    /** The context of a workspace and its color, or null for a host one. */
    _context(ws) {
        const name = ws ? this._workspaces.name(ws) : null;
        if (!name || /^[0-9]+$/.test(name))
            return null;
        return {name, color: this._service.rules.get(`koje-${name}`)?.active ?? '#808080'};
    }

    /** Colors and names again (kojed's colors or the workspaces changed). */
    update() {
        try {
            this._colorDots(false);
            for (const t of this._box?._thumbnails ?? [])
                this._thumbnail(t);
        } catch (e) {
            logError(e, 'koje: marking workspaces');
        }
    }

    _colorDots(reset) {
        const dots = this._dots?.get_children().filter(d => !d.destroying) ?? [];
        dots.forEach((dot, i) => {
            const c = reset ? null : this._context(global.workspace_manager.get_workspace_by_index(i));
            if (dot._dot)
                dot._dot.style = c ? `background-color: ${c.color};` : null;
        });
    }

    _thumbnail(thumbnail) {
        thumbnail._kojeMark?.destroy();
        thumbnail._kojeMark = null;
        const c = this._context(thumbnail.metaWorkspace);
        if (!c)
            return;
        if (!this._thumbnails.has(thumbnail)) {
            this._thumbnails.add(thumbnail);
            thumbnail.connect('destroy', () => this._thumbnails.delete(thumbnail));
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

    _popup(popup, activeIndex) {
        try {
            const list = popup._list;
            list.get_children().forEach((indicator, i) => {
                const c = this._context(global.workspace_manager.get_workspace_by_index(i));
                if (c)
                    indicator.style = `background-color: ${c.color};`;
            });
            const active = this._context(global.workspace_manager.get_workspace_by_index(activeIndex));
            if (active)
                list.insert_child_at_index(pill(active.name, active.color), 0);
        } catch (e) {
            logError(e, 'koje: marking the workspace switcher');
        }
    }
}
