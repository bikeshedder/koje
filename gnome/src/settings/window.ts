// koje Settings' window (ADR 0060): renders what kojed reports through the
// pure functions of model.ts and turns edits into kojed calls. Rows are
// updated in place; a form being edited is never overwritten.

import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk?version=4.0';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';

import {Kojed, KojedError} from '../api/kojed.js';
import {ask, askImage, askName} from './dialogs.js';
import * as model from './model.js';
import {OutputWindow} from './output.js';

Gio._promisify(Gtk.FileDialog.prototype, 'select_folder', 'select_folder_finish');
Gio._promisify(Gtk.FileLauncher.prototype, 'launch', 'launch_finish');

const MEMORY = '';
const IMAGE = model.IMAGE_KEY;

/** An image's row in the sidebar. */
interface ImageRows {
    sidebar: Gtk.ListBoxRow;
    title: Gtk.Label;
    subtitle: Gtk.Label;
}

/** The rows a context has in the sidebar and on the Memory page. */
interface ContextRows {
    sidebar: Gtk.ListBoxRow;
    dot: Gtk.Widget;
    title: Gtk.Label;
    subtitle: Gtk.Label;
    memory: Adw.ActionRow;
    level: Gtk.LevelBar;
}

function uiFile(name: string): string {
    const uri = GLib.uri_resolve_relative(import.meta.url, `ui/${name}`, GLib.UriFlags.NONE);
    const [path] = GLib.filename_from_uri(uri);
    return path;
}

function variant(value: unknown): GLib.Variant {
    if (typeof value === 'string')
        return new GLib.Variant('s', value);
    if (typeof value === 'number')
        return new GLib.Variant('t', value);
    if (Array.isArray(value))
        return new GLib.Variant('as', value);
    return new GLib.Variant('a{sb}', value as Record<string, boolean>);
}

export class SettingsWindow {
    readonly #kojed = new Kojed();
    readonly #builder: Gtk.Builder;
    readonly #window: Adw.ApplicationWindow;
    readonly #toasts: Adw.ToastOverlay;
    readonly #split: Adw.NavigationSplitView;
    readonly #sidebar: Gtk.ListBox;
    readonly #contentPage: Adw.NavigationPage;
    readonly #switcher: Adw.ViewSwitcher;
    readonly #actions: Gtk.Box;
    readonly #pages: Gtk.Stack;
    readonly #memoryGroup: Adw.PreferencesGroup;
    readonly #css = new Gtk.CssProvider();

    #contexts: model.Context[] = [];
    readonly #rows = new Map<string, ContextRows>();
    readonly #memoryRow: Gtk.ListBoxRow;
    /** Context → operation under way (`up`, `down`, …). */
    readonly #busy = new Map<string, string>();
    /** Context → the output of its last operation. */
    readonly #logs = new Map<string, model.Log>();
    readonly #output: OutputWindow;
    #selected = MEMORY;
    #running = false;

    // The selected context's Settings page.
    #config: model.Config | null = null;
    #original: model.Form | null = null;
    #form: model.Form | null = null;
    /** Set while the form's widgets are filled from code. */
    #filling = false;
    readonly #switchRows = new Map<string, Adw.SwitchRow>();
    #mountRows: Gtk.Widget[] = [];
    #stateRows: Gtk.Widget[] = [];
    #windowRows: Gtk.Widget[] = [];
    #refreshSource = 0;

    // Images (ADR 0064).
    #images: model.Image[] = [];
    readonly #imageRows = new Map<string, ImageRows>();
    #imageUserRows: Gtk.Widget[] = [];
    /** The context page's image choices, `[value, title]`. */
    #imageChoices: [string, string][] = [];
    /** The image whose label and description the entries hold. */
    #imageFilled = '';
    /** The buttons of the "koje's Version" row. */
    #originButtons: Gtk.Widget | null = null;

