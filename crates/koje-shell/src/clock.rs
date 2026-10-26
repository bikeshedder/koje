//! The clock in the middle of the bar, with a calendar on click (M8.30), as
//! GNOME's: it follows `org.gnome.desktop.interface`'s `clock-format`,
//! `clock-show-date`, `clock-show-weekday` and `clock-show-seconds`.

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};

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
}

impl Clock {
    pub fn new() -> Self {
        let label = gtk::Label::builder().css_classes(["clock"]).build();
        let header = gtk::Label::builder()
            .css_classes(["title-4"])
            .halign(gtk::Align::Start)
            .build();
        let calendar = gtk::Calendar::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        content.append(&header);
        content.append(&calendar);
        let popover = gtk::Popover::builder().child(&content).build();
        // Always open on today.
        popover.connect_show(move |_| {
            if let Ok(now) = glib::DateTime::now_local() {
                calendar.select_day(&now);
                if let Ok(text) = now.format("%A, %B %-e %Y") {
                    header.set_label(&text);
                }
            }
        });
        let button = gtk::MenuButton::builder()
            .child(&label)
            .popover(&popover)
            .css_classes(["flat", "clock-button"])
            .build();

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
        Self { button }
    }

    pub fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }
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
