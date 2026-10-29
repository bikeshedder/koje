// Windows outside their context (ADR 0057): a context's window on another
// context's workspace or a host workspace glows in the context's color and
// carries a badge with the context's name on its top edge. Host windows
// never do. A window's context is kojed's `koje-<context>` tag
// (`tag-window`); its color the active color of the window's own
// `border-colors` (kojed's focus blip) or of the tag's `context-rule`.
//
// Glow and badge are siblings of the window's actor, right below and above
// it, bound to its position and visibility. The glow is drawn with Cairo
// inside its own area, in four strips along the window's edges: St's
// `box-shadow` left trails while the window was dragged, and one area as
// large as the window made dragging slow (both seen in the GNOME lab).

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

// The glow's strength: focused, unfocused (as the Hyprland plugin's).
const GLOW_FOCUSED = 0.9;
const GLOW_UNFOCUSED = 0.55;
// How far the glow reaches out from the window, and in below its edges
// (where rounded window corners leave room), in px.
const REACH = 24;
const INNER = 8;
// Corner radius of the glow's inner edge: small, so it hugs square corners;
// below rounded ones its fill shows a little, as a shadow would.
const CORNER = 4;
// The badge's distance from the window's left edge.
const BADGE_LEFT = 14;

/** A rounded rectangle path. */
function rounded(cr, x, y, w, h, r) {
    cr.newSubPath();
    cr.arc(x + w - r, y + r, r, -Math.PI / 2, 0);
    cr.arc(x + w - r, y + h - r, r, 0, Math.PI / 2);
    cr.arc(x + r, y + h - r, r, Math.PI / 2, Math.PI);
    cr.arc(x + r, y + r, r, Math.PI, 1.5 * Math.PI);
    cr.closePath();
}

/**
 * The glow around a `w`×`h` window at (REACH, REACH): a fill below the
 * window, then outlines fading out.
 */
function paintGlow(cr, w, h, [r, g, b], alpha) {
    cr.setSourceRGBA(r / 255, g / 255, b / 255, alpha);
    rounded(cr, REACH, REACH, w, h, CORNER);
    cr.fill();
    cr.setLineWidth(1);
    for (let d = 1; d <= REACH; d++) {
        const fade = (1 - d / (REACH + 1)) ** 2;
        cr.setSourceRGBA(r / 255, g / 255, b / 255, alpha * fade);
        rounded(cr, REACH - d + 0.5, REACH - d + 0.5, w + 2 * d - 1, h + 2 * d - 1, CORNER + d);
        cr.stroke();
    }
}