    constructor(app: Adw.Application) {
        this.#builder = Gtk.Builder.new_from_file(uiFile('window.ui'));
        const get = <T>(id: string) => this.#builder.get_object(id) as T;
        this.#window = get<Adw.ApplicationWindow>('window');
        this.#window.set_application(app);
        this.#output = new OutputWindow(app);
        this.#window.connect('close-request', () => {
            this.#output.destroy();
            return false;
        });
        this.#toasts = get('toasts');
        this.#split = get('split');
        this.#sidebar = get('sidebar');
        this.#contentPage = get('content_page');
        this.#switcher = get('switcher');
        this.#actions = get('actions');
        this.#pages = get('pages');
        this.#memoryGroup = get('memory_group');

        const display = Gdk.Display.get_default();
        if (display)
            Gtk.StyleContext.add_provider_for_display(display, this.#css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);

        this.#memoryRow = this.#sidebarRow(MEMORY, 'Memory', 'power-profile-performance-symbolic').sidebar;
        this.#sidebar.append(this.#memoryRow);
        this.#sidebar.set_header_func((row, before) => this.#sidebarHeader(row, before));
        this.#sidebar.connect('row-activated', (_l, row) => {
            this.#select(row.get_name());
            this.#split.show_content = true;
        });
        get<Gtk.Button>('new_context').connect('clicked', () => void this.#newContext());
        this.#connectForm();
        get<Gtk.Button>('save_as').connect('clicked', () => void this.#saveAs());
        get<Gtk.Button>('recreate_button').connect('clicked', () => void this.#recreate());
        get<Gtk.Button>('remove_button').connect('clicked', () => void this.#remove());
        this.#connectImages();

        for (const signal of ['ContextsChanged', 'MemoryChanged', 'ImagesChanged'] as const)
            this.#kojed.connect(signal, () => this.#scheduleRefresh());
        // An image's build: its output under `image:<name>`, as a context's.
        this.#kojed.connect('ImageBuildStarted', image => this.#started(IMAGE + image, 'build'));
        this.#kojed.connect('ImageBuildProgress', (image, line) => this.#progress(IMAGE + image, 'build', line));
        this.#kojed.connect('ImageBuildFinished', image => {
            this.#busy.delete(IMAGE + image);
            this.#ended(IMAGE + image, null);
            this.#toast(`Built image ${image} – contexts get it when recreated`);
            this.#scheduleRefresh();
        });
        this.#kojed.connect('ImageBuildFailed', (image, message) => {
            const key = IMAGE + image;
            this.#busy.delete(key);
            this.#ended(key, message);
            this.#toast(`Image ${image}: build failed – ${message}`, () => this.#showOutput(key));
            this.#scheduleRefresh();
        });
        this.#kojed.connect('StatesChanged', context => {
            if (context === this.#selected)
                void this.#loadStates();
        });
        this.#kojed.connect('OperationStarted', (context, operation) => this.#started(context, operation));
        this.#kojed.connect('OperationProgress', (context, operation, line) => this.#progress(context, operation, line));
        this.#kojed.connect('OperationFinished', (context, operation) => {
            this.#busy.delete(context);
            this.#ended(context, null);
            if (operation === 'remove')
                this.#toast(`Removed ${context}`);
            else if (context === this.#selected)
                void this.#refreshContainer();
            this.#scheduleRefresh();
        });
        this.#kojed.connect('OperationFailed', (context, operation, error, message) => {
            this.#busy.delete(context);
            this.#ended(context, message);
            if (operation === 'down' && error === 'io.koje.Error.WindowsOpen')
                void this.#stopAnyway(context);
            else
                this.#toast(`${context}: ${operation} failed – ${message}`, () => this.#showOutput(context));
            this.#scheduleRefresh();
        });
        this.#kojed.watch(running => {
            this.#running = running;
            this.#scheduleRefresh();
        });
    }

    present(context?: string): void {
        if (context)
            this.#select(context);
        this.#window.present();
    }

    // ----- Contexts, sidebar, memory -----

    #scheduleRefresh(): void {
        // Signals come in bursts: read once.
        if (this.#refreshSource)
            return;
        this.#refreshSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            this.#refreshSource = 0;
            void this.#refresh();
            return GLib.SOURCE_REMOVE;
        });
    }

    async #refresh(): Promise<void> {
        if (!this.#running) {
            this.#render();
            return;
        }
        try {
            const [rows] = await this.#kojed.call('Contexts');
            this.#contexts = model.contexts(rows);
        } catch (e) {
            this.#error(e);
            return;
        }
        this.#syncRows();
        await this.#refreshImages();
        await this.#refreshMemory();
        this.#render();
        if (this.#current() && this.#config === null)
            void this.#loadContext();
        else if (this.#current()?.status === 'running')
            void this.#loadWindows();
    }

    #current(): model.Context | undefined {
        return this.#contexts.find(c => c.name === this.#selected);
    }

    #sidebarRow(name: string, title: string, icon?: string): ContextRows {
        const box = new Gtk.Box({spacing: 12, margin_top: 6, margin_bottom: 6});
        const dot = icon
            ? new Gtk.Image({icon_name: icon})
            : new Gtk.Box({valign: Gtk.Align.CENTER, css_classes: ['koje-dot']});
        box.append(dot);
        const texts = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL});
        const titleLabel = new Gtk.Label({label: title, xalign: 0, ellipsize: 3});
        const subtitle = new Gtk.Label({xalign: 0, css_classes: ['caption', 'dim-label'], visible: false});
        texts.append(titleLabel);
        texts.append(subtitle);
        box.append(texts);
        const sidebar = new Gtk.ListBoxRow({child: box, name});
        const level = new Gtk.LevelBar({width_request: 160, valign: Gtk.Align.CENTER});
        const memory = new Adw.ActionRow({title, activatable: true});
        memory.add_suffix(level);
        memory.add_suffix(new Gtk.Image({icon_name: 'go-next-symbolic'}));
        memory.connect('activated', () => this.#select(name));
        return {sidebar, dot, title: titleLabel, subtitle, memory, level};
    }

    /** Add and remove the rows of contexts that came or went; update the rest. */
    #syncRows(): void {
        const names = new Set(this.#contexts.map(c => c.name));
        for (const [name, rows] of this.#rows) {
            if (!names.has(name)) {
                this.#sidebar.remove(rows.sidebar);
                this.#memoryGroup.remove(rows.memory);
                this.#rows.delete(name);
            }
        }
        let css = '.koje-dot { min-width: 12px; min-height: 12px; border-radius: 999px; }\n';
        this.#contexts.forEach((c, i) => {
            let rows = this.#rows.get(c.name);
            if (!rows) {
                rows = this.#sidebarRow(c.name, c.label);
                rows.dot.add_css_class(`koje-dot-${c.name}`);
                this.#rows.set(c.name, rows);
                // After Memory, in name order.
                this.#sidebar.insert(rows.sidebar, i + 1);
                this.#memoryGroup.add(rows.memory);
            }
            rows.title.label = c.label;
            rows.memory.title = c.label;
            const status = model.statusLabel(c, this.#busy.get(c.name));
            rows.subtitle.label = status;
            rows.subtitle.visible = c.status !== 'stopped' || this.#busy.has(c.name);
            if (/^#[0-9A-Fa-f]{6}$/.test(c.color))
                css += `.koje-dot-${c.name} { background: ${c.color}; }\n`;
        });
        this.#css.load_from_string(css);
        // A context asked for before its row existed (`koje-settings <context>`).
        const selected = this.#rowOf(this.#selected);
        if (selected && this.#sidebar.get_selected_row() !== selected)
            this.#sidebar.select_row(selected);
    }

    /** The sidebar row of a context, an image (`image:<name>`) or Memory. */
    #rowOf(key: string): Gtk.ListBoxRow | undefined {
        if (key === MEMORY)
            return this.#memoryRow;
        if (key.startsWith(IMAGE))
            return this.#imageRows.get(key.slice(IMAGE.length))?.sidebar;
        return this.#rows.get(key)?.sidebar;
    }

    // ----- Images in the sidebar (ADR 0064) -----

    async #refreshImages(): Promise<void> {
        try {
            const [rows] = await this.#kojed.call('Images');
            this.#images = model.images(rows);
        } catch (e) {
            this.#error(e);
            return;
        }
        this.#syncImageRows();
        const img = this.#currentImage();
        if (img)
            this.#fillImage(img);
        if (this.#form && !this.#filling)
            this.#fillImageChoices(this.#form.image);
    }

    #currentImage(): model.Image | undefined {
        if (!this.#selected.startsWith(IMAGE))
            return undefined;
        const name = this.#selected.slice(IMAGE.length);
        return this.#images.find(i => i.name === name);
    }

    /** Image rows after the contexts, under an "Images" heading with +. */
    #syncImageRows(): void {
        const names = new Set(this.#images.map(i => i.name));
        for (const [name, rows] of this.#imageRows) {
            if (!names.has(name)) {
                this.#sidebar.remove(rows.sidebar);
                this.#imageRows.delete(name);
            }
        }
        this.#images.forEach((img, i) => {
            let rows = this.#imageRows.get(img.name);
            if (!rows) {
                const box = new Gtk.Box({spacing: 12, margin_top: 6, margin_bottom: 6});
                box.append(new Gtk.Image({icon_name: 'package-x-generic-symbolic'}));
                const texts = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL});
                const title = new Gtk.Label({xalign: 0, ellipsize: 3});
                const subtitle = new Gtk.Label({xalign: 0, ellipsize: 3, css_classes: ['caption', 'dim-label']});
                texts.append(title);
                texts.append(subtitle);
                box.append(texts);
                rows = {sidebar: new Gtk.ListBoxRow({child: box, name: IMAGE + img.name}), title, subtitle};
                this.#imageRows.set(img.name, rows);
                // After Memory and the contexts, in the listed order.
                this.#sidebar.insert(rows.sidebar, 1 + this.#rows.size + i);
            }
            rows.title.label = img.label;
            const busy = this.#busy.has(IMAGE + img.name);
            rows.subtitle.label = busy ? 'Building…' : model.usedBy(img);
        });
        this.#sidebar.invalidate_headers();
        const selected = this.#rowOf(this.#selected);
        if (selected && this.#sidebar.get_selected_row() !== selected)
            this.#sidebar.select_row(selected);
    }

    /** The heading above the first image row. */
    #sidebarHeader(row: Gtk.ListBoxRow, before: Gtk.ListBoxRow | null): void {
        const first = row.get_name().startsWith(IMAGE) && !before?.get_name().startsWith(IMAGE);
        if (!first) {
            if (row.get_header())
                row.set_header(null);
            return;
        }
        if (row.get_header())
            return;
        const header = new Gtk.Box({spacing: 6, margin_top: 18, margin_start: 12, margin_end: 6});
        header.append(new Gtk.Label({label: 'Images', xalign: 0, hexpand: true, css_classes: ['heading', 'dim-label']}));
        const add = new Gtk.Button({icon_name: 'list-add-symbolic', tooltip_text: 'New Image', css_classes: ['flat']});
        add.connect('clicked', () => void this.#newImage(model.IMAGE_DEFAULT_FROM));
        header.append(add);
        row.set_header(header);
    }

    async #refreshMemory(): Promise<void> {
        for (const c of this.#contexts) {
            const rows = this.#rows.get(c.name);
            if (!rows)
                continue;
            if (c.status === 'running' || c.status === 'paused') {
                try {
                    const [measured, used, , soft, hard] = await this.#kojed.call('Memory', c.name);
                    if (measured) {
                        rows.memory.subtitle = model.memoryLabel(used, soft, hard);
                        rows.level.value = model.memoryFraction(used, soft, hard);
                        rows.level.visible = true;
                        continue;
                    }
                } catch {
                    // shown as its status below
                }
            }
            rows.memory.subtitle = model.statusLabel(c, this.#busy.get(c.name));
            rows.level.visible = false;
        }
    }

    #select(name: string): void {
        if (name === this.#selected)
            return;
        this.#selected = name;
        this.#config = null;
        this.#original = null;
        this.#form = null;
        this.#imageFilled = '';
        const row = this.#rowOf(name);
        if (row)
            this.#sidebar.select_row(row);
        this.#render();
        const img = this.#currentImage();
        if (img)
            this.#fillImage(img);
        else if (name !== MEMORY && !name.startsWith(IMAGE))
            void this.#loadContext();
    }

    /** The page, title and header actions for the selection. */
    #render(): void {
        if (!this.#running) {
            this.#pages.visible_child_name = 'kojed';
            this.#switcher.visible = false;
            this.#clearActions();
            return;
        }
        const img = this.#currentImage();
        if (img) {
            this.#pages.visible_child_name = 'image';
            this.#contentPage.title = img.label;
            this.#switcher.visible = false;
            this.#renderOutputActions(this.#selected);
            return;
        }
        const c = this.#current();
        if (!c) {
            // Gone, or not there yet (images come after the contexts).
            if (this.#selected !== MEMORY && this.#contexts.length && !this.#selected.startsWith(IMAGE))
                this.#selected = MEMORY;
            this.#pages.visible_child_name = 'memory';
            this.#contentPage.title = 'Memory';
            this.#switcher.visible = false;
            this.#sidebar.select_row(this.#memoryRow);
            this.#clearActions();
            return;
        }
        this.#pages.visible_child_name = 'context';
        this.#contentPage.title = c.label;
        this.#switcher.visible = true;
        this.#renderActions(c);
    }

    #clearActions(): void {
        let child = this.#actions.get_first_child();
        while (child) {
            const next = child.get_next_sibling();
            this.#actions.remove(child);
            child = next;
        }
    }

    /** The spinner (output, live) or the last output's button; whether busy. */
    #renderOutputActions(key: string): boolean {
        this.#clearActions();
        if (this.#busy.has(key)) {
            // What it does, live.
            const busy = new Gtk.Button({child: new Adw.Spinner(), tooltip_text: 'Show Output'});
            busy.connect('clicked', () => this.#showOutput(key));
            this.#actions.append(busy);
            return true;
        }
        if (this.#logs.has(key)) {
            const last = new Gtk.Button({
                icon_name: 'utilities-terminal-symbolic', tooltip_text: 'Output of the Last Operation',
            });
            last.connect('clicked', () => this.#showOutput(key));
            this.#actions.append(last);
        }
        return false;
    }

    #renderActions(c: model.Context): void {
        if (this.#renderOutputActions(c.name))
            return;
        const buttons: Record<model.Action, [string, string, () => Promise<void>]> = {
            up: ['media-playback-start-symbolic', 'Start', () => this.#up(c.name, '')],
            down: ['system-shutdown-symbolic', 'Stop', () => this.#down(c.name)],
            sleep: ['media-playback-pause-symbolic', 'Sleep', () => this.#do('Sleep', c.name)],
            wake: ['media-playback-start-symbolic', 'Wake', () => this.#do('Wake', c.name)],
        };
        for (const action of model.actions(c)) {
            const [icon, tooltip, run] = buttons[action];
            const button = new Gtk.Button({icon_name: icon, tooltip_text: tooltip});
            button.connect('clicked', () => void run());
            this.#actions.append(button);
        }
    }

    // ----- Calls -----

    /** A toast; with `output`, a button that shows an operation's output. */
    #toast(text: string, output?: () => void): void {
        const toast = new Adw.Toast({title: GLib.markup_escape_text(text, -1), timeout: 8});
        if (output) {
            toast.button_label = 'Show Output';
            toast.connect('button-clicked', output);
        }
        this.#toasts.add_toast(toast);
    }

    /** An operation of `key` (a context, or `image:<name>`) began. */
    #started(key: string, operation: string): void {
        this.#busy.set(key, operation);
        const log = model.logStarted(operation);
        this.#logs.set(key, log);
        if (this.#output.context === key)
            this.#output.present(key, model.logTitle(this.#titleOf(key), log), []);
        this.#render();
        this.#syncImageRows();
    }

    /** A line of what it does. */
    #progress(key: string, operation: string, line: string): void {
        let log = this.#logs.get(key);
        if (!log || log.state !== 'running') {
            // Started before this window was there.
            log = model.logStarted(operation);
            this.#logs.set(key, log);
        }
        const wasBuilding = log.building;
        model.logLine(log, line);
        if (this.#output.context === key)
            this.#output.append(line);
        else if (!wasBuilding && log.building && key === this.#selected)
            this.#showOutput(key); // a build takes minutes: show what it does
    }

    /** What the output dialog calls `key`: an image's label, a context's name. */
    #titleOf(key: string): string {
        if (!key.startsWith(IMAGE))
            return key;
        const name = key.slice(IMAGE.length);
        return `image ${this.#images.find(i => i.name === name)?.label ?? name}`;
    }

    /** An operation of `context` ended (`failed`: why, else `null`). */
    #ended(context: string, failed: string | null): void {
        const log = this.#logs.get(context);
        if (!log)
            return;
        const ended = model.logEnded(log, failed);
        this.#logs.set(context, ended);
        if (this.#output.context === context) {
            this.#output.title = model.logTitle(this.#titleOf(context), ended);
            if (failed !== null)
                this.#output.append(`failed: ${failed}`);
        }
    }

    /** The output of `context`'s current or last operation. */
    #showOutput(context: string): void {
        const log = this.#logs.get(context);
        if (log)
            this.#output.present(context, model.logTitle(this.#titleOf(context), log), log.lines);
    }

    #error(e: unknown): void {
        this.#toast(e instanceof Error ? e.message : String(e));
    }

    async #do(method: 'Sleep' | 'Wake' | 'Recreate', context: string): Promise<void> {
        try {
            await this.#kojed.call(method, context);
        } catch (e) {
            this.#error(e);
        }
    }

    async #up(context: string, state: string, recovery = ''): Promise<void> {
        try {
            await this.#kojed.call('Up', context, state, recovery, false, true);
        } catch (e) {
            if (e instanceof KojedError && e.short === 'RecoveryPending') {
                const answer = await ask(this.#window, `${context} Did Not Shut Down Cleanly`,
                    'Restore its windows from the recovery file, or start from the state?', [
                        ['discard', 'Discard', Adw.ResponseAppearance.DESTRUCTIVE],
                        ['restore', 'Restore', Adw.ResponseAppearance.SUGGESTED],
                    ]);
                if (answer !== 'cancel')
                    await this.#up(context, state, answer);
                return;
            }
            this.#error(e);
        }
    }

    async #down(context: string, save = true, force = false): Promise<void> {
        try {
            await this.#kojed.call('Down', context, '', save, force);
        } catch (e) {
            this.#error(e);
        }
    }

    // Down saved the windows and closed what it could; the rest may be
    // asking to save.
    async #stopAnyway(context: string): Promise<void> {
        const answer = await ask(this.#window, `${context} Still Has Open Windows`,
            'A program may be asking to save. Its windows were saved into the state; stopping now loses what is unsaved in them.',
            [['stop', 'Stop Anyway', Adw.ResponseAppearance.DESTRUCTIVE]]);
        if (answer === 'stop')
            await this.#down(context, false, true);
    }

    async #newContext(): Promise<void> {
        const name = await askName(this.#window, 'New Context', 'Create');
        if (!name)
            return;
        try {
            const [created] = await this.#kojed.call('NewContext', name);
            await this.#refresh();
            this.#select(created);
        } catch (e) {
            this.#error(e);
        }
    }

    async #recreate(): Promise<void> {
        const answer = await ask(this.#window, `Recreate ${this.#selected}?`,
            'Everything installed in its container outside your home is lost. The home, states and settings stay; a running context comes back with its windows.',
            [['recreate', 'Recreate', Adw.ResponseAppearance.DESTRUCTIVE]]);
        if (answer === 'recreate')
            await this.#do('Recreate', this.#selected);
    }

    async #remove(): Promise<void> {
        const context = this.#selected;
        const answer = await ask(this.#window, `Remove ${context}?`,
            'Its container, everything in its home, its states and settings are deleted for good. A running context is stopped without saving. Folders from your home are not touched.',
            [['remove', 'Remove', Adw.ResponseAppearance.DESTRUCTIVE]]);
        if (answer === 'remove') {
            try {
                await this.#kojed.call('Remove', context);
            } catch (e) {
                this.#error(e);
            }
        }
    }

    /** The Container group from the config: Recreate, and whether its image changed. */
    #fillContainer(c: model.Config): void {
        const recreate = model.recreateRow(c);
        this.#get<Adw.ActionRow>('recreate_row').subtitle = GLib.markup_escape_text(recreate.subtitle, -1);
        this.#get<Gtk.Button>('recreate_button').sensitive = recreate.sensitive;
    }

    // ----- Images (ADR 0064) -----

    #connectImages(): void {
        this.#get<Adw.ComboRow>('image_choice_row').connect('notify::selected', row => {
            const value = this.#imageChoices[row.selected]?.[0];
            if (value !== undefined)
                this.#edit({image: value});
        });
        this.#get<Gtk.Button>('new_image_button').connect('clicked', () => void this.#newImageForContext());
        for (const [id, key] of [['image_label_row', 'label'], ['image_description_row', 'description']] as const) {
            this.#get<Adw.EntryRow>(id).connect('apply', row => void this.#setImage(key, row.text));
        }
        this.#get<Gtk.Button>('image_edit_button').connect('clicked', () => void this.#editContainerfile());
        this.#get<Gtk.Button>('image_build_button').connect('clicked', () => void this.#buildImage());
        this.#get<Gtk.Button>('image_derive_button').connect('clicked', () => {
            const img = this.#currentImage();
            if (img)
                void this.#newImage(img.name);
        });
        this.#get<Gtk.Button>('image_remove_button').connect('clicked', () => void this.#removeImage());
    }

    /** The context page's image dropdown, on `current` (from code: no edit). */
    #fillImageChoices(current: string): void {
        const filling = this.#filling;
        this.#filling = true;
        this.#imageChoices = model.imageChoices(this.#images, current);
        const row = this.#get<Adw.ComboRow>('image_choice_row');
        row.model = Gtk.StringList.new(this.#imageChoices.map(([, title]) => title));
        row.selected = model.imageChoice(this.#imageChoices, current);
        const img = this.#images.find(i => i.name === current);
        row.subtitle = img?.description || (img ? '' : 'A registry image or a folder, as written in context.toml');
        this.#filling = filling;
    }

    /** The image page (the entries only when another image was shown: an edit stays). */
    #fillImage(img: model.Image): void {
        const own = img.kind !== 'builtin';
        const label = this.#get<Adw.EntryRow>('image_label_row');
        const description = this.#get<Adw.EntryRow>('image_description_row');
        if (this.#imageFilled !== img.name) {
            label.text = img.label;
            description.text = img.description;
            this.#imageFilled = img.name;
        }
        label.sensitive = own;
        description.sensitive = own;
        this.#get<Adw.ActionRow>('image_name_row').subtitle = `image = "${img.name}" in a context's context.toml`;
        const file = this.#get<Adw.ActionRow>('image_file_row');
        const home = GLib.get_home_dir();
        file.subtitle = GLib.markup_escape_text(own
            ? (img.file.startsWith(`${home}/`) ? `~${img.file.slice(home.length)}` : img.file) || img.error
            : 'Built into koje – customize it to change it', -1);
        this.#get<Gtk.Button>('image_edit_button').visible = own && img.file !== '';
        const building = this.#busy.has(IMAGE + img.name);
        this.#get<Adw.ActionRow>('image_build_row').subtitle = GLib.markup_escape_text(model.imageStatus(img, building), -1);
        this.#get<Gtk.Button>('image_build_button').sensitive = !building && (!own || img.file !== '');
        this.#fillOrigin(img);
        const removal = model.imageRemoval(img);
        this.#get<Adw.ActionRow>('image_remove_row').subtitle = GLib.markup_escape_text(removal.subtitle, -1);
        this.#get<Gtk.Button>('image_remove_button').sensitive = removal.sensitive;

        const group = this.#get<Adw.PreferencesGroup>('image_users_group');
        this.#imageUserRows.forEach(r => group.remove(r));
        this.#imageUserRows = img.usedBy.map(context => {
            const label = this.#contexts.find(c => c.name === context)?.label ?? context;
            const row = new Adw.ActionRow({title: GLib.markup_escape_text(label, -1), activatable: true});
            row.add_suffix(new Gtk.Image({icon_name: 'go-next-symbolic'}));
            row.connect('activated', () => this.#select(context));
            group.add(row);
            return row;
        });
        if (!img.usedBy.length) {
            const row = new Adw.ActionRow({title: 'No context uses it', subtitle: 'Choose it on a context\'s page.'});
            group.add(row);
            this.#imageUserRows = [row];
        }
    }

    /** koje's version of its own image (ADR 0065): customize, keep, reset. */
    #fillOrigin(img: model.Image): void {
        const row = this.#get<Adw.ActionRow>('image_origin_row');
        const origin = model.imageOrigin(img);
        row.visible = origin !== null;
        if (!origin)
            return;
        row.subtitle = GLib.markup_escape_text(origin.subtitle, -1);
        // The buttons follow the state: replaced each time.
        if (this.#originButtons)
            row.remove(this.#originButtons);
        const box = new Gtk.Box({spacing: 6, valign: Gtk.Align.CENTER});
        const labels: Record<model.OriginAction, string> = {
            customize: 'Customize…', reset: 'Reset to koje\'s Version…', keep: 'Keep Mine',
        };
        for (const action of origin.actions) {
            const button = new Gtk.Button({label: labels[action]});
            if (action === 'reset')
                button.add_css_class('destructive-action');
            button.connect('clicked', () => void this.#originAction(img, action));
            box.append(button);
        }
        row.add_suffix(box);
        this.#originButtons = box;
    }

    async #originAction(img: model.Image, action: model.OriginAction): Promise<void> {
        try {
            switch (action) {
            case 'customize': {
                const answer = await ask(this.#window, `Customize ${img.label}?`,
                    `koje's files of ${img.name} are copied into ~/.config/koje/images/${img.name}, which then takes the place of koje's image for every context that uses it. koje updates no longer change it; you can go back to koje's version at any time.`,
                    [['customize', 'Customize', Adw.ResponseAppearance.SUGGESTED]]);
                if (answer === 'customize') {
                    await this.#kojed.call('CustomizeImage', img.name);
                    this.#imageFilled = '';
                }
                break;
            }
            case 'reset': {
                const answer = await ask(this.#window, `Reset ${img.label} to koje's Version?`,
                    `~/.config/koje/images/${img.name} and your changes in it are deleted. Contexts get koje's image when recreated.`,
                    [['reset', 'Reset', Adw.ResponseAppearance.DESTRUCTIVE]]);
                if (answer === 'reset') {
                    await this.#kojed.call('ResetImage', img.name);
                    this.#imageFilled = '';
                }
                break;
            }
            case 'keep':
                await this.#kojed.call('KeepImageChanges', img.name);
                break;
            }
        } catch (e) {
            this.#error(e);
        }
    }

    async #setImage(key: 'label' | 'description', text: string): Promise<void> {
        const img = this.#currentImage();
        if (!img)
            return;
        try {
            await this.#kojed.call('SetImage', img.name, {[key]: new GLib.Variant('s', text)});
            this.#imageFilled = '';
        } catch (e) {
            this.#error(e);
        }
    }

    /** The Containerfile in the desktop's editor (ADR 0063). */
    async #editContainerfile(): Promise<void> {
        const img = this.#currentImage();
        if (!img?.file)
            return;
        try {
            const launcher = new Gtk.FileLauncher({file: Gio.File.new_for_path(img.file)});
            await launcher.launch(this.#window, null);
        } catch (e) {
            if (!(e instanceof GLib.Error && e.matches(Gtk.DialogError, Gtk.DialogError.DISMISSED)))
                this.#error(e);
        }
    }

    /** Build it now; the output is shown at once. */
    async #buildImage(): Promise<void> {
        const img = this.#currentImage();
        if (!img)
            return;
        const key = IMAGE + img.name;
        this.#output.present(key, model.logTitle(this.#titleOf(key), model.logStarted('build')), []);
        try {
            await this.#kojed.call('BuildImage', img.name);
        } catch (e) {
            this.#error(e);
        }
    }

    async #removeImage(): Promise<void> {
        const img = this.#currentImage();
        if (!img)
            return;
        const answer = await ask(this.#window, `Remove Image ${img.label}?`,
            'Its folder with the Containerfile and everything next to it is deleted.',
            [['remove', 'Remove', Adw.ResponseAppearance.DESTRUCTIVE]]);
        if (answer !== 'remove')
            return;
        try {
            await this.#kojed.call('RemoveImage', img.name);
            this.#select(MEMORY);
        } catch (e) {
            this.#error(e);
        }
    }

    /** A new image building on `from`; shown when made. */
    async #newImage(from: string): Promise<void> {
        const answer = await askImage(this.#window, {name: '', label: '', from}, model.imageChoices(this.#images, from));
        if (!answer)
            return;
        try {
            const [name] = await this.#kojed.call('NewImage', answer.name, answer.from, answer.label);
            await this.#refreshImages();
            this.#select(IMAGE + name);
        } catch (e) {
            this.#error(e);
        }
    }

    /**
     * A new image for the selected context (ADR 0064): from its image, and
     * the context's image at once (a form being edited keeps its edits).
     */
    async #newImageForContext(): Promise<void> {
        const context = this.#selected;
        const f = this.#form;
        if (!f || !this.#original)
            return;
        const label = this.#current()?.label ?? context;
        const answer = await askImage(this.#window,
            {name: model.suggestImageName(context, this.#images), label, from: f.image},
            model.imageChoices(this.#images, f.image));
        if (!answer)
            return;
        try {
            const [name] = await this.#kojed.call('NewImage', answer.name, answer.from, answer.label);
            await this.#kojed.call('SetConfig', context, {image: new GLib.Variant('s', name)});
            if (context !== this.#selected || !this.#form || !this.#original)
                return;
            this.#original = {...this.#original, image: name};
            this.#form = {...this.#form, image: name};
            await this.#refreshImages();
            this.#fillImageChoices(name);
            this.#formChanged();
            this.#toast(`${context} now uses image ${name} – edit it on its page; Recreate applies it`);
        } catch (e) {
            this.#error(e);
        }
    }

    /** Only the Container group's part of the config (image, Recreate): a form being edited stays. */
    async #refreshContainer(): Promise<void> {
        const context = this.#selected;
        if (context === MEMORY || !this.#config)
            return;
        try {
            const [dict] = await this.#kojed.call('Config', context);
            if (context !== this.#selected || !this.#config)
                return;
            const fresh = dict as unknown as model.Config;
            this.#config = {
                ...this.#config,
                container: fresh.container,
                image_file: fresh.image_file,
                image_outdated: fresh.image_outdated,
            };
            this.#fillContainer(this.#config);
        } catch (e) {
            this.#error(e);
        }
    }

    // ----- The selected context -----

    async #loadContext(): Promise<void> {
        const name = this.#selected;
        try {
            const [dict] = await this.#kojed.call('Config', name);
            if (name !== this.#selected)
                return;
            this.#config = dict as unknown as model.Config;
            this.#original = model.form(this.#config);
            this.#form = model.form(this.#config);
            this.#fillForm();
        } catch (e) {
            this.#config = null;
            this.#error(e);
        }
        await this.#loadStates();
        await this.#loadWindows();
    }

    #get<T>(id: string): T {
        return this.#builder.get_object(id) as T;
    }

    #connectForm(): void {
        const entry = (id: string, key: 'prefix' | 'label' | 'icon' | 'memorySoft' | 'memoryHard') => {
            this.#get<Adw.EntryRow>(id).connect('changed', row => this.#edit({[key]: row.text}));
        };
        entry('prefix_row', 'prefix');
        entry('label_row', 'label');
        entry('icon_row', 'icon');
        entry('soft_row', 'memorySoft');
        entry('hard_row', 'memoryHard');
        this.#get<Gtk.ColorDialogButton>('color_button').connect('notify::rgba', button => {
            const c = button.rgba;
            const hex = (x: number) => Math.round(x * 255).toString(16).padStart(2, '0').toUpperCase();
            this.#edit({color: `#${hex(c.red)}${hex(c.green)}${hex(c.blue)}`});
        });
        this.#get<Adw.ComboRow>('profile_row').connect('notify::selected', row => {
            if (this.#filling || !this.#form || !this.#config)
                return;
            const profile = model.profiles(this.#config.default_profile)[row.selected]?.[0] ?? '';
            if (profile === this.#form.profile)
                return;
            // A profile as it is: its switches folded away.
            this.#form = model.withProfile(this.#form, profile);
            this.#get<Adw.ExpanderRow>('switches_row').expanded = false;
            this.#formChanged();
        });
        const group = this.#get<Adw.ExpanderRow>('switches_row');
        for (const [name, title] of model.SWITCHES) {
            const row = new Adw.SwitchRow({title});
            row.connect('notify::active', r => {
                if (this.#filling || !this.#form || !this.#config)
                    return;
                this.#form = model.withSwitch(this.#form, this.#config.bases, name, r.active);
                this.#formChanged();
            });
            this.#switchRows.set(name, row);
            group.add_row(row);
        }
        this.#get<Gtk.Button>('add_mount').connect('clicked', () => void this.#addMount());
        this.#get<Gtk.Button>('revert_button').connect('clicked', () => {
            if (this.#original)
                this.#form = {...this.#original};
            this.#fillForm();
        });
        this.#get<Gtk.Button>('save_button').connect('clicked', () => void this.#save());
    }

    #edit(change: Partial<model.Form>): void {
        if (this.#filling || !this.#form)
            return;
        this.#form = {...this.#form, ...change};
        this.#formChanged();
    }

    /** Put the form into the widgets (from code: no edits). */
    #fillForm(): void {
        const f = this.#form;
        const c = this.#config;
        if (!f || !c)
            return;
        this.#filling = true;
        this.#get<Adw.EntryRow>('prefix_row').text = f.prefix;
        this.#get<Adw.EntryRow>('label_row').text = f.label;
        this.#get<Adw.EntryRow>('icon_row').text = f.icon;
        this.#get<Adw.EntryRow>('soft_row').text = f.memorySoft;
        this.#get<Adw.EntryRow>('hard_row').text = f.memoryHard;
        const rgba = new Gdk.RGBA();
        if (rgba.parse(f.color))
            this.#get<Gtk.ColorDialogButton>('color_button').rgba = rgba;
        const profiles = model.profiles(c.default_profile);
        const profileRow = this.#get<Adw.ComboRow>('profile_row');
        profileRow.model = Gtk.StringList.new(profiles.map(([, label]) => label));
        profileRow.selected = Math.max(0, profiles.findIndex(([p]) => p === f.profile));
        this.#get<Adw.ExpanderRow>('switches_row').expanded = false;
        this.#fillImageChoices(f.image);
        this.#fillContainer(c);
        this.#fillMounts();
        this.#filling = false;
        this.#formChanged();
    }

    #fillMounts(): void {
        const group = this.#get<Adw.PreferencesGroup>('mounts_group');
        this.#mountRows.forEach(r => group.remove(r));
        this.#mountRows = (this.#form?.mounts ?? []).map(path => {
            const row = new Adw.ActionRow({title: path});
            const remove = new Gtk.Button({
                icon_name: 'user-trash-symbolic', tooltip_text: 'Remove', valign: Gtk.Align.CENTER,
                css_classes: ['flat'],
            });
            remove.connect('clicked', () => {
                if (!this.#form)
                    return;
                this.#form = {...this.#form, mounts: this.#form.mounts.filter(m => m !== path)};
                this.#fillMounts();
                this.#formChanged();
            });
            row.add_suffix(remove);
            group.add(row);
            return row;
        });
    }

    async #addMount(): Promise<void> {
        const dialog = new Gtk.FileDialog({title: 'Add Folder', initial_folder: Gio.File.new_for_path(GLib.get_home_dir())});
        let folder: Gio.File | null = null;
        try {
            folder = await dialog.select_folder(this.#window, null);
        } catch (e) {
            if (!(e instanceof GLib.Error && e.matches(Gtk.DialogError, Gtk.DialogError.DISMISSED)))
                this.#error(e);
            return;
        }
        const path = folder?.get_path();
        if (!path || !this.#form)
            return;
        const mount = model.mountPath(path, GLib.get_home_dir());
        if (!mount) {
            this.#toast('Only folders below your home can be added (ADR 0034)');
            return;
        }
        if (!this.#form.mounts.includes(mount)) {
            this.#form = {...this.#form, mounts: [...this.#form.mounts, mount]};
            this.#fillMounts();
            this.#formChanged();
        }
    }

    /** Switches, field errors and the Save button after an edit. */
    #formChanged(): void {
        const f = this.#form;
        const c = this.#config;
        if (!f || !c || !this.#original)
            return;
        this.#filling = true;
        const on = model.effective(f, c.bases);
        for (const [name, row] of this.#switchRows) {
            row.active = on[name] ?? false;
            row.subtitle = name in f.access ? 'Changed from the profile' : '';
        }
        this.#get<Adw.ComboRow>('profile_row').subtitle = model.profileSummary(c.bases, f.profile);
        const img = this.#images.find(i => i.name === f.image);
        this.#get<Adw.ComboRow>('image_choice_row').subtitle =
            img?.description || (img ? '' : 'A registry image or a folder, as written in context.toml');
        this.#get<Adw.ExpanderRow>('switches_row').subtitle = model.adjustments(f);
        this.#filling = false;
        const problems = model.problems(f);
        const rows: [string, keyof model.Form][] = [
            ['prefix_row', 'prefix'], ['icon_row', 'icon'], ['soft_row', 'memorySoft'], ['hard_row', 'memoryHard'],
        ];
        for (const [id, key] of rows) {
            const row = this.#get<Adw.EntryRow>(id);
            if (problems[key]) {
                row.add_css_class('error');
                row.tooltip_text = problems[key] ?? null;
            } else {
                row.remove_css_class('error');
                row.tooltip_text = null;
            }
        }
        const changed = Object.keys(model.changes(this.#original, f)).length > 0;
        this.#get<Gtk.Button>('save_button').sensitive = changed && Object.keys(problems).length === 0;
        this.#get<Gtk.Button>('revert_button').sensitive = changed;
    }

    async #save(): Promise<void> {
        if (!this.#form || !this.#original)
            return;
        const changes = model.changes(this.#original, this.#form);
        const dict = Object.fromEntries(Object.entries(changes).map(([k, v]) => [k, variant(v)]));
        try {
            const [notes] = await this.#kojed.call('SetConfig', this.#selected, dict);
            this.#original = {...this.#form};
            this.#formChanged();
            this.#toast(notes.length ? `Saved – ${notes.join('; ')}` : 'Saved');
        } catch (e) {
            this.#error(e);
        }
    }

    async #loadStates(): Promise<void> {
        const name = this.#selected;
        const group = this.#get<Adw.PreferencesGroup>('states_group');
        let states: model.State[] = [];
        try {
            const [rows] = await this.#kojed.call('States', name);
            states = model.states(rows);
        } catch (e) {
            this.#error(e);
        }
        if (name !== this.#selected)
            return;
        this.#stateRows.forEach(r => group.remove(r));
        const now = Math.floor(Date.now() / 1000);
        const running = this.#current()?.status === 'running';
        this.#stateRows = states.map(s => {
            const row = new Adw.ActionRow({title: s.name, subtitle: model.stateSubtitle(s, now)});
            if (s.current)
                row.add_prefix(new Gtk.Image({icon_name: 'object-select-symbolic', tooltip_text: 'The state the context is in'}));
            if (s.role) {
                row.add_suffix(new Gtk.Label({
                    label: s.role, valign: Gtk.Align.CENTER, css_classes: ['caption', 'dim-label'],
                }));
            }
            row.add_suffix(this.#stateMenu(name, s, running));
            group.add(row);
            return row;
        });
        this.#get<Gtk.Button>('save_as').sensitive = running;
    }

    #stateMenu(context: string, s: model.State, running: boolean): Gtk.MenuButton {
        const menu = new Gio.Menu();
        const group = new Gio.SimpleActionGroup();
        const labels = {
            load: running ? 'Switch to This State' : 'Start with This State',
            save: 'Save Windows Here', copy: 'Copy…', rename: 'Rename…', remove: 'Remove…',
        };
        for (const action of model.stateActions(s, running)) {
            menu.append(labels[action], `state.${action}`);
            const a = new Gio.SimpleAction({name: action});
            a.connect('activate', () => void this.#stateAction(context, s, action));
            group.add_action(a);
        }
        const button = new Gtk.MenuButton({
            icon_name: 'view-more-symbolic', valign: Gtk.Align.CENTER, css_classes: ['flat'], menu_model: menu,
        });
        button.insert_action_group('state', group);
        return button;
    }

    async #stateAction(context: string, s: model.State, action: ReturnType<typeof model.stateActions>[number]): Promise<void> {
        try {
            switch (action) {
            case 'load':
                await this.#up(context, s.name);
                break;
            case 'save':
                this.#saved(await this.#kojed.call('Save', context, s.name));
                break;
            case 'copy': {
                const to = await askName(this.#window, `Copy ${s.name}`, 'Copy', `${s.name}-copy`);
                if (to)
                    await this.#kojed.call('CopyState', context, s.name, to);
                break;
            }
            case 'rename': {
                const to = await askName(this.#window, `Rename ${s.name}`, 'Rename', s.name);
                if (to && to !== s.name)
                    await this.#kojed.call('RenameState', context, s.name, to);
                break;
            }
            case 'remove': {
                const answer = await ask(this.#window, `Remove ${s.name}?`,
                    s.role ? `It is the ${s.role} state; the role then falls back.` : 'It cannot be restored.',
                    [['remove', 'Remove', Adw.ResponseAppearance.DESTRUCTIVE]]);
                if (answer === 'remove')
                    await this.#kojed.call('RemoveState', context, s.name);
                break;
            }
            }
        } catch (e) {
            this.#error(e);
        }
    }

    #saved([state, windows]: [string, number]): void {
        this.#toast(windows === 1 ? `Saved 1 window into ${state}` : `Saved ${windows} windows into ${state}`);
    }

    async #saveAs(): Promise<void> {
        const name = await askName(this.#window, 'Save Windows As', 'Save');
        if (!name)
            return;
        try {
            this.#saved(await this.#kojed.call('Save', this.#selected, name));
        } catch (e) {
            this.#error(e);
        }
    }

    async #loadWindows(): Promise<void> {
        const name = this.#selected;
        const group = this.#get<Adw.PreferencesGroup>('windows_group');
        let windows: [string, string, string, string][] = [];
        if (this.#current()?.status === 'running' || this.#current()?.status === 'paused') {
            try {
                [windows] = await this.#kojed.call('Windows', name);
            } catch {
                // not known yet: none
            }
        }
        if (name !== this.#selected)
            return;
        this.#windowRows.forEach(r => group.remove(r));
        this.#windowRows = windows.map(([id, appId, command, integration]) => {
            const row = new Adw.ActionRow({
                title: GLib.markup_escape_text(appId || id, -1),
                subtitle: GLib.markup_escape_text([command, integration && `(${integration})`].filter(Boolean).join(' '), -1),
            });
            group.add(row);
            return row;
        });
        if (!windows.length) {
            const row = new Adw.ActionRow({title: 'No windows', subtitle: 'The context is not running, or has no windows.'});
            group.add(row);
            this.#windowRows = [row];
        }
    }
}
