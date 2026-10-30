// The settings app's questions: a name, or yes/no.

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';

import {validName} from './model.js';

/** Ask for a context or state name; `null` when cancelled. */
export function askName(parent: Gtk.Widget, heading: string, verb: string, initial = ''): Promise<string | null> {
    const entry = new Gtk.Entry({text: initial, activates_default: true});
    const dialog = new Adw.AlertDialog({
        heading,
        body: 'Lowercase letters, digits and -, starting with a letter.',
        extra_child: entry,
        default_response: 'ok',
        close_response: 'cancel',
    });
    dialog.add_response('cancel', 'Cancel');
    dialog.add_response('ok', verb);
    dialog.set_response_appearance('ok', Adw.ResponseAppearance.SUGGESTED);
    const check = () => {
        const ok = validName(entry.text);
        dialog.set_response_enabled('ok', ok);
        if (ok || entry.text === '')
            entry.remove_css_class('error');
        else
            entry.add_css_class('error');
    };
    entry.connect('changed', check);
    check();
    return new Promise(resolve => {
        dialog.connect('response', (_d, response) => resolve(response === 'ok' ? entry.text : null));
        dialog.present(parent);
        entry.grab_focus();
    });
}

/** Ask before something that cannot be undone; resolves to the response id. */
export function ask(parent: Gtk.Widget, heading: string, body: string,
    responses: [string, string, Adw.ResponseAppearance][]): Promise<string> {
    const dialog = new Adw.AlertDialog({heading, body, close_response: 'cancel'});
    dialog.add_response('cancel', 'Cancel');
    for (const [id, label, appearance] of responses) {
        dialog.add_response(id, label);
        dialog.set_response_appearance(id, appearance);
    }
    return new Promise(resolve => {
        dialog.connect('response', (_d, response) => resolve(response));
        dialog.present(parent);
    });
}
