//! The settings app (M8.9–M8.15): `koje-shell settings [<context>]`, its own
//! process and one instance (`io.koje.Settings`; a second start shows the
//! window, on the given context). A libadwaita window as GNOME's apps: the
//! contexts in the sidebar with their color and status, Memory on top; per
//! context the pages Context, States, Layouts and Windows. What they show is
//! [`crate::settings`]; changes run `koje` (`settings_backend`).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

use crate::settings::{
    self, ContextItem, Form, Snapshot, Status, form, matrix, memory_fraction, memory_label,
    state_row,
};
use crate::settings_backend::{self, Command, Event};

const APP_ID: &str = "io.koje.Settings";
/// The sidebar's first row.
const MEMORY: &str = "";

const CSS: &str = "
.context-dot { min-width: 12px; min-height: 12px; border-radius: 999px; }
.swatch-button { min-width: 24px; min-height: 24px; padding: 0; border-radius: 999px; }
.swatch-button:checked { outline: 2px solid var(--accent-bg-color); outline-offset: 2px; }
";

/// Start the settings app (its own process; a running one shows its window),
/// on `context` if given – from the bar's koje menu (M8.15).
pub fn open(context: Option<&str>) {
    let exe = std::env::current_exe().unwrap_or_else(|_| "koje-shell".into());
    let mut argv: Vec<std::ffi::OsString> = vec![exe.into_os_string(), "settings".into()];
    if let Some(c) = context {
        argv.push(c.into());
    }
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsString::as_os_str).collect();
    if let Err(e) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        tracing::warn!("starting the settings app: {e}");
    }
}

pub fn run(args: &[String]) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Rc::default();
    app.connect_command_line(move |app, line| {
        let wanted = line
            .arguments()
            .get(1)
            .and_then(|a| a.to_str().map(str::to_owned));
        let ui = ui.borrow_mut().get_or_insert_with(|| Ui::new(app)).clone();
        if let Some(c) = wanted {
            ui.select(&c);
        }
        ui.window.present();
        glib::ExitCode::SUCCESS
    });
    let mut command_line = vec!["koje-shell-settings".to_owned()];
    command_line.extend(args.iter().cloned());
    app.run_with_args(&command_line)
}

/// Per-context CSS: the sidebar's dots in the contexts' colors.
fn dots_css(snapshot: &Snapshot) -> String {
    use std::fmt::Write as _;
    let mut css = String::new();
    for c in &snapshot.contexts {
        if let Ok(cfg) = &c.config {
            let _ = writeln!(css, ".dot-{} {{ background: {}; }}", c.name, cfg.color);
        }
    }
    css
}

pub(crate) struct Ui {
    window: adw::ApplicationWindow,
    commands: UnboundedSender<Command>,
    toasts: adw::ToastOverlay,
    sidebar: gtk::ListBox,
    main_page: adw::NavigationPage,
    split: adw::NavigationSplitView,
    pages: gtk::Stack,
    memory: adw::PreferencesGroup,
    context_page: adw::PreferencesPage,
    states_page: adw::PreferencesPage,
    layouts_page: adw::PreferencesPage,
    windows_page: adw::PreferencesPage,
    actions_box: gtk::Box,
    dots: gtk::CssProvider,
    snapshot: RefCell<Snapshot>,
    /// The sidebar's selection: a context, or [`MEMORY`].
    selected: RefCell<String>,
    /// The context form as built, and whether the user changed it since.
    built: RefCell<Option<(String, Form)>>,
    dirty: Cell<bool>,
    busy: RefCell<BTreeSet<String>>,
    /// The sidebar is being rebuilt: its selection signals are ours.
    filling: Cell<bool>,
    /// What the sidebar and the context's pages were last built from: they
    /// are only rebuilt when it changed (open menus and typing survive).
    shown_sidebar: RefCell<(String, Vec<(String, Status)>)>,
    shown_item: RefCell<Option<ContextItem>>,
}

