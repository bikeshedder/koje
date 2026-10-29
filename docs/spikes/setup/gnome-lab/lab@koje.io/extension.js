// Lab only (docs/spikes/setup/gnome-lab): lifts GNOME Shell's D-Bus
// restrictions so `lab.sh eval` and `lab.sh screenshot` work.
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class LabExtension extends Extension {
    enable() {
        global.context.unsafe_mode = true;
    }

    disable() {
        global.context.unsafe_mode = false;
    }
}
