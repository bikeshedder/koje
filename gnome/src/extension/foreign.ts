// Windows outside their context (ADR 0057): a context's window on another
// context's workspace or a host workspace glows in the context's color and
// carries a badge with the context's name on its top edge. Host windows
// never do. A window's context is kojed's `koje-<context>` tag
// (`tag-window`); its color the active color of the window's own
// `border-colors` (kojed's focus blip) or of the tag's `context-rule`.
//
// Glow and badge follow the window's actor (follow.ts). The glow is drawn with Cairo
// inside its own area, in four strips along the window's edges: St's
// `box-shadow` left trails while the window was dragged, and one area as
// large as the window made dragging slow (both seen in the GNOME lab).

import type Clutter from 'gi://Clutter';
import type Meta from 'gi://Meta';
import St from 'gi://St';
import type cairo from 'cairo';

import {dark, rgb, type Rgb} from './colors.js';
import {follow, frameIn, offsets, restack} from './follow.js';
import type {Service} from './service.js';
import type {Workspaces} from './workspaces.js';

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
function rounded(cr: cairo.Context, x: number, y: number, w: number, h: number, r: number): void {
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
function paintGlow(cr: cairo.Context, w: number, h: number, [r, g, b]: Rgb, alpha: number): void {
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

interface Mark {
    window: Meta.Window;
    actor: Clutter.Actor;
    glow: St.Widget;
    badge: St.Label;
    color: string;
    alpha: number;
    width: number;
    height: number;
    ids: number[];
    actorId: number;
}

export class Foreign {
    #service: Service;
    #workspaces: Workspaces;
    /** window id → its marks */
    #marks = new Map<number, Mark>();
    #displayIds: number[];

    constructor(service: Service, workspaces: Workspaces) {
        this.#service = service;
        this.#workspaces = workspaces;
        this.#displayIds = [
            global.display.connect('notify::focus-window', () => this.update()),
            global.display.connect('restacked', () => this.#marks.forEach(
                m => restack(m.actor, [m.glow], [m.badge]))),
        ];
    }

    destroy(): void {
        this.#displayIds.forEach(id => global.display.disconnect(id));
        for (const id of [...this.#marks.keys()])
            this.#unmark(id);
    }

    /** Mark exactly the foreign windows; a failure is logged, not thrown. */
    update(): void {
        try {
            this.#update();
        } catch (e) {
            logError(e as Error, 'koje: marking foreign windows');
        }
    }

    #update(): void {
        const foreign = new Set<number>();
        for (const w of this.#service.windows?.all() ?? []) {
            const f = this.#foreign(w);
            if (f) {
                foreign.add(w.get_id());
                this.#mark(w, f);
            }
        }
        for (const id of [...this.#marks.keys()]) {
            if (!foreign.has(id))
                this.#unmark(id);
        }
    }

    /** The context and color of a foreign window, else null. */
    #foreign(w: Meta.Window): {context: string; color: string} | null {
        const id = w.get_id();
        const tag = this.#service.tags.get(id);
        if (!tag?.startsWith('koje-'))
            return null;
        const context = tag.slice('koje-'.length);
        const ws = w.is_on_all_workspaces() ? null : w.get_workspace();
        if (ws && this.#workspaces.name(ws) === context)
            return null;
        const colors = this.#service.colors.get(id) ?? this.#service.rules.get(tag);
        return {context, color: colors?.active ?? '#808080'};
    }

    #mark(w: Meta.Window, {context, color}: {context: string; color: string}): void {
        const id = w.get_id();
        let m = this.#marks.get(id);
        if (!m) {
            const actor = w.get_compositor_private<Meta.WindowActor>();
            if (!actor)
                return;
            const glow = new St.Widget({name: 'koje-glow', reactive: false});
            const badge = new St.Label({name: 'koje-badge', reactive: false});
            const mark: Mark = {
                window: w, actor, glow, badge,
                color: '', alpha: 0, width: 0, height: 0, ids: [], actorId: 0,
            };
            for (let i = 0; i < 4; i++) {
                const strip = new St.DrawingArea({reactive: false});
                strip.connect('repaint', a => {
                    const cr = a.get_context();
                    cr.translate(-a.x, -a.y);
                    paintGlow(cr, mark.width, mark.height, rgb(mark.color), mark.alpha);
                    cr.$dispose();
                });
                glow.add_child(strip);
            }
            follow(glow, actor);
            follow(badge, actor);
            restack(actor, [glow], [badge]);
            mark.ids = [w.connect('size-changed', () => this.#place(mark))];
            // The window is gone: its marks too.
            mark.actorId = actor.connect('destroy', () => this.#unmark(id));
            this.#marks.set(id, mark);
            m = mark;
        }
        const alpha = w.has_focus() ? GLOW_FOCUSED : GLOW_UNFOCUSED;
        if (m.color !== color || m.alpha !== alpha) {
            Object.assign(m, {color, alpha});
            (m.glow.get_children() as St.DrawingArea[]).forEach(strip => strip.queue_repaint());
        }
        m.badge.text = context;
        m.badge.style = `background-color: ${color}; color: ${dark(color) ? '#ffffff' : 'rgba(0, 0, 0, 0.87)'}; ` +
            'border-radius: 999px; padding: 2px 8px; font-size: 10pt; font-weight: bold;';
        this.#place(m);
    }

    // Offsets from the actor, which also holds client-side shadows and may
    // start below the frame (title bars as subsurfaces); the badge sits on
    // the top edge, inside the window where that edge is under the top bar.
    #place(m: Mark): void {
        const {window: w, glow, badge} = m;
        const {x, y, frame} = frameIn(w);
        offsets(glow, x - REACH, y - REACH);
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
                    const strip = glow.get_child_at_index(i) as St.DrawingArea;
                    strip.set_position(sx, sy);
                    strip.set_size(sw, sh);
                    strip.queue_repaint();
                });
        }
        const [, height] = badge.get_preferred_height(-1);
        const area = w.get_work_area_current_monitor();
        offsets(badge, x + BADGE_LEFT,
            frame.y - height / 2 < area.y ? y + 4 : y - Math.round(height / 2));
    }

    #unmark(id: number): void {
        const m = this.#marks.get(id);
        if (!m)
            return;
        this.#marks.delete(id);
        m.ids.forEach(i => m.window.disconnect(i));
        m.actor.disconnect(m.actorId);
        m.glow.destroy();
        m.badge.destroy();
    }
}
