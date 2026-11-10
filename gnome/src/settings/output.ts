// The output of an operation (kojed's OperationProgress lines), live, in a
// window of its own: what a start, recreate or image build does, and why
// something failed.

import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk?version=4.0';
import Gtk from 'gi://Gtk?version=4.0';

/** How close to the end (pixels) still counts as following the output. */
const FOLLOW_SLACK = 24;

export class OutputWindow {
    readonly #window: Adw.Window;
    readonly #title: Adw.WindowTitle;
    readonly #buffer = new Gtk.TextBuffer();
    readonly #adjustment: Gtk.Adjustment;
    /** Whether new output scrolls into view: until scrolled up to read. */
    #follow = true;
    /** What it shows (a context, or `image:<name>`); `null` while closed. */
    context: string | null = null;

    constructor(app: Gtk.Application) {
        this.#title = new Adw.WindowTitle();
        const header = new Adw.HeaderBar({title_widget: this.#title});
        const copy = new Gtk.Button({icon_name: 'edit-copy-symbolic', tooltip_text: 'Copy All'});
        copy.connect('clicked', () => {
            const [start, end] = this.#buffer.get_bounds();
            Gdk.Display.get_default()?.get_clipboard().set(this.#buffer.get_text(start, end, false));
        });
        header.pack_start(copy);
        const view = new Gtk.TextView({
            buffer: this.#buffer,
            editable: false,
            cursor_visible: false,
            monospace: true,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 12, bottom_margin: 12, left_margin: 12, right_margin: 12,
        });
        const scroll = new Gtk.ScrolledWindow({child: view, vexpand: true});
        this.#adjustment = scroll.vadjustment;
        // The range grows once new lines are laid out: follow it then, not
        // when they are inserted (the end is not where it will be yet).
        this.#adjustment.connect('notify::upper', () => this.#keepUp());
        this.#adjustment.connect('notify::page-size', () => this.#keepUp());
        this.#adjustment.connect('value-changed', adj => {
            this.#follow = adj.value + adj.page_size >= adj.upper - FOLLOW_SLACK;
        });
        const toolbar = new Adw.ToolbarView({content: scroll});
        toolbar.add_top_bar(header);
        this.#window = new Adw.Window({
            application: app,
            content: toolbar,
            default_width: 900,
            default_height: 600,
            hide_on_close: true,
        });
        this.#window.connect('close-request', () => {
            this.context = null;
            return false;
        });
    }

    /** Show `lines` of `context`'s operation under `title`, following the end. */
    present(context: string, title: string, lines: string[]): void {
        this.context = context;
        this.title = title;
        this.#buffer.set_text(lines.join('\n'), -1);
        this.#follow = true;
        this.#window.present();
        this.#keepUp();
    }

    /** Gone with the settings window: a hidden window would keep the app running. */
    destroy(): void {
        this.#window.destroy();
    }

    set title(title: string) {
        this.#title.title = title;
        this.#window.title = title;
    }

    /** One more line. */
    append(line: string): void {
        const end = this.#buffer.get_end_iter();
        this.#buffer.insert(end, this.#buffer.get_char_count() > 0 ? `\n${line}` : line, -1);
    }

    #keepUp(): void {
        if (this.#follow)
            this.#adjustment.value = this.#adjustment.upper - this.#adjustment.page_size;
    }
}
