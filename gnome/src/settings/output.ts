// The output of an operation (kojed's OperationProgress lines), live: what a
// start or recreate builds, and why something failed.

import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk?version=4.0';
import Gtk from 'gi://Gtk?version=4.0';

export class OutputDialog {
    readonly #dialog: Adw.Dialog;
    readonly #title: Adw.WindowTitle;
    readonly #buffer = new Gtk.TextBuffer();
    readonly #view: Gtk.TextView;
    readonly #scroll: Gtk.ScrolledWindow;
    /** The context it shows; `null` while closed. */
    context: string | null = null;

    constructor() {
        this.#title = new Adw.WindowTitle();
        const header = new Adw.HeaderBar({title_widget: this.#title});
        const copy = new Gtk.Button({icon_name: 'edit-copy-symbolic', tooltip_text: 'Copy All'});
        copy.connect('clicked', () => {
            const [start, end] = this.#buffer.get_bounds();
            Gdk.Display.get_default()?.get_clipboard().set(this.#buffer.get_text(start, end, false));
        });
        header.pack_start(copy);
        this.#view = new Gtk.TextView({
            buffer: this.#buffer,
            editable: false,
            cursor_visible: false,
            monospace: true,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 12, bottom_margin: 12, left_margin: 12, right_margin: 12,
        });
        this.#scroll = new Gtk.ScrolledWindow({child: this.#view, vexpand: true});
        const toolbar = new Adw.ToolbarView({content: this.#scroll});
        toolbar.add_top_bar(header);
        this.#dialog = new Adw.Dialog({child: toolbar, content_width: 860, content_height: 560});
        this.#dialog.connect('closed', () => {
            this.context = null;
        });
    }

    /** Show `lines` of `context`'s operation under `title`. */
    present(parent: Gtk.Widget, context: string, title: string, lines: string[]): void {
        this.context = context;
        this.#title.title = title;
        this.#buffer.set_text(lines.join('\n'), -1);
        this.#dialog.present(parent);
        this.#scrollToEnd();
    }

    set title(title: string) {
        this.#title.title = title;
    }

    /** One more line; followed to the end unless scrolled up to read. */
    append(line: string): void {
        const adj = this.#scroll.vadjustment;
        const atEnd = adj.value + adj.page_size >= adj.upper - 24;
        const end = this.#buffer.get_end_iter();
        this.#buffer.insert(end, this.#buffer.get_char_count() > 0 ? `\n${line}` : line, -1);
        if (atEnd)
            this.#scrollToEnd();
    }

    #scrollToEnd(): void {
        const mark = this.#buffer.create_mark(null, this.#buffer.get_end_iter(), false);
        this.#view.scroll_mark_onscreen(mark);
        this.#buffer.delete_mark(mark);
    }
}
