//! The clock in the middle of the bar (M8.30), as GNOME's: it follows
//! `org.gnome.desktop.interface`'s `clock-format`, `clock-show-date`,
//! `clock-show-weekday` and `clock-show-seconds`. A click opens GNOME
//! Shell 50's date menu (M8.81, `dateMenu.js`, `calendar.js`): the
//! notifications on the left – a card per app, as Android groups them
//! (M8.99, ADR 0058) – today and the calendar on the right; a dot beside the
//! clock while there are unseen notifications.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use tokio::sync::mpsc::UnboundedSender;

use crate::model::{Action, DateView};
use crate::notifications::groups;
use crate::notifications_ui::{Place, follow_style, group_card};
use crate::notify_server::Request;

/// GNOME's date menu (`_calendar.scss`, `_message-list.scss`): the list
/// 29em wide with a hairline on its right; koje's cards 6 px apart; the
/// placeholder's 96 px icon; today's date as a heading.
pub const CSS: &str = "
.date-menu .message-list { min-width: 29em; border-right: 1px solid alpha(currentColor, 0.1); margin-right: 4px; padding-right: 6px; }
.date-menu .message-view .message { margin-bottom: 6px; }
.date-menu .message-view { margin-right: 12px; }
.date-menu .message-list-placeholder { color: alpha(currentColor, 0.45); font-size: 15pt; font-weight: 700; }
.date-menu .message-list-placeholder image { -gtk-icon-size: 96px; margin-bottom: 12px; }
.date-menu .message-list-controls { padding: 12px 12px 9px 12px; font-weight: bold; }
.date-menu .message-list-clear-button { border-radius: 999px; }
.date-menu .datemenu-calendar-column { margin-left: 6px; }
.date-menu .datemenu-today-button { padding: 9px; border-radius: 12px; }
.date-menu .day-label { font-weight: bold; }
.date-menu .date-label { font-size: 15pt; font-weight: 800; }
.messages-indicator { min-width: 5px; min-height: 5px; border-radius: 999px; background: currentColor; margin: 0 4px; }
.messages-indicator-pad { min-width: 13px; }
";

const SCHEMA: &str = "org.gnome.desktop.interface";

/// `clock-format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hours {
    H24,
    H12,
}

/// What GNOME's clock settings ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockOptions {
    pub hours: Hours,
    pub date: bool,
    pub weekday: bool,
    pub seconds: bool,
}

impl Default for ClockOptions {
    /// GNOME's defaults.
    fn default() -> Self {
        Self {
            hours: Hours::H24,
            date: false,
            weekday: true,
            seconds: false,
        }
    }
}

/// The `strftime` format of the clock, as GNOME's wall clock builds it:
/// day and time apart by an en space; [`ratio`] turns `:` into `∶` after.
pub fn clock_format(o: ClockOptions) -> String {
    let day = match (o.weekday, o.date) {
        (true, true) => "%a %b %-e",
        (false, true) => "%b %-e",
        (true, false) => "%a",
        (false, false) => "",
    };
    let time = match (o.hours, o.seconds) {
        (Hours::H24, false) => "%R",
        (Hours::H24, true) => "%R:%S",
        (Hours::H12, false) => "%-l:%M %p",
        (Hours::H12, true) => "%-l:%M:%S %p",
    };
    if day.is_empty() {
        time.to_owned()
    } else {
        format!("{day}\u{2002}{time}")
    }
}

/// GNOME shows times with the ratio sign, not a colon.
pub fn ratio(text: &str) -> String {
    text.replace(':', "\u{2236}")
}

/// Milliseconds until the clock next changes: the next second or minute.
pub fn next_tick(o: ClockOptions, second: u32, millis: u32) -> u32 {
    let to_second = 1000 - millis.min(999);
    if o.seconds {
        to_second
    } else {
        (59 - second.min(59)) * 1000 + to_second
    }
}

#[derive(Debug)]
pub struct Clock {
    button: gtk::MenuButton,
    actions: UnboundedSender<Action>,
    list: gtk::Box,
    scroller: gtk::ScrolledWindow,
    placeholder: gtk::Box,
    clear: gtk::Button,
    indicator: gtk::Box,
    pad: gtk::Box,
    shown: RefCell<Option<DateView>>,
    /// The groups the user expanded or collapsed, by key.
    expanded: Rc<RefCell<BTreeMap<String, bool>>>,
}