impl Ui {
    fn new(app: &adw::Application) -> Rc<Self> {
        let dots = style();
        let (tx, rx) = async_channel::unbounded();
        let (commands, commands_rx) = tokio::sync::mpsc::unbounded_channel();
        if let Err(e) = settings_backend::spawn(tx, commands_rx) {
            tracing::error!("starting the settings backend: {e:#}");
        }

        let (sidebar, sidebar_page, new_context) = sidebar();
        let (stack, switcher, [context_page, states_page, layouts_page, windows_page]) =
            context_pages();

        let memory = adw::PreferencesGroup::builder()
            .title("Memory")
            .description("Each running context's working set against its limit. Limits are set on a context's page.")
            .build();
        let memory_page = adw::PreferencesPage::new();
        memory_page.add(&memory);

        let pages = gtk::Stack::new();
        pages.add_named(&memory_page, Some("memory"));
        pages.add_named(&stack, Some("context"));
        let actions_box = gtk::Box::builder().spacing(6).build();
        let header = adw::HeaderBar::new();
        header.pack_end(&actions_box);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&header);
        content_view.set_content(Some(&pages));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&content_view));
        let main_page = adw::NavigationPage::builder()
            .title("Memory")
            .child(&toasts)
            .build();
        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&main_page)
            .min_sidebar_width(220.0)
            .build();
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("koje Settings")
            .default_width(960)
            .default_height(680)
            .content(&split)
            .build();
        {
            let (header, switcher) = (header.clone(), switcher.clone());
            pages.connect_visible_child_name_notify(move |p| {
                let context = p.visible_child_name().as_deref() == Some("context");
                header.set_title_widget(context.then_some(&switcher));
            });
        }

        let ui = Rc::new(Self {
            window,
            commands,
            toasts,
            sidebar,
            main_page,
            split,
            pages,
            memory,
            context_page,
            states_page,
            layouts_page,
            windows_page,
            actions_box,
            dots,
            snapshot: RefCell::default(),
            selected: RefCell::new(MEMORY.to_owned()),
            built: RefCell::default(),
            dirty: Cell::new(false),
            busy: RefCell::default(),
            filling: Cell::new(false),
            shown_sidebar: RefCell::default(),
            shown_item: RefCell::default(),
        });
        ui.connect(&new_context, rx);
        ui.render();
        ui
    }

    /// The sidebar's and the backend's signals.
    fn connect(self: &Rc<Self>, new_context: &gtk::Button, rx: async_channel::Receiver<Event>) {
        {
            let me = Rc::downgrade(self);
            new_context.connect_clicked(move |_| {
                if let Some(me) = me.upgrade() {
                    // `koje new` picks a color; the context page changes it.
                    me.ask_name("New Context", "Create", vec!["new".into()], "");
                }
            });
        }
        {
            let me = Rc::downgrade(self);
            // Activated, not selected: GTK selects the first row by itself
            // when the list gets the focus.
            self.sidebar.connect_row_activated(move |_, row| {
                let Some(me) = me.upgrade() else {
                    return;
                };
                if me.filling.get() {
                    return;
                }
                let name = row.widget_name().to_string();
                me.select(&name);
                me.split.set_show_content(true);
            });
        }
        {
            let me = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                while let Ok(event) = rx.recv().await {
                    let Some(me) = me.upgrade() else { return };
                    me.on_event(event);
                }
            });
        }
    }

    fn on_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Snapshot(s) => {
                self.dots.load_from_string(&dots_css(&s));
                *self.snapshot.borrow_mut() = *s;
                self.render();
            }
            Event::Busy { context, busy } => {
                if busy {
                    self.busy.borrow_mut().insert(context);
                } else {
                    self.busy.borrow_mut().remove(&context);
                }
                self.render_actions();
            }
            Event::Failed { context, error } => {
                let toast = adw::Toast::builder()
                    .title(glib::markup_escape_text(&format!("{context}: {error}")))
                    .timeout(8)
                    .build();
                self.toasts.add_toast(toast);
            }
        }
    }

    fn run(&self, context: &str, args: &[&str], input: Option<String>) {
        let _ = self.commands.send(Command {
            context: context.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            input,
        });
    }

    /// Show a context (or [`MEMORY`]).
    fn select(self: &Rc<Self>, name: &str) {
        if *self.selected.borrow() != name {
            self.selected.replace(name.to_owned());
            self.built.replace(None);
            self.dirty.set(false);
        }
        self.render();
    }

    fn item(&self) -> Option<ContextItem> {
        let selected = self.selected.borrow();
        self.snapshot
            .borrow()
            .contexts
            .iter()
            .find(|c| c.name == *selected)
            .cloned()
    }

    fn render(self: &Rc<Self>) {
        let sidebar = (
            self.selected.borrow().clone(),
            self.snapshot
                .borrow()
                .contexts
                .iter()
                .map(|c| (c.name.clone(), c.status))
                .collect::<Vec<_>>(),
        );
        if *self.shown_sidebar.borrow() != sidebar || self.sidebar.first_child().is_none() {
            self.render_sidebar();
            self.shown_sidebar.replace(sidebar);
        }
        self.render_memory();
        if let Some(item) = self.item() {
            self.pages.set_visible_child_name("context");
            self.main_page.set_title(&item.name);
            self.render_context(&item);
            // Memory readings change all the time; the pages do not show them.
            let key = ContextItem {
                memory: None,
                ..item.clone()
            };
            if self.shown_item.borrow().as_ref() != Some(&key) {
                self.render_states(&item);
                self.render_layouts(&item);
                self.render_windows(&item);
                self.shown_item.replace(Some(key));
            }
        } else {
            self.pages.set_visible_child_name("memory");
            self.main_page.set_title("Memory");
        }
        self.render_actions();
    }

    fn render_sidebar(&self) {
        self.filling.set(true);
        while let Some(child) = self.sidebar.first_child() {
            self.sidebar.remove(&child);
        }
        let row = |name: &str, title: &str, subtitle: &str, dot: Option<&str>| {
            let b = gtk::Box::builder()
                .spacing(12)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            match dot {
                Some(class) => b.append(
                    &gtk::Box::builder()
                        .css_classes(["context-dot", class])
                        .valign(gtk::Align::Center)
                        .build(),
                ),
                None => b.append(&gtk::Image::from_icon_name(
                    "power-profile-performance-symbolic",
                )),
            }
            let texts = gtk::Box::new(gtk::Orientation::Vertical, 0);
            texts.append(&gtk::Label::builder().label(title).xalign(0.0).build());
            if !subtitle.is_empty() {
                texts.append(
                    &gtk::Label::builder()
                        .label(subtitle)
                        .xalign(0.0)
                        .css_classes(["caption", "dim-label"])
                        .build(),
                );
            }
            b.append(&texts);
            let r = gtk::ListBoxRow::builder().child(&b).build();
            r.set_widget_name(name);
            r
        };
        let memory = row(MEMORY, "Memory", "", None);
        self.sidebar.append(&memory);
        let selected = self.selected.borrow().clone();
        let mut select = (selected == MEMORY).then(|| memory.clone());
        for c in &self.snapshot.borrow().contexts {
            let r = row(
                &c.name,
                &c.name,
                c.status.label(),
                Some(&format!("dot-{}", c.name)),
            );
            if c.name == selected {
                select = Some(r.clone());
            }
            self.sidebar.append(&r);
        }
        self.sidebar.select_row(select.as_ref());
        self.filling.set(false);
    }

    fn render_memory(self: &Rc<Self>) {
        clear_group(&self.memory);
        let snapshot = self.snapshot.borrow();
        if !snapshot.kojed {
            self.memory.add(
                &adw::ActionRow::builder()
                    .title("kojed is not running")
                    .build(),
            );
            return;
        }
        for c in &snapshot.contexts {
            let row = adw::ActionRow::builder().title(&c.name).build();
            match c.memory {
                Some((used, soft, hard)) => {
                    row.set_subtitle(&memory_label(used, soft, hard));
                    let bar = gtk::LevelBar::builder()
                        .value(memory_fraction(used, soft, hard))
                        .width_request(160)
                        .valign(gtk::Align::Center)
                        .build();
                    row.add_suffix(&bar);
                }
                None => row.set_subtitle(c.status.label()),
            }
            row.set_activatable(true);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let me = Rc::downgrade(self);
            let name = c.name.clone();
            row.connect_activated(move |_| {
                if let Some(me) = me.upgrade() {
                    me.select(&name);
                }
            });
            self.memory.add(&row);
        }
    }

    /// Start, stop, pause and wake in the header, as the context's status
    /// allows; a spinner while a command runs.
    fn render_actions(self: &Rc<Self>) {
        while let Some(child) = self.actions_box.first_child() {
            self.actions_box.remove(&child);
        }
        let Some(item) = self.item() else { return };
        if self.busy.borrow().contains(&item.name) {
            self.actions_box.append(&adw::Spinner::new());
            return;
        }
        let button = |icon: &str, tip: &str, args: &[&str]| {
            let b = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tip)
                .build();
            let me = Rc::downgrade(self);
            let (name, args): (String, Vec<String>) = (
                item.name.clone(),
                args.iter().map(|a| (*a).to_owned()).collect(),
            );
            b.connect_clicked(move |_| {
                if let Some(me) = me.upgrade() {
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    me.run(&name, &args, None);
                }
            });
            self.actions_box.append(&b);
        };
        let n = item.name.as_str();
        match item.status {
            Status::Running => {
                button("media-playback-pause-symbolic", "Pause", &["sleep", n]);
                button("system-shutdown-symbolic", "Stop", &["down", n]);
            }
            Status::Paused => {
                button("media-playback-start-symbolic", "Wake", &["wake", n]);
                button("system-shutdown-symbolic", "Stop", &["down", n]);
            }
            Status::Stopped => button(
                "media-playback-start-symbolic",
                "Start",
                &["up", n, "--no-focus"],
            ),
            Status::Unknown => {}
        }
    }

    /// The context page: built from the config when the context is shown or
    /// its file changed, unless the user is editing it.
    fn render_context(self: &Rc<Self>, item: &ContextItem) {
        let cfg = match &item.config {
            Ok(cfg) => cfg,
            Err(e) => {
                clear_page(&self.context_page);
                let g = adw::PreferencesGroup::new();
                g.add(
                    &adw::ActionRow::builder()
                        .title("context.toml cannot be read")
                        .subtitle(e)
                        .build(),
                );
                self.context_page.add(&g);
                return;
            }
        };
        let fresh = form(cfg);
        let same = self
            .built
            .borrow()
            .as_ref()
            .is_some_and(|(n, f)| *n == item.name && (*f == fresh || self.dirty.get()));
        if same {
            return;
        }
        self.built.replace(Some((item.name.clone(), fresh.clone())));
        self.dirty.set(false);
        crate::settings_form::build(self, &item.name, cfg, fresh);
    }

    fn render_states(self: &Rc<Self>, item: &ContextItem) {
        clear_page(&self.states_page);
        let group = adw::PreferencesGroup::builder()
            .title("States")
            .description("Which windows run and how they are arranged. Loading one in a running context switches to it.")
            .build();
        for s in &item.states {
            let (name, subtitle) = state_row(s);
            let row = adw::ActionRow::builder()
                .title(&name)
                .subtitle(&subtitle)
                .build();
            let load = gtk::Button::builder()
                .label("Load")
                .valign(gtk::Align::Center)
                .build();
            {
                let me = Rc::downgrade(self);
                let (ctx, state) = (item.name.clone(), name.clone());
                load.connect_clicked(move |_| {
                    if let Some(me) = me.upgrade() {
                        me.run(&ctx, &["up", &ctx, "-s", &state, "--no-focus"], None);
                    }
                });
            }
            row.add_suffix(&load);
            if name != "empty" && s.summary.is_some() {
                row.add_suffix(&self.state_menu(&item.name, &name));
            }
            group.add(&row);
        }
        self.states_page.add(&group);
    }

    /// Copy, rename and delete a state, each asking first.
    fn state_menu(self: &Rc<Self>, ctx: &str, state: &str) -> gtk::MenuButton {
        let items = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let popover = gtk::Popover::builder().child(&items).build();
        let entries: [(&str, &str); 3] = [("Copy…", "cp"), ("Rename…", "mv"), ("Delete…", "rm")];
        for (label, verb) in entries {
            let b = gtk::Button::builder()
                .label(label)
                .css_classes(["flat"])
                .build();
            let me = Rc::downgrade(self);
            let (ctx, state, popover) = (ctx.to_owned(), state.to_owned(), popover.clone());
            b.connect_clicked(move |_| {
                popover.popdown();
                let Some(me) = me.upgrade() else { return };
                if verb == "rm" {
                    me.confirm(
                        &format!("Delete the state {state}?"),
                        "Its windows and layouts are gone for good.",
                        "Delete",
                        vec!["state".into(), "rm".into(), ctx.clone(), state.clone()],
                        &ctx,
                    );
                } else {
                    me.ask_name(
                        &format!("{} {state}", if verb == "cp" { "Copy" } else { "Rename" }),
                        if verb == "cp" { "Copy" } else { "Rename" },
                        vec!["state".into(), verb.into(), ctx.clone(), state.clone()],
                        &ctx,
                    );
                }
            });
            items.append(&b);
        }
        gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .popover(&popover)
            .build()
    }

    fn render_layouts(self: &Rc<Self>, item: &ContextItem) {
        clear_page(&self.layouts_page);
        let running = item.status == Status::Running;
        let group = adw::PreferencesGroup::builder()
            .title("Layouts")
            .description(if running {
                "Arrangements of the running windows; switching hides the others in the stash."
            } else {
                "The layouts of the state the context starts with. Start it to change them."
            })
            .build();
        let Some(state) = &item.live else {
            group.add(
                &adw::ActionRow::builder()
                    .title("No layouts saved yet")
                    .build(),
            );
            self.layouts_page.add(&group);
            return;
        };
        let m = matrix(state);
        for (name, active, autosave, shown) in &m.layouts {
            let windows = shown.iter().filter(|s| **s).count();
            let row = adw::ActionRow::builder()
                .title(name)
                .subtitle(settings::plural(windows, "window"))
                .build();
            if *active {
                row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
            }
            if running {
                let auto = gtk::Switch::builder()
                    .active(*autosave)
                    .valign(gtk::Align::Center)
                    .tooltip_text("Save automatically")
                    .build();
                {
                    let me = Rc::downgrade(self);
                    let (ctx, layout) = (item.name.clone(), name.clone());
                    auto.connect_state_set(move |_, on| {
                        if let Some(me) = me.upgrade() {
                            let v = if on { "on" } else { "off" };
                            me.run(&ctx, &["layout", "autosave", &layout, v, "-c", &ctx], None);
                        }
                        glib::Propagation::Proceed
                    });
                }
                row.add_suffix(&auto);
                if !*active {
                    let switch = gtk::Button::builder()
                        .label("Switch")
                        .valign(gtk::Align::Center)
                        .build();
                    let me = Rc::downgrade(self);
                    let (ctx, layout) = (item.name.clone(), name.clone());
                    switch.connect_clicked(move |_| {
                        if let Some(me) = me.upgrade() {
                            me.run(&ctx, &["layout", &layout, "-c", &ctx], None);
                        }
                    });
                    row.add_suffix(&switch);
                }
                row.add_suffix(&self.layout_menu(&item.name, name, *active));
            } else if *autosave {
                row.set_subtitle(&format!(
                    "{} · saved automatically",
                    settings::plural(windows, "window")
                ));
            }
            group.add(&row);
        }
        if running {
            let new = adw::EntryRow::builder()
                .title("New layout")
                .show_apply_button(true)
                .build();
            let me = Rc::downgrade(self);
            let ctx = item.name.clone();
            new.connect_apply(move |e| {
                if let Some(me) = me.upgrade() {
                    let name = e.text().to_string();
                    me.run(&ctx, &["layout", "new", &name, "--copy", "-c", &ctx], None);
                    e.set_text("");
                }
            });
            group.add(&new);
        }
        self.layouts_page.add(&group);
        self.layouts_page.add(&matrix_group(&m));
    }

    /// Rename and delete a layout (not the active one), each asking first.
    fn layout_menu(self: &Rc<Self>, ctx: &str, layout: &str, active: bool) -> gtk::MenuButton {
        let items = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let popover = gtk::Popover::builder().child(&items).build();
        let rename = gtk::Button::builder()
            .label("Rename…")
            .css_classes(["flat"])
            .build();
        {
            let me = Rc::downgrade(self);
            let (ctx, layout, popover) = (ctx.to_owned(), layout.to_owned(), popover.clone());
            rename.connect_clicked(move |_| {
                popover.popdown();
                if let Some(me) = me.upgrade() {
                    me.ask_name(
                        &format!("Rename {layout}"),
                        "Rename",
                        vec![
                            "layout".into(),
                            "mv".into(),
                            layout.clone(),
                            "-c".into(),
                            ctx.clone(),
                        ],
                        &ctx,
                    );
                }
            });
        }
        items.append(&rename);
        if !active {
            let delete = gtk::Button::builder()
                .label("Delete…")
                .css_classes(["flat"])
                .build();
            let me = Rc::downgrade(self);
            let (ctx, layout, popover) = (ctx.to_owned(), layout.to_owned(), popover.clone());
            delete.connect_clicked(move |_| {
                popover.popdown();
                if let Some(me) = me.upgrade() {
                    me.confirm(
                        &format!("Delete the layout {layout}?"),
                        "Its windows stay open; they are not arranged by it any more.",
                        "Delete",
                        vec![
                            "layout".into(),
                            "rm".into(),
                            layout.clone(),
                            "-c".into(),
                            ctx.clone(),
                        ],
                        &ctx,
                    );
                }
            });
            items.append(&delete);
        }
        gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .popover(&popover)
            .build()
    }

    fn render_windows(&self, item: &ContextItem) {
        clear_page(&self.windows_page);
        let group = adw::PreferencesGroup::builder()
            .title("Windows")
            .description("The windows koje remembers in this context and how it starts them again.")
            .build();
        match &item.live {
            Some(state) if !state.windows.is_empty() => {
                for w in &state.windows {
                    let how = match (&w.integration, &w.command) {
                        (Some(i), _) => format!("Integration {i}"),
                        (None, Some(c)) => c.clone(),
                        (None, None) => String::new(),
                    };
                    group.add(
                        &adw::ActionRow::builder()
                            .title(settings::window_label(w))
                            .subtitle(format!("{} · {how}", w.id))
                            .build(),
                    );
                }
            }
            _ => group.add(&adw::ActionRow::builder().title("No windows").build()),
        }
        self.windows_page.add(&group);
    }

    /// A question before something that cannot be undone; runs `args` on yes.
    fn confirm(
        self: &Rc<Self>,
        heading: &str,
        body: &str,
        verb: &str,
        args: Vec<String>,
        ctx: &str,
    ) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_responses(&[("cancel", "Cancel"), ("yes", verb)]);
        dialog.set_response_appearance("yes", adw::ResponseAppearance::Destructive);
        let me = Rc::downgrade(self);
        let ctx = ctx.to_owned();
        dialog.connect_response(None, move |_, response| {
            if response == "yes"
                && let Some(me) = me.upgrade()
            {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                me.run(&ctx, &args, None);
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Ask for a name, then run `args` with it appended.
    fn ask_name(self: &Rc<Self>, heading: &str, verb: &str, args: Vec<String>, ctx: &str) {
        let dialog = adw::AlertDialog::new(Some(heading), None);
        let entry = gtk::Entry::builder().placeholder_text("Name").build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("ok", verb)]);
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        let me = Rc::downgrade(self);
        let ctx = ctx.to_owned();
        dialog.connect_response(None, move |_, response| {
            if response == "ok"
                && let Some(me) = me.upgrade()
            {
                let mut args = args.clone();
                args.push(entry.text().trim().to_owned());
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                me.run(&ctx, &args, None);
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(crate) fn context_page(&self) -> &adw::PreferencesPage {
        &self.context_page
    }

    pub(crate) fn global(&self) -> koje_core::config::GlobalConfig {
        self.snapshot.borrow().global.clone()
    }

    pub(crate) fn set_dirty(&self) {
        self.dirty.set(true);
    }

    pub(crate) fn saved(&self) {
        self.dirty.set(false);
        self.built.replace(None);
    }

    pub(crate) fn toast(&self, text: &str) {
        self.toasts
            .add_toast(adw::Toast::new(&glib::markup_escape_text(text)));
    }

    pub(crate) fn command(&self, context: &str, args: &[&str], input: Option<String>) {
        self.run(context, args, input);
    }

    pub(crate) fn ask(
        self: &Rc<Self>,
        heading: &str,
        body: &str,
        verb: &str,
        args: Vec<String>,
        ctx: &str,
    ) {
        self.confirm(heading, body, verb, args, ctx);
    }
}

/// The app's stylesheet; the returned provider takes the sidebar's dots.
fn style() -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("{CSS}{}", crate::swatch_css()));
    let dots = gtk::CssProvider::new();
    if let Some(display) = gtk::gdk::Display::default() {
        for p in [&provider, &dots] {
            gtk::style_context_add_provider_for_display(
                &display,
                p,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }
    dots
}

/// The sidebar's list in its page; "New Context…" in its header.
fn sidebar() -> (gtk::ListBox, adw::NavigationPage, gtk::Button) {
    let list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let new = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("New Context…")
        .build();
    header.pack_start(&new);
    view.add_top_bar(&header);
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    ));
    let page = adw::NavigationPage::builder()
        .title("koje")
        .child(&view)
        .build();
    (list, page, new)
}

/// A context's pages and the switcher for the header.
fn context_pages() -> (adw::ViewStack, adw::ViewSwitcher, [adw::PreferencesPage; 4]) {
    let stack = adw::ViewStack::new();
    let pages = [
        ("context", "Context", "preferences-system-symbolic"),
        ("states", "States", "document-save-symbolic"),
        ("layouts", "Layouts", "view-grid-symbolic"),
        ("windows", "Windows", "focus-windows-symbolic"),
    ]
    .map(|(name, title, icon)| {
        let p = adw::PreferencesPage::new();
        stack.add_titled_with_icon(&p, Some(name), title, icon);
        p
    });
    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    (stack, switcher, pages)
}

/// Which window is shown in which layout, read-only.
fn matrix_group(m: &settings::Matrix) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Windows in layouts")
        .build();
    if m.windows.is_empty() || m.layouts.is_empty() {
        return group;
    }
    let grid = gtk::Grid::builder()
        .column_spacing(18)
        .row_spacing(6)
        .margin_top(6)
        .build();
    for (col, (name, active, ..)) in m.layouts.iter().enumerate() {
        let l = gtk::Label::builder()
            .label(name)
            .css_classes(["heading"])
            .build();
        if *active {
            l.add_css_class("accent");
        }
        grid.attach(&l, i32::try_from(col).unwrap_or(0) + 1, 0, 1, 1);
    }
    for (row, (id, label)) in m.windows.iter().enumerate() {
        let r = i32::try_from(row).unwrap_or(0) + 1;
        grid.attach(
            &gtk::Label::builder()
                .label(format!("{label} ({id})"))
                .xalign(0.0)
                .build(),
            0,
            r,
            1,
            1,
        );
        for (col, (_, _, _, shown)) in m.layouts.iter().enumerate() {
            let icon = if shown.get(row).copied().unwrap_or(false) {
                "object-select-symbolic"
            } else {
                "window-minimize-symbolic"
            };
            let image = gtk::Image::from_icon_name(icon);
            if !shown.get(row).copied().unwrap_or(false) {
                image.add_css_class("dim-label");
                image.set_tooltip_text(Some("In the stash"));
            }
            grid.attach(&image, i32::try_from(col).unwrap_or(0) + 1, r, 1, 1);
        }
    }
    group.add(&grid);
    group
}

pub(crate) fn clear_page(page: &adw::PreferencesPage) {
    // Groups are the page's only children it hands out through `group`.
    let mut groups = Vec::new();
    let mut child = page.first_child();
    while let Some(c) = child {
        collect_groups(&c, &mut groups);
        child = c.next_sibling();
    }
    for g in groups {
        page.remove(&g);
    }
}

fn collect_groups(w: &gtk::Widget, out: &mut Vec<adw::PreferencesGroup>) {
    if let Ok(g) = w.clone().downcast::<adw::PreferencesGroup>() {
        out.push(g);
        return;
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        collect_groups(&c, out);
        child = c.next_sibling();
    }
}

fn clear_group(group: &adw::PreferencesGroup) {
    let mut rows = Vec::new();
    collect_rows(group.upcast_ref(), &mut rows);
    for r in rows {
        group.remove(&r);
    }
}

fn collect_rows(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    let mut child = w.first_child();
    while let Some(c) = child {
        if c.is::<adw::PreferencesRow>() {
            out.push(c.clone());
        } else {
            collect_rows(&c, out);
        }
        child = c.next_sibling();
    }
}
