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

/** What a new image is: its name, label and the image it builds on. */
export interface NewImage {
    name: string;
    label: string;
    from: string;
}

/**
 * Ask for a new image (ADR 0064): name, label, and what it builds on from
 * `choices` (`[value, title]`); `null` when cancelled.
 */
export function askImage(parent: Gtk.Widget, initial: NewImage, choices: [string, string][]): Promise<NewImage | null> {
    const name = new Gtk.Entry({text: initial.name, activates_default: true, placeholder_text: 'Name'});
    const label = new Gtk.Entry({text: initial.label, activates_default: true, placeholder_text: 'Label (optional)'});
    const from = Gtk.DropDown.new_from_strings(choices.map(([, title]) => title));
    from.selected = Math.max(0, choices.findIndex(([v]) => v === initial.from));
    const box = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 12});
    box.append(name);
    box.append(label);
    const fromBox = new Gtk.Box({spacing: 12});
    fromBox.append(new Gtk.Label({label: 'Builds on'}));
    from.hexpand = true;
    fromBox.append(from);
    box.append(fromBox);
    const dialog = new Adw.AlertDialog({
        heading: 'New Image',
        body: 'A folder with a Containerfile and image.toml in ~/.config/koje/images. The name is lowercase letters, digits and -, starting with a letter.',
        extra_child: box,
        default_response: 'ok',
        close_response: 'cancel',
    });
    dialog.add_response('cancel', 'Cancel');
    dialog.add_response('ok', 'Create');
    dialog.set_response_appearance('ok', Adw.ResponseAppearance.SUGGESTED);
    const check = () => {
        const ok = validName(name.text) && name.text !== 'fedora';
        dialog.set_response_enabled('ok', ok);
        if (ok || name.text === '')
            name.remove_css_class('error');
        else
            name.add_css_class('error');
    };
    name.connect('changed', check);
    check();
    return new Promise(resolve => {
        dialog.connect('response', (_d, response) => resolve(response === 'ok'
            ? {name: name.text, label: label.text, from: choices[from.selected]?.[0] ?? initial.from}
            : null));
        dialog.present(parent);
        name.grab_focus();
    });
}

/**
 * Ask for a new context's name and image (ADR 0067: there is no default);
 * `choices` are `[value, title]`; `null` when cancelled.
 */
export function askContext(parent: Gtk.Widget, choices: [string, string][]): Promise<{name: string; image: string} | null> {
    const name = new Gtk.Entry({activates_default: true, placeholder_text: 'Name'});
    const image = Gtk.DropDown.new_from_strings(choices.map(([, title]) => title));
    image.hexpand = true;
    const box = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 12});
    box.append(name);
    const imageBox = new Gtk.Box({spacing: 12});
    imageBox.append(new Gtk.Label({label: 'Image'}));
    imageBox.append(image);
    box.append(imageBox);
    const dialog = new Adw.AlertDialog({
        heading: 'New Context',
        body: 'Lowercase letters, digits and -, starting with a letter. The image is where its programs come from; it can be changed later.',
        extra_child: box,
        default_response: 'ok',
        close_response: 'cancel',
    });
    dialog.add_response('cancel', 'Cancel');
    dialog.add_response('ok', 'Create');
    dialog.set_response_appearance('ok', Adw.ResponseAppearance.SUGGESTED);
    const check = () => {
        const ok = validName(name.text) && choices.length > 0;
        dialog.set_response_enabled('ok', ok);
        if (ok || name.text === '')
            name.remove_css_class('error');
        else
            name.add_css_class('error');
    };
    name.connect('changed', check);
    check();
    return new Promise(resolve => {
        dialog.connect('response', (_d, response) => {
            const value = choices[image.selected]?.[0];
            resolve(response === 'ok' && value ? {name: name.text, image: value} : null);
        });
        dialog.present(parent);
        name.grab_focus();
    });
}
