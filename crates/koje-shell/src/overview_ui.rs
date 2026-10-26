//! The overview of one monitor (ADR 0049): a layer-shell surface over the
//! area below the bar with the workspace strip on top, the current
//! workspace's windows as cards in the middle, the context's stash and the
//! dash below. It renders an [`OverviewView`]; the shell's `hide-overview`
//! action closes it on every monitor.

use std::cell::RefCell;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::dash::DashEntry;
use crate::model::{Action, WorkspaceButton};
use crate::overview::{OverviewView, WindowCard};

/// Room between the cards.
const GAP: f64 = 32.0;
/// Below each card: its title.
const TITLE_HEIGHT: i32 = 32;

pub const CSS: &str = "
window.koje-overview { background: #1d1d20; color: #ffffff; }
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
.window-preview {
  border-radius: 12px; padding: 0;
  background: alpha(var(--accent-bg-color), 0.22);
  border: 3px solid transparent; box-shadow: 0 2px 8px alpha(black, 0.4);
}
.window-preview:hover { border-color: alpha(#ffffff, 0.6); }
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
";

mod imp {
    use std::cell::RefCell;

    use super::{GAP, TITLE_HEIGHT, glib, gtk};
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    /// Places the cards with [`crate::overview::arrange`].
    #[derive(Debug, Default)]
    pub struct CardsLayout {
        /// Each card's window size, in child order.
        pub sizes: RefCell<Vec<(f64, f64)>>,
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

#[derive(Debug)]
pub struct OverviewSurface {
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    strip: gtk::Box,
    cards: gtk::Box,
    layout: CardsLayout,
    empty: gtk::Label,
    stash: gtk::Box,
    dash: gtk::Box,
    shown: RefCell<Option<OverviewView>>,
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
    ) -> Self {
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
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&strip);
        content.append(&cards);
        content.append(&empty);
        content.append(&stash);
        content.append(&dash);
        window.set_child(Some(&content));

        // A click on empty space or Escape closes it.
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

        Self {
            window,
            actions,
            strip,
            cards,
            layout,
            empty,
            stash,
            dash,
            shown: RefCell::new(None),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Show it; the focused monitor's takes the keyboard.
    pub fn show(&self, focused: bool) {
        self.window.set_keyboard_mode(if focused {
            KeyboardMode::Exclusive
        } else {
            KeyboardMode::None
        });
        self.window.present();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn close(&self) {
        self.window.destroy();
    }

    pub fn render(&self, view: &OverviewView) {
        if self.shown.borrow().as_ref() == Some(view) {
            return;
        }
        clear(&self.strip);
        for w in &view.workspaces {
            self.strip.append(&self.workspace(w));
        }
        clear(&self.cards);
        self.layout.replace_sizes(
            view.windows
                .iter()
                .map(|c| (f64::from(c.size.0), f64::from(c.size.1)))
                .collect(),
        );
        for card in &view.windows {
            self.cards.append(&self.card(card));
        }
        self.cards.set_visible(!view.windows.is_empty());
        self.empty.set_visible(view.windows.is_empty());
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
            self.dash.append(&self.dash_app(entry));
        }
        self.dash.set_visible(!view.dash.is_empty());
        self.shown.replace(Some(view.clone()));
    }

    fn send(&self, action: Action) -> impl Fn() + 'static {
        let actions = self.actions.clone();
        move || {
            let _ = actions.send(action.clone());
        }
    }

    /// A workspace in the strip: switching keeps the overview open – the
    /// strip has no thumbnails, so it shows the other workspace's windows.
    fn workspace(&self, w: &WorkspaceButton) -> gtk::Button {
        let b = gtk::Button::builder().label(&w.name).build();
        b.add_css_class(&w.accent.css_class());
        if matches!(w.accent, crate::model::Accent::Context(_)) {
            b.add_css_class("context");
        }
        if w.active {
            b.add_css_class("active");
        }
        if w.empty {
            b.add_css_class("empty");
        }
        let send = self.send(if w.empty {
            Action::FocusContext(w.name.clone())
        } else {
            Action::FocusWorkspace(w.name.clone())
        });
        b.connect_clicked(move |_| send());
        b
    }

    /// A window: its preview (the app's icon until thumbnails, M8.67),
    /// a close button on hover, its title below. A click focuses it.
    fn card(&self, card: &WindowCard) -> gtk::Box {
        let icon = gtk::Image::from_gicon(&crate::app_info::icon(&card.app));
        icon.set_pixel_size(64);
        let preview = gtk::Button::builder()
            .child(&icon)
            .css_classes(["window-preview", &card.accent.css_class()])
            .vexpand(true)
            .tooltip_text(&card.title)
            .build();
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
    fn dash_app(&self, entry: &DashEntry) -> gtk::Widget {
        let icon = gtk::Image::from_gicon(&crate::app_info::icon(&entry.app));
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
        let menu = self.dash_menu(entry, &button);
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
    fn dash_menu(&self, entry: &DashEntry, parent: &gtk::Button) -> gtk::Popover {
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let popover = gtk::Popover::builder().child(&list).has_arrow(true).build();
        popover.set_parent(parent);
        let title = match &entry.context {
            Some(c) => format!("{} – {c}", crate::app_info::name(&entry.app)),
            None => crate::app_info::name(&entry.app),
        };
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
        if let Some(id) = entry.id.clone() {
            let favorite = entry.favorite;
            item(
                if favorite { "Unpin" } else { "Pin to Dash" },
                Box::new(move || crate::app_info::set_favorite(&id, !favorite)),
            );
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

/// The app's name and, for a context's, the context.
fn dash_tooltip(entry: &DashEntry) -> String {
    let name = crate::app_info::name(&entry.app);
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
