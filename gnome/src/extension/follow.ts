// Actors that go with a window's actor as its siblings: bound to its
// position and visibility, right below or above it in the stack (as its
// children they were repainted only within the window's area).

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import type Meta from 'gi://Meta';
import type Mtk from 'gi://Mtk';

/** Let `widget` follow the window actor `actor` (offsets: `offsets`). */
export function follow(widget: Clutter.Actor, actor: Clutter.Actor): void {
    for (const c of ['X', 'Y'] as const) {
        widget.add_constraint(new Clutter.BindConstraint({
            source: actor, coordinate: Clutter.BindCoordinate[c],
        }));
    }
    actor.bind_property('visible', widget, 'visible', GObject.BindingFlags.SYNC_CREATE);
}

/** Move a following widget by (x, y) from its window actor. */
export function offsets(widget: Clutter.Actor, x: number, y: number): void {
    const [cx, cy] = widget.get_constraints() as Clutter.BindConstraint[];
    cx.offset = x;
    cy.offset = y;
}

/** `below` right below the window actor, `above` right above it. */
export function restack(actor: Clutter.Actor, below: Clutter.Actor[] = [], above: Clutter.Actor[] = []): void {
    const parent = actor.get_parent();
    if (!parent)
        return;
    for (const a of [...below, ...above]) {
        if (a.get_parent() !== parent) {
            a.get_parent()?.remove_child(a);
            parent.add_child(a);
        }
    }
    below.forEach(a => parent.set_child_below_sibling(a, actor));
    // The last one ends up topmost.
    let under = actor;
    for (const a of above) {
        parent.set_child_above_sibling(a, under);
        under = a;
    }
}

/** A window's frame within its actor (which also holds client-side shadows). */
export function frameIn(w: Meta.Window): {x: number; y: number; frame: Mtk.Rectangle} {
    const frame = w.get_frame_rect();
    const buffer = w.get_buffer_rect();
    return {x: frame.x - buffer.x, y: frame.y - buffer.y, frame};
}
