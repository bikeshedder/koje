//! Notifications as cards (M8.36, M8.99, ADR 0058): one card per app and
//! context, grouped as Android groups them, compact – in a banner at the top
//! of the work area and in the date menu's list (M8.81). The colors are
//! GNOME's (`_message-list.scss`); the sizes are koje's; the context mark (a
//! stripe in the context's color) is koje's.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::Action;
use crate::notifications::{Group, Notification, Urgency, escape, groups, short_span};
use crate::notify_server::Request;

/// GNOME's `NOTIFICATION_TIMEOUT`, `ANIMATION_TIME`, `HIDE_TIMEOUT`.
const SHOWN: Duration = Duration::from_millis(4000);
const ANIMATION: Duration = Duration::from_millis(200);
const HIDE_AFTER_LEAVE: Duration = Duration::from_millis(200);

/// Body lines: collapsed in the list, in a banner, expanded.
const LINES_COLLAPSED: i32 = 1;
const LINES_BANNER: i32 = 2;
const LINES_EXPANDED: i32 = 6;

/// Light: `$card_bg_color` white, secondary text `#6b6b6e`; dark (in
/// `.dark`): `#535359`, secondary text `#b1b1b3`, as GNOME's messages.
pub const CSS: &str = "
window.koje-banner { background: transparent; }
.message {
  background: #ffffff; color: #222226;
  border-radius: 14px; padding: 8px 10px;
  border: 1px solid alpha(black, 0.09);
  box-shadow: 0 1px 2px 0 alpha(black, 0.03);
}
.dark .message { background: #535359; color: #ffffff; border-color: transparent; box-shadow: none; }
.message-header, .message-time, .message-line, .message-more { color: #6b6b6e; }
.dark .message-header, .dark .message-time, .dark .message-line, .dark .message-more { color: #b1b1b3; }
.message-header { font-size: 9pt; min-height: 22px; }
.message-source-icon { -gtk-icon-size: 16px; }
.message-source-title { font-weight: bold; }
.message-count {
  font-size: 8pt; font-weight: bold; padding: 0 6px; border-radius: 999px;
  background: alpha(currentColor, 0.12);
}
.message button.circular {
  background: none; box-shadow: none; border: none;
  min-height: 20px; min-width: 20px; padding: 0; border-radius: 999px;
}
.message button.circular:hover { background: alpha(currentColor, 0.15); }
.message button.circular image { -gtk-icon-size: 12px; }
.message-expand-button:checked { background: none; }
.message-expand-button:checked image { -gtk-icon-transform: rotate(180deg); }
.notification-item { padding-top: 4px; }
.notification-item + .notification-item { border-top: 1px solid alpha(currentColor, 0.08); margin-top: 4px; padding-top: 6px; }
.message-icon { -gtk-icon-size: 32px; }
.message-themed-icon {
  border-radius: 999px; background: alpha(currentColor, 0.07);
  -gtk-icon-size: 16px; min-width: 32px; min-height: 32px;
}
.message-title { font-weight: bold; }
.message-time { font-size: 8pt; }
.message-lines { padding-top: 2px; }
.message-line { font-size: 9.5pt; }
.message-more { font-size: 8pt; font-weight: bold; }
.notification-button {
  background: alpha(currentColor, 0.1); box-shadow: none; border: none;
  font-size: 9pt; font-weight: bold; padding: 3px 10px; border-radius: 8px;
  min-height: 0; min-width: 0;
}
.notification-button:hover { background: alpha(currentColor, 0.2); }
.message .context-mark { min-width: 3px; border-radius: 999px; margin: 2px 8px 2px 0; background: var(--accent-bg-color); }
.notification-banner {
  min-width: 26em; margin: 6px;
  box-shadow: 0 2px 4px 2px alpha(black, 0.05);
}
.dark .notification-banner { box-shadow: 0 2px 4px 2px alpha(black, 0.2); }
@keyframes banner-in {
  from { opacity: 0; transform: translateY(-48px) scale(0.9); }
  to { opacity: 1; transform: none; }
}
@keyframes banner-out {
  from { opacity: 1; transform: none; }
  to { opacity: 0; transform: translateY(-48px); }
}
.banner-in { animation: banner-in 200ms cubic-bezier(0.34, 1.56, 0.64, 1); }
.banner-out { animation: banner-out 200ms cubic-bezier(0.34, 1.56, 0.64, 1) forwards; }
";

/// Where a card is: a banner shows one notification and cannot expand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Banner,
    List,
}

fn send(actions: &UnboundedSender<Action>, request: Request) {
    let _ = actions.send(Action::Notification(request));
}

/// A click on the notification: its `default` action, or – as GNOME's
/// `source.open()` – its app, in its context.
fn activate(n: &Notification, actions: &UnboundedSender<Action>, display: &gtk::gdk::Display) {
    if !n.default_action
        && let Some(entry) = &n.desktop_entry
    {
        let id = format!("{entry}.desktop");
        match &n.context {
            Some(context) => {
                let _ = actions.send(Action::Launch {
                    context: context.clone(),
                    id,
                });
            }
            None => {
                if let Some(app) = gio_unix::DesktopAppInfo::new(&id)
                    && let Err(e) = app.launch(&[], Some(&display.app_launch_context()))
                {
                    tracing::warn!("opening {id}: {e}");
                }
            }
        }
    }
    send(
        actions,
        Request::Activate {
            id: n.id,
            action: None,
        },
    );
}

/// A file or a themed icon; a name the theme lacks gets GNOME's fallback.
fn icon(name: &str) -> gtk::Image {
    if name.starts_with('/') {
        return gtk::Image::from_file(name);
    }
    let known = gtk::gdk::Display::default()
        .is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon(name));
    gtk::Image::from_icon_name(if known {
        name
    } else {
        "application-x-executable-symbolic"
    })
}

fn small_button(icon: &str, class: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .css_classes(["circular", class])
        .valign(gtk::Align::Center)
        .build()
}

fn on_click(widget: &impl IsA<gtk::Widget>, f: impl Fn(&gtk::Widget) + 'static) {
    let click = gtk::GestureClick::new();
    click.connect_released(move |g, _, _, _| {
        if let Some(w) = g.widget() {
            f(&w);
        }
    });
    widget.add_controller(click);
}

/// The buttons, as wide as each other.
fn buttons(n: &Notification, actions: &UnboundedSender<Action>) -> gtk::Box {
    let bin = gtk::Box::builder()
        .spacing(6)
        .homogeneous(true)
        .margin_top(6)
        .css_classes(["message-action-bin"])
        .build();
    for (key, label) in &n.actions {
        let b = gtk::Button::builder()
            .label(label)
            .hexpand(true)
            .css_classes(["notification-button"])
            .build();
        let (actions, id, key) = (actions.clone(), n.id, key.clone());
        b.connect_clicked(move |_| {
            send(
                &actions,
                Request::Activate {
                    id,
                    action: Some(key.clone()),
                },
            );
        });
        bin.append(&b);
    }
    bin
}

/// One notification in a card: its picture, title and body (a click
/// activates it), its own time and close button (shown while its group is
/// expanded), its buttons.
struct Item {
    widget: gtk::Box,
    body: gtk::Label,
    own: [gtk::Widget; 2],
}

fn item(n: &Notification, now: i64, actions: &UnboundedSender<Action>) -> Item {
    let title = gtk::Label::builder()
        .label(&n.title)
        .xalign(0.0)
        .css_classes(["message-title"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .hexpand(true)
        .build();
    let time = gtk::Label::builder()
        .label(short_span(now, n.time))
        .css_classes(["message-time"])
        .valign(gtk::Align::Center)
        .build();
    let title_line = gtk::Box::builder().spacing(6).build();
    title_line.append(&title);
    title_line.append(&time);
    let body = gtk::Label::builder()
        .use_markup(true)
        .label(&n.body)
        .xalign(0.0)
        .css_classes(["message-body"])
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        // As wide as the card, not as the text: it ellipsizes.
        .max_width_chars(1)
        .hexpand(true)
        .visible(!n.body.is_empty())
        .build();
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(1)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text.append(&title_line);
    text.append(&body);
    let content = gtk::Box::builder().spacing(10).hexpand(true).build();
    let picture = if let Some(image) = &n.image {
        Some(icon(image))
    } else {
        n.app_icon
            .as_deref()
            .filter(|i| i.ends_with("-symbolic"))
            .map(|name| {
                let picture = icon(name);
                picture.add_css_class("message-themed-icon");
                picture
            })
    };
    if let Some(picture) = picture {
        picture.add_css_class("message-icon");
        picture.set_valign(gtk::Align::Start);
        content.append(&picture);
    }
    content.append(&text);
    {
        let (actions, n) = (actions.clone(), n.clone());
        on_click(&content, move |w| activate(&n, &actions, &w.display()));
    }
    let close = small_button("window-close-symbolic", "message-close-button");
    close.set_valign(gtk::Align::Start);
    {
        let (actions, id) = (actions.clone(), n.id);
        close.connect_clicked(move |_| send(&actions, Request::Dismiss(id)));
    }
    let row = gtk::Box::builder().spacing(6).build();
    row.append(&content);
    row.append(&close);
    let widget = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["notification-item"])
        .build();
    widget.append(&row);
    if !n.actions.is_empty() {
        widget.append(&buttons(n, actions));
    }
    Item {
        widget,
        body,
        own: [time.upcast(), close.upcast()],
    }
}

/// The header: app icon, app name, context, the newest one's time, how
/// many there are; the expand (in the list) and close buttons.
fn header(
    group: &Group<'_>,
    now: i64,
    place: Place,
    actions: &UnboundedSender<Action>,
) -> (gtk::Box, gtk::Box, Option<gtk::ToggleButton>) {
    let source = gtk::Box::builder().spacing(6).hexpand(true).build();
    let source_icon = icon(
        group
            .app_icon
            .as_deref()
            .unwrap_or("application-x-executable-symbolic"),
    );
    source_icon.add_css_class("message-source-icon");
    source.append(&source_icon);
    source.append(
        &gtk::Label::builder()
            .label(&group.app_name)
            .css_classes(["message-source-title"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    let mut details: Vec<String> = group.context.iter().cloned().collect();
    if let Some(newest) = group.items.first() {
        details.push(short_span(now, newest.time));
    }
    for d in details {
        source.append(&gtk::Label::new(Some("·")));
        source.append(&gtk::Label::new(Some(&d)));
    }
    let count = gtk::Box::builder().valign(gtk::Align::Center).build();
    if group.items.len() > 1 {
        count.append(
            &gtk::Label::builder()
                .label(group.items.len().to_string())
                .css_classes(["message-count"])
                .build(),
        );
        source.append(&count);
    }
    let header = gtk::Box::builder()
        .spacing(4)
        .css_classes(["message-header"])
        .build();
    header.append(&source);
    let expand = (place == Place::List).then(|| {
        let b = gtk::ToggleButton::builder()
            .icon_name("pan-down-symbolic")
            .css_classes(["circular", "message-expand-button"])
            .valign(gtk::Align::Center)
            .build();
        header.append(&b);
        b
    });
    let close = small_button("window-close-symbolic", "message-close-button");
    {
        let (actions, ids) = (actions.clone(), group.ids());
        close.connect_clicked(move |_| send(&actions, Request::DismissGroup(ids.clone())));
    }
    header.append(&close);
    if let Some(b) = &expand {
        let b = b.clone();
        on_click(&source, move |_| b.set_active(!b.is_active()));
    }
    (header, count, expand)
}

/// The older ones of a collapsed group, a line each, and how many more.
fn lines(group: &Group<'_>) -> gtk::Box {
    let (shown, more) = group.collapsed();
    let lines = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["message-lines"])
        .build();
    for n in group.items.iter().skip(1).take(shown) {
        lines.append(
            &gtk::Label::builder()
                .use_markup(true)
                .label(format!("<b>{}</b>  {}", escape(&n.title), n.body))
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(1)
                .hexpand(true)
                .css_classes(["message-line"])
                .build(),
        );
    }
    if more > 0 {
        lines.append(
            &gtk::Label::builder()
                .label(format!("+{more} more"))
                .xalign(0.0)
                .css_classes(["message-more"])
                .build(),
        );
    }
    lines.set_visible(shown > 0);
    lines
}

/// A group's card. Collapsed: the newest notification, the next ones as a
/// line each, the count; expanded: all of them, each with its time, close
/// button and longer body. `on_toggle` hears when the user expands or
/// collapses it.
pub fn group_card(
    group: &Group<'_>,
    now: i64,
    place: Place,
    actions: &UnboundedSender<Action>,
    accent_class: Option<String>,
    expanded: bool,
    on_toggle: impl Fn(bool) + 'static,
) -> gtk::Box {
    let (header, count, expand) = header(group, now, place, actions);
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .build();
    column.append(&header);
    let items: Vec<Item> = group.items.iter().map(|n| item(n, now, actions)).collect();
    for i in &items {
        column.append(&i.widget);
    }
    let lines = lines(group);
    column.append(&lines);
    let many = items.len() > 1;
    let collapsed_lines = if place == Place::Banner {
        LINES_BANNER
    } else {
        LINES_COLLAPSED
    };
    let apply = {
        let lines = lines.clone();
        let has_lines = lines.first_child().is_some();
        move |on: bool| {
            for (i, item) in items.iter().enumerate() {
                item.widget.set_visible(i == 0 || on);
                item.body
                    .set_lines(if on { LINES_EXPANDED } else { collapsed_lines });
                for w in &item.own {
                    w.set_visible(on && many);
                }
            }
            lines.set_visible(has_lines && !on);
            count.set_visible(!on);
        }
    };
    apply(expanded);
    if let Some(b) = &expand {
        b.set_active(expanded);
        b.connect_toggled(move |b| {
            apply(b.is_active());
            on_toggle(b.is_active());
        });
        let b = b.clone();
        on_click(&lines, move |_| b.set_active(true));
    }

    let card = gtk::Box::builder().css_classes(["message"]).build();
    // koje's mark: a stripe in the context's color.
    if let Some(class) = accent_class {
        card.append(
            &gtk::Box::builder()
                .css_classes(["context-mark", class.as_str()])
                .build(),
        );
    }
    card.append(&column);
    card
}

/// Follow the style: the cards are light or dark with it.
pub fn follow_style(widget: &impl IsA<gtk::Widget>) {
    let style = adw::StyleManager::default();
    let set = {
        let widget = widget.clone().upcast::<gtk::Widget>();
        move |s: &adw::StyleManager| {
            if s.is_dark() {
                widget.add_css_class("dark");
            } else {
                widget.remove_css_class("dark");
            }
        }
    };
    set(&style);
    style.connect_dark_notify(set);
}

/// The banner: one at a time at the top of the work area on the focused
/// monitor, 4 s (critical ones until dismissed), kept while the pointer is
/// on it.
#[derive(Debug)]
pub struct Banners {
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    shown: RefCell<Option<(u32, gtk::Box)>>,
    hovered: Rc<Cell<bool>>,
    timer: RefCell<Option<glib::SourceId>>,
    /// The time ran out while the pointer was on it.
    expired: Cell<bool>,
}

impl Banners {
    pub fn new(app: &adw::Application, actions: UnboundedSender<Action>) -> Rc<Self> {
        let window = gtk::Window::builder()
            .application(app)
            .title("koje notification")
            .css_classes(["koje-banner"])
            .build();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-notification"));
        window.set_keyboard_mode(KeyboardMode::None);
        // Below the bar: a zero exclusive zone keeps clear of others'.
        window.set_anchor(Edge::Top, true);
        window.set_exclusive_zone(0);
        follow_style(&window);
        let me = Rc::new(Self {
            window,
            actions,
            shown: RefCell::new(None),
            hovered: Rc::new(Cell::new(false)),
            timer: RefCell::new(None),
            expired: Cell::new(false),
        });
        let motion = gtk::EventControllerMotion::new();
        {
            let me = Rc::downgrade(&me);
            motion.connect_enter(move |_, _, _| {
                if let Some(me) = me.upgrade() {
                    me.hovered.set(true);
                }
            });
        }
        {
            let me = Rc::downgrade(&me);
            motion.connect_leave(move |_| {
                if let Some(me) = me.upgrade() {
                    me.hovered.set(false);
                    if me.expired.get() {
                        me.clone().after(HIDE_AFTER_LEAVE);
                    }
                }
            });
        }
        me.window.add_controller(motion);
        me
    }

    /// Show `n`, replacing what is shown.
    pub fn show(self: &Rc<Self>, n: &Notification, now: i64, accent_class: Option<String>) {
        let Some(group) = groups(std::slice::from_ref(n)).pop() else {
            return;
        };
        let expanded = group.expanded_by_default();
        let card = group_card(
            &group,
            now,
            Place::Banner,
            &self.actions,
            accent_class,
            expanded,
            |_| {},
        );
        card.add_css_class("notification-banner");
        card.add_css_class("banner-in");
        card.set_halign(gtk::Align::Center);
        self.window.set_child(Some(&card));
        self.window.set_visible(true);
        self.shown.replace(Some((n.id, card)));
        self.expired.set(false);
        if let Some(t) = self.timer.take() {
            t.remove();
        }
        if n.urgency != Urgency::Critical {
            self.clone().after(SHOWN);
        }
    }

    /// Hide when the time is up – later while the pointer is on it.
    fn after(self: Rc<Self>, wait: Duration) {
        if let Some(t) = self.timer.take() {
            t.remove();
        }
        let me = Rc::downgrade(&self);
        let id = glib::timeout_add_local_once(wait, move || {
            let Some(me) = me.upgrade() else { return };
            me.timer.take();
            if me.hovered.get() {
                me.expired.set(true);
            } else {
                me.hide(true);
            }
        });
        self.timer.replace(Some(id));
    }

    /// The shown banner goes when its notification is no longer there
    /// (dismissed in the list, activated, closed by its app).
    pub fn keep_only(&self, list: &[Notification]) {
        let gone = self
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|(id, _)| !list.iter().any(|n| n.id == *id));
        if gone {
            self.hide_now();
        }
    }

    fn hide_now(&self) {
        if let Some(t) = self.timer.take() {
            t.remove();
        }
        self.shown.take();
        self.window.set_child(None::<&gtk::Widget>);
        self.window.set_visible(false);
    }

    /// Slide it out; the server shows the next.
    fn hide(self: &Rc<Self>, animate: bool) {
        let Some((id, card)) = self.shown.borrow().clone() else {
            return;
        };
        let me = Rc::downgrade(self);
        let done = move || {
            if let Some(me) = me.upgrade() {
                if me.shown.borrow().as_ref().is_some_and(|(s, _)| *s == id) {
                    me.hide_now();
                }
                send(&me.actions, Request::BannerDone(id));
            }
        };
        if animate {
            card.remove_css_class("banner-in");
            card.add_css_class("banner-out");
            glib::timeout_add_local_once(ANIMATION, done);
        } else {
            done();
        }
    }
}

/// GNOME's notification settings: `show-banners` is "Do Not Disturb"
/// turned off.
pub fn notification_settings() -> Option<gio::Settings> {
    const SCHEMA: &str = "org.gnome.desktop.notifications";
    gio::SettingsSchemaSource::default()?.lookup(SCHEMA, true)?;
    Some(gio::Settings::new(SCHEMA))
}