function rgb(hex) {
    const n = parseInt(hex.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Whether white text reads better than black on `hex` (WCAG luminance). */
function dark(hex) {
    const [r, g, b] = rgb(hex).map(c => {
        const s = c / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.4;
}

export class Foreign {
    constructor(service, workspaces) {
        this._service = service;
        this._workspaces = workspaces;
        /** window id → {window, actor, glow, badge, color, alpha, ids, actorId} */
        this._marks = new Map();
        this._displayIds = [
            global.display.connect('notify::focus-window', () => this.update()),
            global.display.connect('restacked', () => this._marks.forEach(m => this._restack(m))),
        ];
    }

    destroy() {
        this._displayIds.forEach(id => global.display.disconnect(id));
        for (const id of [...this._marks.keys()])
            this._unmark(id);
    }

    /** Mark exactly the foreign windows; a failure is logged, not thrown. */
    update() {
        try {
            this._update();
        } catch (e) {
            logError(e, 'koje: marking foreign windows');
        }
    }

    _update() {
        const foreign = new Set();
        for (const w of this._service.windows?.all() ?? []) {
            const f = this._foreign(w);
            if (f) {
                foreign.add(w.get_id());
                this._mark(w, f);
            }
        }
        for (const id of [...this._marks.keys()]) {
            if (!foreign.has(id))
                this._unmark(id);
        }
    }

    /** The context and color of a foreign window, else null. */
    _foreign(w) {
        const id = w.get_id();
        const tag = this._service.tags.get(id);
        if (!tag?.startsWith('koje-'))
            return null;
        const context = tag.slice('koje-'.length);
        const ws = w.is_on_all_workspaces() ? null : w.get_workspace();
        if (ws && this._workspaces.name(ws) === context)
            return null;
        const colors = this._service.colors.get(id) ?? this._service.rules.get(tag);
        return {context, color: colors?.active ?? '#808080'};
    }

    _mark(w, {context, color}) {
        const id = w.get_id();
        let m = this._marks.get(id);
        if (!m) {
            const actor = w.get_compositor_private();
            if (!actor)
                return;
            const glow = new St.Widget({name: 'koje-glow', reactive: false});
            for (let i = 0; i < 4; i++) {
                const strip = new St.DrawingArea({reactive: false});
                strip.connect('repaint', a => {
                    const cr = a.get_context();
                    cr.translate(-a.x, -a.y);
                    paintGlow(cr, m.width, m.height, rgb(m.color), m.alpha);
                    cr.$dispose();
                });
                glow.add_child(strip);
            }
            const badge = new St.Label({name: 'koje-badge', reactive: false});
            for (const a of [glow, badge]) {
                for (const c of ['X', 'Y']) {
                    a.add_constraint(new Clutter.BindConstraint({
                        source: actor, coordinate: Clutter.BindCoordinate[c],
                    }));
                }
                actor.bind_property('visible', a, 'visible', GObject.BindingFlags.SYNC_CREATE);
            }
            m = {window: w, actor, glow, badge};
            this._restack(m);
            m.ids = [w.connect('size-changed', () => this._place(m))];
            // The window is gone: its marks too.
            m.actorId = actor.connect('destroy', () => this._unmark(id));
            this._marks.set(id, m);
        }
        const alpha = w.has_focus() ? GLOW_FOCUSED : GLOW_UNFOCUSED;
        if (m.color !== color || m.alpha !== alpha) {
            Object.assign(m, {color, alpha});
            m.glow.get_children().forEach(strip => strip.queue_repaint());
        }
        m.badge.text = context;
        m.badge.style = `background-color: ${color}; color: ${dark(color) ? '#ffffff' : 'rgba(0, 0, 0, 0.87)'}; ` +
            'border-radius: 999px; padding: 2px 8px; font-size: 10pt; font-weight: bold;';
        this._place(m);
    }

    /** Glow right below the window, badge right above it. */
    _restack({actor, glow, badge}) {
        const parent = actor.get_parent();
        for (const a of [glow, badge]) {
            if (a.get_parent() !== parent) {
                a.get_parent()?.remove_child(a);
                parent?.add_child(a);
            }
        }
        parent?.set_child_below_sibling(glow, actor);
        parent?.set_child_above_sibling(badge, actor);
    }

    // Offsets from the actor, which also holds client-side shadows and may
    // start below the frame (title bars as subsurfaces); the badge sits on
    // the top edge, inside the window where that edge is under the top bar.
    _place(m) {
        const {window: w, glow, badge} = m;
        const frame = w.get_frame_rect();
        const buffer = w.get_buffer_rect();
        const x = frame.x - buffer.x;
        const y = frame.y - buffer.y;
        const [glowX, glowY] = glow.get_constraints();
        glowX.offset = x - REACH;
        glowY.offset = y - REACH;
        if (m.width !== frame.width || m.height !== frame.height) {
            Object.assign(m, {width: frame.width, height: frame.height});
            const W = frame.width + 2 * REACH;
            const H = frame.height + 2 * REACH;
            const T = REACH + INNER;
            const side = Math.max(H - 2 * T, 1);
            glow.set_size(W, H);
            // top, bottom, left, right
            [[0, 0, W, T], [0, H - T, W, T], [0, T, T, side], [W - T, T, T, side]]
                .forEach(([sx, sy, sw, sh], i) => {
                    const strip = glow.get_child_at_index(i);
                    strip.set_position(sx, sy);
                    strip.set_size(sw, sh);
                    strip.queue_repaint();
                });
        }
        const [, height] = badge.get_preferred_height(-1);
        const area = w.get_work_area_current_monitor();
        const [badgeX, badgeY] = badge.get_constraints();
        badgeX.offset = x + BADGE_LEFT;
        badgeY.offset = frame.y - height / 2 < area.y ? y + 4 : y - Math.round(height / 2);
    }

    _unmark(id) {
        const m = this._marks.get(id);
        if (!m)
            return;
        this._marks.delete(id);
        m.ids.forEach(i => m.window.disconnect(i));
        m.actor.disconnect(m.actorId);
        m.glow.destroy();
        m.badge.destroy();
    }
}
