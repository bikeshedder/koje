//! The overview of one monitor (ADR 0049): a layer-shell surface over the
//! area below the bar with the workspace strip on top, the current
//! workspace's windows as cards in the middle, the context's stash and the
//! dash below. It renders an [`OverviewView`]; the shell's `hide-overview`
//! action closes it on every monitor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::dash::DashEntry;
use crate::model::{Accent, Action, Shell};
use crate::overview::{
    OverviewView, Place, StripTarget, WindowCard, drop_window, plugin_send, strip_click,
};
use crate::search::{AppHit, AppItem, AppSection, SearchView, WindowHit};

/// Room between the cards.
const GAP: f64 = 32.0;
/// Below each card: its title.
const TITLE_HEIGHT: i32 = 32;
/// The cards wait for the plugin to tell that the windows landed – at most
/// this long, should it never tell.
const ARRIVAL: std::time::Duration = std::time::Duration::from_millis(1500);

pub const CSS: &str = "
window.koje-overview { background: #1d1d20; color: #ffffff; }
/* koje's plugin draws the background and the windows (ADR 0051). */
window.koje-overview.live { background: transparent; }
window.koje-overview.live.covered { background: #1d1d20; }
window.koje-overview.live .window-preview { background: transparent; box-shadow: none; }
/* While the plugin flies the windows in, the cards' rings, icons and titles
   wait, as GNOME's do: gone at once, back with a fade. */
window.koje-overview.live:not(.arriving) .window-card { transition: opacity 150ms ease-out; }
window.koje-overview.live.arriving .window-card { opacity: 0; }
.overview-strip { margin: 16px 0 8px 0; }
.overview-strip button {
  min-height: 28px; padding: 0 14px; border-radius: 9999px;
  background: alpha(#ffffff, 0.1); color: #ffffff; font-weight: bold;
}
.overview-strip button:hover { background: alpha(#ffffff, 0.17); }
.overview-strip button.koje-host.active { background: alpha(#ffffff, 0.3); }
.overview-strip button.context { background: alpha(var(--accent-bg-color), 0.45); }
.overview-strip button.context.active { background: var(--accent-bg-color); color: var(--accent-fg-color); }
.overview-strip button.empty { opacity: 0.6; }
.overview-strip button label { font-weight: normal; }
.overview-strip button.active label { font-weight: bold; }
.overview-strip .overlay-gap { min-width: 16px; }
/* A window's card dragged over a place it may go. */
.overview-strip button:drop(active) { box-shadow: inset 0 0 0 2px #ffffff; }
.window-preview {
  border-radius: 12px; padding: 0;
  background: alpha(var(--accent-bg-color), 0.22);
  border: 3px solid transparent; box-shadow: 0 2px 8px alpha(black, 0.4);
}
/* The close button sits on the window: hovering it keeps the ring. */
.window-card:hover .window-preview { border-color: alpha(#ffffff, 0.6); }
.window-preview.koje-host { background: alpha(#ffffff, 0.08); }
.window-card .window-close { opacity: 0; min-width: 28px; min-height: 28px; padding: 0; margin: 6px; }
.window-card:hover .window-close { opacity: 1; }
.window-title {
  margin-top: 6px; padding: 2px 10px; border-radius: 9999px;
  background: alpha(#000000, 0.5); font-size: 0.9em;
}
.overview-stash { margin: 8px 0 24px 0; }
.overview-stash > label { opacity: 0.7; font-weight: bold; }
.overview-stash button { border-radius: 9999px; padding: 4px 12px; background: alpha(#ffffff, 0.1); color: #ffffff; }
.overview-stash button:hover { background: alpha(#ffffff, 0.17); }
.overview-empty { opacity: 0.5; font-size: 1.2em; }
.dash { margin: 0 0 16px 0; padding: 6px; border-radius: 24px; background: alpha(#ffffff, 0.1); }
.dash button.dash-app { padding: 6px; border-radius: 16px; background: transparent; }
.dash button.dash-app:hover { background: alpha(#ffffff, 0.12); }
.dash .running-dot { min-width: 5px; min-height: 5px; border-radius: 9999px; background: #ffffff; }
.dash .koje-ctx .running-dot { background: var(--accent-color); min-width: 12px; }
.dash .context-mark { min-width: 24px; min-height: 3px; border-radius: 9999px; background: var(--accent-bg-color); }
.dash separator { margin: 8px 4px; background: alpha(#ffffff, 0.25); min-width: 1px; }
.dash button.dash-grid:checked { background: alpha(#ffffff, 0.2); }
entry.overview-search { margin-top: 16px; min-width: 320px; border-radius: 9999px; background: alpha(#ffffff, 0.1); color: #ffffff; }
button.app-tile { padding: 12px 6px; border-radius: 16px; background: transparent; color: #ffffff; }
button.app-tile:hover, flowboxchild:focus button.app-tile { background: alpha(#ffffff, 0.12); }
.app-tile .context-mark { min-width: 24px; min-height: 4px; border-radius: 9999px; background: var(--accent-bg-color); }
.grid-section { font-weight: bold; padding: 2px 12px; border-radius: 9999px; margin: 24px 0 8px 0; }
.grid-section.koje-ctx { background: var(--accent-bg-color); color: var(--accent-fg-color); }
.grid-section.koje-host { background: alpha(#ffffff, 0.15); }
button.window-hit { border-radius: 12px; padding: 6px 12px; background: alpha(#ffffff, 0.08); color: #ffffff; }
button.window-hit:hover { background: alpha(#ffffff, 0.15); }
.context-pill { font-weight: bold; padding: 2px 10px; border-radius: 9999px; background: var(--accent-bg-color); color: var(--accent-fg-color); }
.results-heading { opacity: 0.7; font-weight: bold; margin: 16px 0 8px 0; }
";

mod imp {
    use std::cell::RefCell;

    use super::{GAP, TITLE_HEIGHT, glib, gtk};
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    /// Places the cards with [`crate::overview::arrange`].
    #[derive(Default)]
    pub struct CardsLayout {
        /// Each card's window size, in child order.
        pub sizes: RefCell<Vec<(f64, f64)>>,
        /// Called after the cards got their places (the plugin follows them).
        pub allocated: RefCell<Option<Box<dyn Fn()>>>,
    }

    impl std::fmt::Debug for CardsLayout {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CardsLayout")
                .field("sizes", &self.sizes)
                .finish_non_exhaustive()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardsLayout {
        const NAME: &'static str = "KojeCardsLayout";
        type Type = super::CardsLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for CardsLayout {}

    impl LayoutManagerImpl for CardsLayout {
        fn measure(
            &self,
            _widget: &gtk::Widget,
            _orientation: gtk::Orientation,
            _for_size: i32,
        ) -> (i32, i32, i32, i32) {
            // Takes what it gets: the cards shrink to fit.
            (0, 0, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, _baseline: i32) {
            let sizes = self.sizes.borrow();
            let rects = crate::overview::arrange(
                &sizes,
                f64::from(width),
                f64::from((height - TITLE_HEIGHT).max(0)),
                GAP,
            );
            let mut child = widget.first_child();
            let mut rects = rects.iter();
            while let Some(c) = child {
                child = c.next_sibling();
                let Some(r) = rects.next() else {
                    c.set_child_visible(false);
                    continue;
                };
                c.set_child_visible(true);
                let (min_w, _, _, _) = c.measure(gtk::Orientation::Horizontal, -1);
                let w = (super::px(r.w)).max(min_w);
                let (min_h, _, _, _) = c.measure(gtk::Orientation::Vertical, w);
                let h = (super::px(r.h) + TITLE_HEIGHT).max(min_h);
                c.size_allocate(
                    &gtk::Allocation::new(super::px(r.x), super::px(r.y), w, h),
                    -1,
                );
            }
            if let Some(f) = self.allocated.borrow().as_ref() {
                f();
            }
        }
    }
}

glib::wrapper! {
    pub struct CardsLayout(ObjectSubclass<imp::CardsLayout>)
        @extends gtk::LayoutManager;
}

impl Default for CardsLayout {
    fn default() -> Self {
        glib::Object::new()
    }
}

/// What the middle of the overview shows while nothing is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Windows,
    /// The app grid (the dash's grid button, Super+A).
    Apps,
}

#[derive(Debug)]
pub struct OverviewSurface {
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    shell: Rc<RefCell<Shell>>,
    strip: gtk::Box,
    cards: gtk::Box,
    layout: CardsLayout,
    empty: gtk::Label,
    stash: gtk::Box,
    dash: gtk::Box,
    grid_button: gtk::ToggleButton,
    entry: gtk::SearchEntry,
    pages: gtk::Stack,
    results: gtk::Box,
    grid: gtk::Box,
    mode: Cell<Mode>,
    shown: RefCell<Option<OverviewView>>,
    /// The monitor's connector, as Hyprland names it.
    monitor: String,
    /// Each card's window and the area its window is drawn into by koje's
    /// plugin (ADR 0051).
    previews: RefCell<Vec<(String, gtk::Button)>>,
    /// The plugin was told to fly the windows in since it was shown.
    opened: Cell<bool>,
    /// The places it was told last.
    sent: RefCell<String>,
    shown_search: RefCell<Option<SearchView>>,
    shown_grid: RefCell<Option<Vec<AppSection>>>,
}

/// Close the overview on every monitor.
fn hide_all(widget: &impl IsA<gtk::Widget>) {
    let _ = widget.activate_action("app.hide-overview", None);
}

impl OverviewSurface {
    pub fn new(
        app: &adw::Application,
        monitor: &gdk::Monitor,
        actions: UnboundedSender<Action>,
        shell: Rc<RefCell<Shell>>,
    ) -> Rc<Self> {
        let window = layer_window(app, monitor);

        let strip = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::Center)
            .css_classes(["overview-strip"])
            .build();
        let layout = CardsLayout::default();
        let cards = gtk::Box::builder()
            .hexpand(true)
            .vexpand(true)
            .margin_start(48)
            .margin_end(48)
            .margin_top(24)
            .margin_bottom(16)
            .build();
        cards.set_layout_manager(Some(layout.clone()));
        let empty = gtk::Label::builder()
            .label("No windows")
            .css_classes(["overview-empty"])
            .vexpand(true)
            .visible(false)
            .build();
        let stash = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::Center)
            .css_classes(["overview-stash"])
            .visible(false)
            .build();
        let dash = gtk::Box::builder()
            .spacing(4)
            .halign(gtk::Align::Center)
            .css_classes(["dash"])
            .visible(false)
            .build();
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Type to search")
            .halign(gtk::Align::Center)
            .css_classes(["overview-search"])
            .build();
        // Typing anywhere searches, as in GNOME.
        entry.set_key_capture_widget(Some(&window));
        let windows_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        windows_page.append(&cards);
        windows_page.append(&empty);
        windows_page.append(&stash);
        let (pages, results, grid) = pages(&windows_page);
        let grid_icon = gtk::Image::from_icon_name("view-app-grid-symbolic");
        grid_icon.set_pixel_size(32);
        let grid_button = gtk::ToggleButton::builder()
            .child(&grid_icon)
            .width_request(60)
            .css_classes(["dash-app", "dash-grid"])
            .tooltip_text("Show Apps")
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&entry);
        content.append(&strip);
        content.append(&pages);
        content.append(&dash);
        window.set_child(Some(&content));

        close_on_escape_and_click(&window, &content);

        let me = Rc::new(Self {
            window,
            actions,
            shell,
            strip,
            cards,
            layout,
            empty,
            stash,
            dash,
            grid_button,
            entry,
            pages,
            results,
            grid,
            mode: Cell::new(Mode::Windows),
            shown: RefCell::new(None),
            shown_search: RefCell::new(None),
            shown_grid: RefCell::new(None),
            monitor: monitor.connector().map(String::from).unwrap_or_default(),
            previews: RefCell::default(),
            opened: Cell::new(false),
            sent: RefCell::default(),
        });
        me.connect_controls();
        // Where the cards are, the plugin draws the windows – after each
        // layout, once GTK is done with it.
        let weak = Rc::downgrade(&me);
        me.layout.set_allocated(move || {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(me) = weak.upgrade() {
                    me.send_places();
                }
            });
        });
        me
    }

    /// The search entry and the grid button.
    fn connect_controls(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_search_changed(move |_| {
            if let Some(me) = weak.upgrade() {
                me.update_page();
            }
        });
        // Escape: first the search, then the overview.
        let weak = Rc::downgrade(self);
        self.entry.connect_stop_search(move |e| {
            if e.text().is_empty() {
                hide_all(e);
            } else if let Some(me) = weak.upgrade() {
                e.set_text("");
                me.update_page();
            }
        });
        // Enter: the first result.
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |e| {
            if let Some(me) = weak.upgrade() {
                me.activate_first();
                hide_all(e);
            }
        });
        let weak = Rc::downgrade(self);
        self.grid_button.connect_clicked(move |b| {
            if let Some(me) = weak.upgrade() {
                me.set_mode(if b.is_active() {
                    Mode::Apps
                } else {
                    Mode::Windows
                });
            }
        });
    }

    /// Windows or the app grid; a search shows its results over either.
    pub fn set_mode(&self, mode: Mode) {
        self.mode.set(mode);
        self.grid_button.set_active(mode == Mode::Apps);
        self.update_page();
    }

    pub fn mode(&self) -> Mode {
        self.mode.get()
    }

    /// The page for the search and the mode, its content current.
    fn update_page(&self) {
        let query = self.entry.text();
        // The search and the app grid cover the plugin's windows.
        let covered = !query.trim().is_empty() || self.mode.get() == Mode::Apps;
        if covered {
            self.window.add_css_class("covered");
        } else {
            self.window.remove_css_class("covered");
        }
        if !query.trim().is_empty() {
            let view = self.shell.borrow().search(&query);
            if self.shown_search.borrow().as_ref() != Some(&view) {
                self.render_search(&view);
                self.shown_search.replace(Some(view));
            }
            self.pages.set_visible_child_name("search");
            return;
        }
        match self.mode.get() {
            Mode::Windows => self.pages.set_visible_child_name("windows"),
            Mode::Apps => {
                let grid = self.shell.borrow().app_grid();
                if self.shown_grid.borrow().as_ref() != Some(&grid) {
                    self.render_grid(&grid);
                    self.shown_grid.replace(Some(grid));
                }
                self.pages.set_visible_child_name("apps");
            }
        }
    }

    /// Enter in the search: start the first app, else show the first window.
    fn activate_first(&self) {
        let Some(view) = self.shown_search.borrow().clone() else {
            return;
        };
        if let Some(hit) = view.apps.first() {
            self.launch_app(&hit.app, hit.context.as_deref())();
        } else if let Some(hit) = view.windows.first() {
            self.show_window(hit)();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Show it; the focused monitor's takes the keyboard.
    pub fn show(self: &Rc<Self>, focused: bool) {
        self.window.set_keyboard_mode(if focused {
            KeyboardMode::Exclusive
        } else {
            KeyboardMode::None
        });
        // The plugin flies the windows in; no card shows before they landed
        // (`landed`).
        if !self.window.is_visible() {
            self.window.add_css_class("arriving");
            let window = self.window.clone();
            glib::timeout_add_local_once(ARRIVAL, move || window.remove_css_class("arriving"));
        }
        self.window.present();
        // Shown again unchanged, GTK may not lay the cards out again: tell
        // the plugin here too, once it is mapped.
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(me) = weak.upgrade() {
                me.send_places();
            }
        });
    }

    /// koje's plugin flew the windows in: the cards show up.
    pub fn landed(&self) {
        self.window.remove_css_class("arriving");
    }

    /// Hidden, it starts over next time: no search, the windows.
    pub fn hide(&self) {
        self.window.set_visible(false);
        self.entry.set_text("");
        self.set_mode(Mode::Windows);
        // The plugin flies the windows back.
        if self.opened.replace(false) {
            self.sent.replace(String::new());
            let _ = self
                .actions
                .send(Action::Overview(format!("close {}", self.monitor)));
        }
    }

    pub fn close(&self) {
        self.window.destroy();
    }

    /// Tell koje's plugin where the cards are, when that changed: the first
    /// time since shown it flies the windows in.
    fn send_places(&self) {
        let live = self.shown.borrow().as_ref().is_some_and(|v| v.live);
        if !live || !self.window.is_visible() {
            return;
        }
        let places: Vec<Place> = self
            .previews
            .borrow()
            .iter()
            .filter_map(|(address, preview)| {
                let b = preview.compute_bounds(&self.window)?;
                (b.width() >= 1.0 && b.height() >= 1.0).then(|| Place {
                    address: address.clone(),
                    x: px(f64::from(b.x())),
                    y: px(f64::from(b.y())),
                    w: px(f64::from(b.width())),
                    h: px(f64::from(b.height())),
                })
            })
            .collect();
        let Some((command, key)) = plugin_send(
            self.opened.get(),
            &self.sent.borrow(),
            &self.monitor,
            &places,
        ) else {
            return;
        };
        self.opened.set(true);
        self.sent.replace(key);
        let _ = self.actions.send(Action::Overview(command));
    }

    pub fn render(&self, view: &OverviewView) {
        if self.shown.borrow().as_ref() == Some(view) {
            return;
        }
        self.shown.replace(Some(view.clone()));
        clear(&self.strip);
        for w in &view.workspaces {
            self.strip
                .append(&self.strip_button(view, StripTarget::Workspace(w)));
        }
        // The overlays after a gap, as in the bar.
        if !view.overlays.is_empty() {
            self.strip
                .append(&gtk::Box::builder().css_classes(["overlay-gap"]).build());
        }
        for o in &view.overlays {
            self.strip
                .append(&self.strip_button(view, StripTarget::Overlay(o)));
        }
        clear(&self.cards);
        self.previews.borrow_mut().clear();
        // With the plugin the windows show through: no background.
        if view.live {
            self.window.add_css_class("live");
        } else {
            self.window.remove_css_class("live");
        }
        self.layout.replace_sizes(
            view.windows
                .iter()
                .map(|c| (f64::from(c.size.0), f64::from(c.size.1)))
                .collect(),
        );
        for card in &view.windows {
            self.cards.append(&self.card(card, view.live));
        }
        self.cards.set_visible(!view.windows.is_empty());
        self.empty.set_visible(view.windows.is_empty());
        // Hidden, the cards get no layout and nothing would tell the plugin
        // that this workspace has no windows (M8.79).
        if view.windows.is_empty() {
            self.send_places();
        }
        clear(&self.stash);
        match &view.stash {
            Some(stash) if !stash.windows.is_empty() => {
                self.stash.append(&gtk::Label::new(Some("Stash")));
                for card in &stash.windows {
                    self.stash.append(&self.stashed(&stash.context, card));
                }
                self.stash.set_visible(true);
            }
            _ => self.stash.set_visible(false),
        }
        clear(&self.dash);
        let mut favorites = true;
        for entry in &view.dash {
            // GNOME's line between the favorites and the other apps.
            if favorites && !entry.favorite && self.dash.first_child().is_some() {
                self.dash
                    .append(&gtk::Separator::new(gtk::Orientation::Vertical));
            }
            favorites = entry.favorite;
            self.dash.append(&self.dash_app(entry, &view.dash));
        }
        if !view.dash.is_empty() {
            self.dash
                .append(&gtk::Separator::new(gtk::Orientation::Vertical));
            self.dash.append(&self.grid_button);
        }
        self.dash.set_visible(!view.dash.is_empty());
        self.update_page();
    }

    fn send(&self, action: Action) -> impl Fn() + 'static {
        let actions = self.actions.clone();
        move || {
            let _ = actions.send(action.clone());
        }
    }

    /// A workspace or an overlay in the strip, with its icon and label as
    /// in the bar (ADR 0055): switching keeps the overview open – the strip
    /// has no thumbnails, so it shows the other one's windows. A window's
    /// card dropped on it moves the window there.
    fn strip_button(&self, view: &OverviewView, target: StripTarget<'_>) -> gtk::Button {
        let (icon, label) = match target {
            StripTarget::Workspace(w) => (&w.icon, &w.label),
            StripTarget::Overlay(o) => (&o.icon, &o.label),
        };
        let b = gtk::Button::builder()
            .child(&crate::bar::icon_label(icon, label))
            .tooltip_text(label)
            .build();
        match target {
            StripTarget::Workspace(w) => {
                b.add_css_class(&w.accent.css_class());
                if matches!(w.accent, Accent::Context(_)) {
                    b.add_css_class("context");
                }
                // Under an open overlay the overview shows the overlay.
                if w.active && view.overlay.is_none() {
                    b.add_css_class("active");
                }
                if w.empty {
                    b.add_css_class("empty");
                }
            }
            StripTarget::Overlay(o) => {
                b.add_css_class(&Accent::Host.css_class());
                if o.open {
                    b.add_css_class("active");
                }
            }
        }
        let clicks = strip_click(view, target);
        let actions = self.actions.clone();
        b.connect_clicked(move |_| {
            for a in &clicks {
                let _ = actions.send(a.clone());
            }
        });
        // Each card's moves, decided now: the view does not live on.
        let drops: Rc<Vec<(String, Vec<Action>)>> = Rc::new(
            view.windows
                .iter()
                .map(|c| (c.address.clone(), drop_window(view, c, target)))
                .collect(),
        );
        let moves = move |address: Option<String>| -> Vec<Action> {
            address
                .and_then(|a| drops.iter().find(|(d, _)| *d == a))
                .map(|(_, m)| m.clone())
                .unwrap_or_default()
        };
        let dragged = |t: &gtk::DropTarget| t.value().and_then(|v| v.get::<String>().ok());
        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        drop.set_preload(true);
        let accept = {
            let moves = moves.clone();
            move |t: &gtk::DropTarget| {
                if moves(dragged(t)).is_empty() {
                    gdk::DragAction::empty()
                } else {
                    gdk::DragAction::MOVE
                }
            }
        };
        let on_enter = accept.clone();
        drop.connect_enter(move |t, _, _| on_enter(t));
        drop.connect_motion(move |t, _, _| accept(t));
        let actions = self.actions.clone();
        drop.connect_drop(move |_, value, _, _| {
            let moves = moves(value.get::<String>().ok());
            for a in &moves {
                let _ = actions.send(a.clone());
            }
            !moves.is_empty()
        });
        b.add_controller(drop);
        b
    }

    /// A window: koje's plugin draws it into its preview, the app's icon
    /// on the bottom edge as in GNOME – without the plugin the icon in the
    /// middle; a close button on hover, its title below. A click focuses it.
    fn card(&self, card: &WindowCard, live: bool) -> gtk::Box {
        let icon = gtk::Image::from_gicon(&crate::app_info::icon(&card.app));
        icon.set_pixel_size(if live { 48 } else { 64 });
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(if live {
            gtk::Align::End
        } else {
            gtk::Align::Center
        });
        if live {
            icon.set_margin_bottom(10);
        }
        let preview = gtk::Button::builder()
            .child(&icon)
            .css_classes(["window-preview", &card.accent.css_class()])
            .vexpand(true)
            .tooltip_text(&card.title)
            .build();
        self.previews
            .borrow_mut()
            .push((card.address.clone(), preview.clone()));
        // Dragged onto the strip it moves to that workspace or overlay; the
        // app's icon goes along.
        let source = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .content(&gdk::ContentProvider::for_value(&card.address.to_value()))
            .build();
        let app = card.app.clone();
        source.connect_drag_begin(move |source, _| {
            let Some(display) = gdk::Display::default() else {
                return;
            };
            let paintable = gtk::IconTheme::for_display(&display).lookup_by_gicon(
                &crate::app_info::icon(&app),
                48,
                1,
                gtk::TextDirection::None,
                gtk::IconLookupFlags::empty(),
            );
            source.set_icon(Some(&paintable), 24, 24);
        });
        preview.add_controller(source);
        let send = self.send(Action::FocusWindow(card.address.clone()));
        preview.connect_clicked(move |b| {
            send();
            hide_all(b);
        });
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .css_classes(["circular", "window-close"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .tooltip_text("Close")
            .build();
        let send = self.send(Action::CloseWindow(card.address.clone()));
        close.connect_clicked(move |_| send());
        let overlay = gtk::Overlay::builder()
            .child(&preview)
            .vexpand(true)
            .build();
        overlay.add_overlay(&close);
        let title = gtk::Label::builder()
            .label(&card.title)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .width_chars(1)
            .halign(gtk::Align::Center)
            .css_classes(["window-title"])
            .build();
        let b = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["window-card"])
            .build();
        b.append(&overlay);
        b.append(&title);
        b
    }

    /// A stashed window: a click brings it back into the active layout.
    fn stashed(&self, context: &str, card: &WindowCard) -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let icon = gtk::Image::from_gicon(&crate::app_info::icon(&card.app));
        icon.set_pixel_size(20);
        row.append(&icon);
        row.append(
            &gtk::Label::builder()
                .label(&card.title)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(24)
                .build(),
        );
        let b = gtk::Button::builder()
            .child(&row)
            .tooltip_text(format!("Bring back: {}", card.title))
            .build();
        let unstash = self.send(Action::Unstash {
            context: context.to_owned(),
            address: card.address.clone(),
        });
        let focus = self.send(Action::FocusWindow(card.address.clone()));
        b.connect_clicked(move |b| {
            unstash();
            focus();
            hide_all(b);
        });
        b
    }
}

impl OverviewSurface {
    /// An app in the dash: its icon, a mark in its context's color, a dot
    /// while it runs. A click shows its most recent window or starts it; a
    /// right click opens its menu.
    fn dash_app(&self, entry: &DashEntry, dash: &[DashEntry]) -> gtk::Widget {
        let icon = gtk::Image::from_gicon(&dash_icon(entry));
        icon.set_pixel_size(48);
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .build();
        column.append(&icon);
        let dot = gtk::Box::builder()
            .css_classes(["running-dot"])
            .halign(gtk::Align::Center)
            .build();
        // Keeps its room while not running, so icons do not jump.
        dot.set_opacity(if entry.windows.is_empty() { 0.0 } else { 1.0 });
        column.append(&dot);
        let button = gtk::Button::builder()
            .child(&column)
            .css_classes(["dash-app", &entry.accent.css_class()])
            .tooltip_text(dash_tooltip(entry))
            .build();
        if entry.context.is_some() {
            button.add_css_class("koje-ctx");
        }
        let open = self.open_app(entry);
        button.connect_clicked(move |b| {
            open();
            hide_all(b);
        });
        let menu = self.dash_menu(entry, dash, &button);
        let right = gtk::GestureClick::builder().button(3).build();
        right.connect_pressed(move |_, _, _, _| menu.popup());
        button.add_controller(right);
        button.upcast()
    }

    /// Show the app's most recent window – one in view before a stashed
    /// one, which comes back first – or start it.
    fn open_app(&self, entry: &DashEntry) -> impl Fn() + use<> {
        let actions = self.actions.clone();
        let entry = entry.clone();
        let launch = self.launch(&entry);
        move || {
            let Some(w) = entry.windows.first() else {
                launch();
                return;
            };
            if let (true, Some(context)) = (w.stashed, &entry.context) {
                let _ = actions.send(Action::Unstash {
                    context: context.clone(),
                    address: w.address.clone(),
                });
            }
            let _ = actions.send(Action::FocusWindow(w.address.clone()));
        }
    }

    /// Start the app in its context, or on the host.
    fn launch(&self, entry: &DashEntry) -> impl Fn() + use<> {
        let actions = self.actions.clone();
        let (context, id) = (entry.context.clone(), entry.id.clone());
        let app = entry.app.clone();
        move || match (&context, &id) {
            (Some(context), Some(id)) => {
                let _ = actions.send(Action::Launch {
                    context: context.clone(),
                    id: id.clone(),
                });
            }
            (None, Some(id)) => launch_on_host(id),
            (_, None) => tracing::info!("{app}: no desktop file to start it by"),
        }
    }

    /// Right click: the context, New Window, Quit, the favorites.
    fn dash_menu(
        &self,
        entry: &DashEntry,
        dash: &[DashEntry],
        parent: &gtk::Button,
    ) -> gtk::Popover {
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let popover = gtk::Popover::builder().child(&list).has_arrow(true).build();
        popover.set_parent(parent);
        let title = dash_tooltip(entry);
        list.append(
            &gtk::Label::builder()
                .label(&title)
                .css_classes(["heading"])
                .margin_bottom(6)
                .build(),
        );
        let item = |label: &str, run: Box<dyn Fn()>| {
            let b = gtk::Button::builder()
                .label(label)
                .css_classes(["flat"])
                .build();
            b.connect_clicked(move |b| {
                run();
                hide_all(b);
            });
            list.append(&b);
        };
        if entry.id.is_some() {
            item("New Window", Box::new(self.launch(entry)));
        }
        // GNOME's favorites on the host, the context's own in a context
        // (ADR 0053) – from then on its list, not GNOME's.
        if let Some(id) = entry.id.clone() {
            let pin = !entry.favorite;
            let label = if pin { "Pin to Dash" } else { "Unpin" };
            match &entry.context {
                None => item(
                    label,
                    Box::new(move || crate::app_info::set_favorite(&id, pin)),
                ),
                Some(context) => {
                    let action = Action::SetFavorites {
                        context: context.clone(),
                        ids: crate::dash::pinned(dash, &id, pin),
                    };
                    let actions = self.actions.clone();
                    item(
                        label,
                        Box::new(move || {
                            let _ = actions.send(action.clone());
                        }),
                    );
                }
            }
        }
        if !entry.windows.is_empty() {
            let actions = self.actions.clone();
            let windows: Vec<String> = entry.windows.iter().map(|w| w.address.clone()).collect();
            item(
                "Quit",
                Box::new(move || {
                    for w in &windows {
                        let _ = actions.send(Action::CloseWindow(w.clone()));
                    }
                }),
            );
        }
        popover
    }
}

impl OverviewSurface {
    fn render_search(&self, view: &SearchView) {
        clear(&self.results);
        if view.apps.is_empty() && view.windows.is_empty() {
            self.results.append(
                &gtk::Label::builder()
                    .label("No results")
                    .css_classes(["overview-empty"])
                    .margin_top(48)
                    .build(),
            );
            return;
        }
        if !view.apps.is_empty() {
            let flow = tiles();
            for hit in &view.apps {
                flow.append(&self.app_tile(hit));
            }
            self.results.append(&flow);
        }
        if !view.windows.is_empty() {
            self.results.append(
                &gtk::Label::builder()
                    .label("Windows")
                    .halign(gtk::Align::Start)
                    .css_classes(["results-heading"])
                    .build(),
            );
            let list = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .build();
            for hit in &view.windows {
                list.append(&self.window_hit(hit));
            }
            self.results.append(&list);
        }
    }

    fn render_grid(&self, sections: &[AppSection]) {
        clear(&self.grid);
        if sections.is_empty() {
            self.grid.append(
                &gtk::Label::builder()
                    .label("No apps")
                    .css_classes(["overview-empty"])
                    .margin_top(48)
                    .build(),
            );
        }
        for section in sections {
            let title = gtk::Label::builder()
                .label(section.context.as_deref().unwrap_or("Host"))
                .halign(gtk::Align::Start)
                .css_classes(["grid-section", &section.accent.css_class()])
                .build();
            if section.context.is_some() {
                title.add_css_class("koje-ctx");
            }
            self.grid.append(&title);
            let flow = tiles();
            for app in &section.apps {
                flow.append(&self.app_tile(&AppHit {
                    app: app.clone(),
                    context: section.context.clone(),
                    accent: section.accent.clone(),
                }));
            }
            self.grid.append(&flow);
        }
    }

    /// An app in the grid or the search: its icon, its name, a mark in its
    /// context's color. A click starts it there.
    fn app_tile(&self, hit: &AppHit) -> gtk::Button {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        let icon = gtk::Image::from_gicon(&item_icon(&hit.app));
        icon.set_pixel_size(64);
        column.append(&icon);
        column.append(
            &gtk::Label::builder()
                .label(&hit.app.name)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(14)
                .width_chars(14)
                .justify(gtk::Justification::Center)
                .build(),
        );
        let mark = gtk::Box::builder()
            .css_classes(["context-mark"])
            .halign(gtk::Align::Center)
            .build();
        mark.set_opacity(if hit.context.is_some() { 1.0 } else { 0.0 });
        column.append(&mark);
        let b = gtk::Button::builder()
            .child(&column)
            .css_classes(["app-tile", &hit.accent.css_class()])
            .tooltip_text(match &hit.context {
                Some(c) => format!("{} – {c}", hit.app.name),
                None => hit.app.name.clone(),
            })
            .build();
        let launch = self.launch_app(&hit.app, hit.context.as_deref());
        b.connect_clicked(move |b| {
            launch();
            hide_all(b);
        });
        b
    }

    fn window_hit(&self, hit: &WindowHit) -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = gtk::Image::from_gicon(&crate::app_info::icon(&hit.app));
        icon.set_pixel_size(32);
        row.append(&icon);
        row.append(
            &gtk::Label::builder()
                .label(&hit.title)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(60)
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        if let Some(c) = &hit.context {
            row.append(
                &gtk::Label::builder()
                    .label(c)
                    .css_classes(["context-pill", &hit.accent.css_class()])
                    .valign(gtk::Align::Center)
                    .build(),
            );
        }
        let b = gtk::Button::builder()
            .child(&row)
            .css_classes(["window-hit"])
            .build();
        let show = self.show_window(hit);
        b.connect_clicked(move |b| {
            show();
            hide_all(b);
        });
        b
    }

    /// Start an app in its context, or on the host.
    fn launch_app(&self, app: &AppItem, context: Option<&str>) -> impl Fn() + use<> {
        let actions = self.actions.clone();
        let (id, context) = (app.id.clone(), context.map(str::to_owned));
        move || match &context {
            Some(context) => {
                let _ = actions.send(Action::Launch {
                    context: context.clone(),
                    id: id.clone(),
                });
            }
            None => launch_on_host(&id),
        }
    }

    /// A window from the search: a stashed one comes back first.
    fn show_window(&self, hit: &WindowHit) -> impl Fn() + use<> {
        let actions = self.actions.clone();
        let hit = hit.clone();
        move || {
            if let (true, Some(context)) = (hit.stashed, &hit.context) {
                let _ = actions.send(Action::Unstash {
                    context: context.clone(),
                    address: hit.address.clone(),
                });
            }
            let _ = actions.send(Action::FocusWindow(hit.address.clone()));
        }
    }
}

/// The middle of the overview: the windows, the search results, the app
/// grid – the last two scrolled.
fn pages(windows: &gtk::Box) -> (gtk::Stack, gtk::Box, gtk::Box) {
    let results = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .margin_top(16)
        .build();
    let grid = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(96)
        .margin_end(96)
        .margin_bottom(24)
        .build();
    let scrolled = |child: &gtk::Box| {
        gtk::ScrolledWindow::builder()
            .child(child)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build()
    };
    let pages = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .vexpand(true)
        .build();
    pages.add_named(windows, Some("windows"));
    pages.add_named(&scrolled(&results), Some("search"));
    pages.add_named(&scrolled(&grid), Some("apps"));
    (pages, results, grid)
}

/// A layer-shell surface over the whole monitor below the bar.
fn layer_window(app: &adw::Application, monitor: &gdk::Monitor) -> gtk::Window {
    let window = gtk::Window::builder()
        .application(app)
        .title("koje overview")
        .build();
    window.add_css_class("koje-overview");
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_namespace(Some("koje-overview"));
    window.set_monitor(Some(monitor));
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    // Below the bar, as GNOME's.
    window.set_exclusive_zone(0);
    window
}

/// A click on empty space or Escape closes the overview.
fn close_on_escape_and_click(window: &gtk::Window, content: &gtk::Box) {
    let click = gtk::GestureClick::new();
    click.connect_released(|g, _, _, _| {
        if let Some(w) = g.widget() {
            hide_all(&w);
        }
    });
    content.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(|k, key, _, _| {
        if key == gdk::Key::Escape {
            if let Some(w) = k.widget() {
                hide_all(&w);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

/// A flow of app tiles, as GNOME's grid.
fn tiles() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .homogeneous(true)
        .max_children_per_line(8)
        .min_children_per_line(3)
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(12)
        .row_spacing(12)
        .build()
}

/// An app's icon: a file in its context, a theme icon the host has, else
/// what the host knows for its desktop file id.
fn item_icon(app: &AppItem) -> gtk4::gio::Icon {
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    let usable = |i: &str| {
        if i.starts_with('/') {
            std::path::Path::new(i).is_file()
        } else {
            theme.as_ref().is_some_and(|t| t.has_icon(i))
        }
    };
    app.icon
        .as_deref()
        .filter(|i| usable(i))
        .and_then(|i| gtk4::gio::Icon::for_string(i).ok())
        .unwrap_or_else(|| crate::app_info::icon(&app.id))
}

/// A dash entry's icon: the context's program's, else the host's.
fn dash_icon(entry: &DashEntry) -> gtk4::gio::Icon {
    entry
        .item
        .as_ref()
        .map_or_else(|| crate::app_info::icon(&entry.app), item_icon)
}

/// The app's name and, for a context's, the context.
fn dash_tooltip(entry: &DashEntry) -> String {
    let name = entry
        .item
        .as_ref()
        .map_or_else(|| crate::app_info::name(&entry.app), |i| i.name.clone());
    match &entry.context {
        Some(c) => format!("{name} – {c}"),
        None => name,
    }
}

/// Start a host program by its desktop file id.
fn launch_on_host(id: &str) {
    let Some(info) = gio_unix::DesktopAppInfo::new(id) else {
        tracing::warn!("no desktop file {id}");
        return;
    };
    let context = gdk::Display::default().map(|d| d.app_launch_context());
    if let Err(e) = info.launch(&[], context.as_ref()) {
        tracing::warn!("starting {id}: {e}");
    }
}

impl CardsLayout {
    fn set_allocated(&self, f: impl Fn() + 'static) {
        use gtk4::subclass::prelude::ObjectSubclassIsExt as _;
        self.imp().allocated.replace(Some(Box::new(f)));
    }

    fn replace_sizes(&self, sizes: Vec<(f64, f64)>) {
        use gtk4::subclass::prelude::ObjectSubclassIsExt as _;
        self.imp().sizes.replace(sizes);
        self.layout_changed();
    }
}

/// A layout value in whole pixels.
#[allow(clippy::cast_possible_truncation)]
fn px(v: f64) -> i32 {
    v.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}