impl Clock {
    pub fn new(actions: &UnboundedSender<Action>) -> std::rc::Rc<Self> {
        let label = gtk::Label::builder().css_classes(["clock"]).build();
        // GNOME pads the left as wide as the dot: the time stays centered.
        let pad = gtk::Box::builder()
            .css_classes(["messages-indicator-pad"])
            .visible(false)
            .build();
        let indicator = gtk::Box::builder()
            .css_classes(["messages-indicator"])
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let face = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        face.append(&pad);
        face.append(&label);
        face.append(&indicator);

        let (message_list, list, scroller, placeholder, clear) = message_list(actions);
        let (column, calendar, day, date) = calendar_column();
        let content = gtk::Box::builder()
            .css_classes(["date-menu"])
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        content.append(&message_list);
        content.append(&column);
        follow_style(&content);
        let popover = gtk::Popover::builder().child(&content).build();
        crate::popup::keep_inside(&popover);
        let button = gtk::MenuButton::builder()
            .child(&face)
            .popover(&popover)
            .css_classes(["panel-button", "clock-button"])
            .build();
        let me = std::rc::Rc::new(Self {
            button,
            actions: actions.clone(),
            list,
            scroller,
            placeholder,
            clear,
            indicator,
            pad,
            shown: RefCell::new(None),
            expanded: Rc::default(),
        });
        // Always open on today, with fresh times; what is in it is seen.
        {
            let me = std::rc::Rc::downgrade(&me);
            popover.connect_show(move |_| {
                if let Ok(now) = glib::DateTime::now_local() {
                    calendar.select_day(&now);
                    if let Ok(text) = now.format("%A") {
                        day.set_label(&text);
                    }
                    if let Ok(text) = now.format("%B %-d %Y") {
                        date.set_label(&text);
                    }
                }
                if let Some(me) = me.upgrade() {
                    let _ = me.actions.send(Action::Notification(Request::Seen));
                    if let Some(view) = me.shown.borrow().clone() {
                        me.fill(&view);
                    }
                }
            });
        }

        let settings = settings();
        tick(&label, settings.as_ref());
        if let Some(s) = &settings {
            let label = label.clone();
            s.connect_changed(None, move |s, key| {
                if key.starts_with("clock-") {
                    show(&label, options(Some(s)));
                }
            });
        }
        me
    }

    pub fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }

    pub fn render(&self, view: &DateView) {
        if self.shown.borrow().as_ref() == Some(view) {
            return;
        }
        self.indicator.set_visible(view.unseen);
        self.pad.set_visible(view.unseen);
        self.fill(view);
        self.shown.replace(Some(view.clone()));
    }

    /// The list: a card per app and context, newest first.
    fn fill(&self, view: &DateView) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let now = glib::DateTime::now_local().map_or(0, |d| d.to_unix());
        let plain: Vec<_> = view.notifications.iter().map(|(n, _)| n.clone()).collect();
        let all = groups(&plain);
        self.expanded
            .borrow_mut()
            .retain(|key, _| all.iter().any(|g| g.key == *key));
        for group in all {
            let accent = view
                .notifications
                .iter()
                .find(|(x, _)| x.id == group.items[0].id)
                .and_then(|(_, a)| a.clone());
            let expanded = self
                .expanded
                .borrow()
                .get(&group.key)
                .copied()
                .unwrap_or_else(|| group.expanded_by_default());
            let on_toggle = {
                let (memory, key) = (self.expanded.clone(), group.key.clone());
                move |on| {
                    memory.borrow_mut().insert(key.clone(), on);
                }
            };
            self.list.append(&group_card(
                &group,
                now,
                Place::List,
                &self.actions,
                accent,
                expanded,
                on_toggle,
            ));
        }
        let empty = view.notifications.is_empty();
        self.placeholder.set_visible(empty);
        self.scroller.set_visible(!empty);
        self.clear.set_visible(!empty);
    }
}

/// The date menu's right side: today's weekday and date (a click shows
/// today in the calendar) above the calendar.
fn calendar_column() -> (gtk::Box, gtk::Calendar, gtk::Label, gtk::Label) {
    let day = gtk::Label::builder()
        .css_classes(["day-label"])
        .halign(gtk::Align::Start)
        .build();
    let date = gtk::Label::builder()
        .css_classes(["date-label"])
        .halign(gtk::Align::Start)
        .build();
    let today_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    today_box.append(&day);
    today_box.append(&date);
    let today = gtk::Button::builder()
        .child(&today_box)
        .css_classes(["flat", "datemenu-today-button"])
        .build();
    let calendar = gtk::Calendar::new();
    {
        let calendar = calendar.clone();
        today.connect_clicked(move |_| {
            if let Ok(now) = glib::DateTime::now_local() {
                calendar.select_day(&now);
            }
        });
    }
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["datemenu-calendar-column"])
        .build();
    column.append(&today);
    column.append(&calendar);

    (column, calendar, day, date)
}

