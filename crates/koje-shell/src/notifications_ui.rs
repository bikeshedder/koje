//! Notifications as GNOME Shell 50 draws them (M8.36): the message card
//! (`messageList.js`, `_message-list.scss`) in a banner at the top of the
//! work area (`messageTray.js`, `_notifications.scss`) and in the date
//! menu's list (M8.81). The sizes and colors below are GNOME's, computed from
//! its SCSS; the context mark (a stripe in the context's color) is koje's.

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
use crate::notifications::{Notification, Urgency, time_span};
use crate::notify_server::Request;

/// GNOME's `NOTIFICATION_TIMEOUT`, `ANIMATION_TIME`, `HIDE_TIMEOUT`.
const SHOWN: Duration = Duration::from_millis(4000);
const ANIMATION: Duration = Duration::from_millis(200);
const HIDE_AFTER_LEAVE: Duration = Duration::from_millis(200);

/// Light: `$card_bg_color` white, header text `#6b6b6e`; dark (in
/// `.dark`): the message `#535359` (`#36363a` lightened 7 %, then 5 %), header
/// text `#b1b1b3`. Buttons are the text color at 15 %, 30 % on hover.
pub const CSS: &str = "
window.koje-banner { background: transparent; }
.message {
  background: #ffffff; color: #222226;
  border-radius: 16px; padding: 6px;
  border: 1px solid alpha(black, 0.09);
  box-shadow: 0 1px 2px 0 alpha(black, 0.03);
}
.dark .message { background: #535359; color: #ffffff; border-color: transparent; box-shadow: none; }
.message-header { padding: 0 0 0 6px; color: #6b6b6e; }
.dark .message-header { color: #b1b1b3; }
.message-header-content { min-height: 24px; padding-bottom: 6px; }
.message-source-icon { -gtk-icon-size: 16px; }
.message-source-title { font-weight: bold; }
.message-header .event-time { font-size: 9pt; }
.message-header button, .notification-button {
  background: alpha(currentColor, 0.15); box-shadow: none; border: none;
  min-height: 0; min-width: 0;
}
.message-header button:hover, .notification-button:hover { background: alpha(currentColor, 0.3); }
.message-header button { border-radius: 999px; padding: 6px; }
.message-close-button { margin: 3px; }
.message-box { padding: 6px; }
.message-icon { margin-right: 6px; -gtk-icon-size: 48px; }
.message-themed-icon {
  border-radius: 999px; background: alpha(currentColor, 0.07);
  -gtk-icon-size: 16px; min-width: 48px; min-height: 48px;
}
.message-title { font-weight: bold; }
.message-action-bin { padding: 4px; }
.notification-button { font-weight: bold; padding: 6px 12px; border-radius: 8px; }
.message .context-mark { min-width: 4px; border-radius: 999px; margin: 6px 0 6px 0; background: var(--accent-bg-color); }
.notification-banner {
  min-height: 64px; min-width: 34em; margin: 4px;
  box-shadow: 0 2px 4px 2px alpha(black, 0.05);
}
.dark .notification-banner { box-shadow: 0 2px 4px 2px alpha(black, 0.2); }
@keyframes banner-in {
  from { opacity: 0; transform: translateY(-64px) scale(0.9); }
  to { opacity: 1; transform: none; }
}
@keyframes banner-out {
  from { opacity: 1; transform: none; }
  to { opacity: 0; transform: translateY(-64px); }
}
.banner-in { animation: banner-in 200ms cubic-bezier(0.34, 1.56, 0.64, 1); }
.banner-out { animation: banner-out 200ms cubic-bezier(0.34, 1.56, 0.64, 1) forwards; }
";

/// Where a card is: the banner hides the expand button and stays one line.
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

/// GNOME's message header: app icon, app name, time, then the expand
/// (in the list) and close buttons; the expand button wraps `body`.
fn header(
    n: &Notification,
    now: i64,
    place: Place,
    actions: &UnboundedSender<Action>,
    body: &gtk::Label,
) -> gtk::Box {
    let source_icon = icon(
        n.app_icon
            .as_deref()
            .unwrap_or("application-x-executable-symbolic"),
    );
    source_icon.add_css_class("message-source-icon");
    let source_title = gtk::Label::builder()
        .label(&n.app_name)
        .css_classes(["message-source-title"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let time = gtk::Label::builder()
        .label(time_span(now, n.time))
        .css_classes(["event-time"])
        .hexpand(true)
        .xalign(1.0)
        .build();
    let header_content = gtk::Box::builder()
        .spacing(6)
        .hexpand(true)
        .css_classes(["message-header-content"])
        .build();
    header_content.append(&source_title);
    header_content.append(&time);
    let header = gtk::Box::builder()
        .spacing(6)
        .css_classes(["message-header"])
        .build();
    header.append(&source_icon);
    header.append(&header_content);
    let set_expanded = {
        let body = body.clone();
        move |on: bool| {
            body.set_wrap(on);
            body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            body.set_lines(if on { 6 } else { 1 });
            body.set_ellipsize(gtk::pango::EllipsizeMode::End);
        }
    };
    // GNOME expands critical ones by itself.
    set_expanded(n.urgency == Urgency::Critical);
    if place == Place::List {
        let expand = gtk::ToggleButton::builder()
            .icon_name("pan-down-symbolic")
            .css_classes(["message-expand-button"])
            .valign(gtk::Align::Center)
            .build();
        expand.connect_toggled(move |b| set_expanded(b.is_active()));
        header.append(&expand);
    }
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .css_classes(["message-close-button"])
        .valign(gtk::Align::Center)
        .build();
    {
        let (actions, id) = (actions.clone(), n.id);
        close.connect_clicked(move |_| send(&actions, Request::Dismiss(id)));
    }
    header.append(&close);
    header
}

/// The buttons, as wide as each other.
fn buttons(n: &Notification, actions: &UnboundedSender<Action>) -> gtk::Box {
    let bin = gtk::Box::builder()
        .spacing(6)
        .homogeneous(true)
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

/// GNOME's message: header, the picture, title and body, the buttons.
pub fn message(
    n: &Notification,
    now: i64,
    place: Place,
    actions: &UnboundedSender<Action>,
    accent_class: Option<String>,
) -> gtk::Box {
    let body = gtk::Label::builder()
        .use_markup(true)
        .label(&n.body)
        .xalign(0.0)
        .css_classes(["message-body"])
        // As wide as the card, not as the text: it ellipsizes.
        .max_width_chars(1)
        .hexpand(true)
        .build();
    let header = header(n, now, place, actions, &body);
    let title = gtk::Label::builder()
        .label(&n.title)
        .xalign(0.0)
        .css_classes(["message-title"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .hexpand(true)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .css_classes(["message-content"])
        .build();
    content.append(&title);
    if !n.body.is_empty() {
        content.append(&body);
    }
    let message_box = gtk::Box::builder()
        .spacing(6)
        .css_classes(["message-box"])
        .build();
    if let Some(image) = &n.image {
        let picture = icon(image);
        picture.add_css_class("message-icon");
        picture.set_valign(gtk::Align::Start);
        message_box.append(&picture);
    } else if let Some(name) = n.app_icon.as_deref().filter(|i| i.ends_with("-symbolic")) {
        let picture = icon(name);
        picture.add_css_class("message-icon");
        picture.add_css_class("message-themed-icon");
        picture.set_valign(gtk::Align::Start);
        message_box.append(&picture);
    }
    message_box.append(&content);

    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["message"])
        .build();
    card.append(&header);
    card.append(&message_box);
    if !n.actions.is_empty() {
        card.append(&buttons(n, actions));
    }
    // koje's mark: a stripe in the context's color.
    if let Some(class) = accent_class {
        let mark = gtk::Box::builder()
            .css_classes(["context-mark", class.as_str()])
            .build();
        message_box.prepend(&mark);
    }
    // A click on the card activates it.
    let click = gtk::GestureClick::new();
    {
        let (actions, n) = (actions.clone(), n.clone());
        click.connect_released(move |g, _, _, _| {
            if let Some(w) = g.widget() {
                activate(&n, &actions, &w.display());
            }
        });
    }
    message_box.add_controller(click);
    card
}

/// Follow the style: GNOME's notifications are light or dark with it.
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
        let card = message(n, now, Place::Banner, &self.actions, accent_class);
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
