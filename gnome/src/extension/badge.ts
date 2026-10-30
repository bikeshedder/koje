// A context's programs carry a badge on their icon (user, 2026-10-30): a
// pill in the context's color with its name, wherever GNOME draws the icon
// (search, app grid, dash), so they are told from the host's apps of the
// same name. Host apps stay plain.

import Clutter from 'gi://Clutter';
import Pango from 'gi://Pango';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {dark} from './colors.js';

const createIconTexture = Shell.App.prototype.create_icon_texture;

/** Let `app`'s icons carry `context`'s badge; `color` is asked each time. */
export function badge(app: Shell.App, context: string, color: () => string): void {
    Object.assign(app, {
        create_icon_texture: (size: number) => {
            const icon = createIconTexture.call(app, size);
            const scale = St.ThemeContext.get_for_stage(global.stage as Clutter.Stage).scale_factor;
            const box = new St.Widget({
                layout_manager: new Clutter.BinLayout(),
                width: size * scale,
                height: size * scale,
            });
            box.add_child(icon);
            const c = color();
            const label = new St.Label({
                text: context,
                x_align: Clutter.ActorAlign.END,
                y_align: Clutter.ActorAlign.END,
                style: `background-color: ${c}; color: ${dark(c) ? '#ffffff' : 'rgba(0, 0, 0, 0.87)'}; ` +
                    `border-radius: 999px; padding: 0 ${Math.max(2, Math.round(size / 24))}px; ` +
                    `font-size: ${Math.max(7, Math.round(size / 6))}px; font-weight: bold;`,
            });
            label.clutter_text.ellipsize = Pango.EllipsizeMode.END;
            box.add_child(label);
            return box;
        },
    });
}