/// The date menu's left side: the list, its placeholder, "Clear".
fn message_list(
    actions: &UnboundedSender<Action>,
) -> (
    gtk::Box,
    gtk::Box,
    gtk::ScrolledWindow,
    gtk::Box,
    gtk::Button,
) {
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["message-view"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(560)
        .vexpand(true)
        .build();
    let placeholder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["message-list-placeholder"])
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    placeholder.append(&gtk::Image::from_icon_name("no-notifications-symbolic"));
    placeholder.append(&gtk::Label::new(Some("No Notifications")));
    let clear = gtk::Button::builder()
        .label("Clear")
        .halign(gtk::Align::Start)
        .css_classes(["message-list-clear-button"])
        .build();
    {
        let actions = actions.clone();
        clear.connect_clicked(move |_| {
            let _ = actions.send(Action::Notification(Request::Clear));
        });
    }
    let controls = gtk::Box::builder()
        .css_classes(["message-list-controls"])
        .build();
    controls.append(&clear);
    let message_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["message-list"])
        .build();
    message_list.append(&placeholder);
    message_list.append(&scroller);
    message_list.append(&controls);

    (message_list, list, scroller, placeholder, clear)
}

/// GNOME's interface settings, if the schema is installed (it is with
/// GNOME Settings, ADR 0044); without it the defaults.
fn settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(SCHEMA, true)?;
    Some(gio::Settings::new(SCHEMA))
}

fn options(s: Option<&gio::Settings>) -> ClockOptions {
    let Some(s) = s else {
        return ClockOptions::default();
    };
    ClockOptions {
        hours: if s.string("clock-format") == "12h" {
            Hours::H12
        } else {
            Hours::H24
        },
        date: s.boolean("clock-show-date"),
        weekday: s.boolean("clock-show-weekday"),
        seconds: s.boolean("clock-show-seconds"),
    }
}

/// Show the time now; the time it was shown at.
fn show(label: &gtk::Label, o: ClockOptions) -> Option<glib::DateTime> {
    let now = glib::DateTime::now_local().ok()?;
    if let Ok(text) = now.format(&clock_format(o)) {
        label.set_label(&ratio(&text));
    }
    Some(now)
}

/// Show the time, again whenever it changes.
fn tick(label: &gtk::Label, settings: Option<&gio::Settings>) {
    let o = options(settings);
    let wait = show(label, o).map_or(1000, |now| {
        let millis = u32::try_from(now.microsecond() / 1000).unwrap_or(0);
        next_tick(o, u32::try_from(now.second()).unwrap_or(0), millis)
    });
    let (label, settings) = (label.clone(), settings.cloned());
    glib::timeout_add_local_once(
        std::time::Duration::from_millis(u64::from(wait)),
        move || tick(&label, settings.as_ref()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(hours: Hours, date: bool, weekday: bool, seconds: bool) -> ClockOptions {
        ClockOptions {
            hours,
            date,
            weekday,
            seconds,
        }
    }

    #[test]
    fn formats_like_gnome() {
        assert_eq!(clock_format(ClockOptions::default()), "%a\u{2002}%R");
        assert_eq!(
            clock_format(opts(Hours::H24, true, true, false)),
            "%a %b %-e\u{2002}%R"
        );
        assert_eq!(
            clock_format(opts(Hours::H24, true, false, true)),
            "%b %-e\u{2002}%R:%S"
        );
        assert_eq!(
            clock_format(opts(Hours::H12, false, false, false)),
            "%-l:%M %p"
        );
        assert_eq!(ratio("Mon 13:31"), "Mon 13\u{2236}31");
    }

    #[test]
    fn ticks_at_the_next_change() {
        let minutes = ClockOptions::default();
        assert_eq!(next_tick(minutes, 0, 0), 60_000);
        assert_eq!(next_tick(minutes, 59, 900), 100);
        assert_eq!(next_tick(opts(Hours::H24, false, true, true), 30, 250), 750);
    }
}
