// koje Settings (`koje-settings [<context>]`, io.koje.Settings, ADR 0060):
// one instance; a second start shows the window, on the given context.

import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import {programArgs, programInvocationName} from 'system';

import {SettingsWindow} from './window.js';

const app = new Adw.Application({
    application_id: 'io.koje.Settings',
    flags: Gio.ApplicationFlags.HANDLES_COMMAND_LINE,
});
let window: SettingsWindow | null = null;
app.connect('command-line', (_app, line) => {
    window ??= new SettingsWindow(app);
    const wanted = line.get_arguments()[1];
    window.present(wanted);
    return 0;
});
app.runAsync([programInvocationName, ...programArgs]);
