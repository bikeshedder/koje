//! Notifications (M8.36) as GNOME Shell 50 handles them
//! (`notificationDaemon.js`, `messageTray.js`, `messageList.js`), without
//! GTK: what a `Notify` call becomes, the list the date menu shows, the
//! banners waiting their turn, and the texts – markup as GNOME allows it.
//! The list groups them by app as Android does (M8.99, ADR 0058).

use std::collections::VecDeque;

/// `org.freedesktop.Notifications` urgency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_hint(v: u8) -> Self {
        match v {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// Why a notification went (the spec's `NotificationClosed` reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    Expired = 1,
    Dismissed = 2,
    /// `CloseNotification`.
    App = 3,
}

/// A `Notify` call, its hints read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Incoming {
    pub app_name: String,
    pub replaces: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    /// The flat `[key, label, key, label, …]` list.
    pub actions: Vec<String>,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    /// `desktop-entry`, without `.desktop`.
    pub desktop_entry: Option<String>,
    /// `image-path` (or the old `image_path`).
    pub image_path: Option<String>,
    /// The running context it came from, if any.
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "the spec's hints")]
pub struct Notification {
    pub id: u32,
    /// The app's name for the header: the desktop file's when known.
    pub app_name: String,
    /// A themed icon name or a path for the header (16 px).
    pub app_icon: Option<String>,
    /// The picture in the body (48 px): `image-path`, else the app icon
    /// when it is a file (GNOME shows the sender's icon only in the header).
    pub image: Option<String>,
    pub title: String,
    /// Pango markup, made safe as GNOME does ([`fix_markup`]).
    pub body: String,
    /// Buttons, at most three, without `default`.
    pub actions: Vec<(String, String)>,
    /// The app asked for a `default` action: a click invokes it.
    pub default_action: bool,
    pub urgency: Urgency,
    pub transient: bool,
    pub resident: bool,
    pub desktop_entry: Option<String>,
    pub context: Option<String>,
    /// Seconds since the epoch.
    pub time: i64,
    /// Shown as a banner or in the opened date menu (GNOME's
    /// `acknowledged`): the clock's dot is for unseen ones.
    pub seen: bool,
}

/// GNOME's `MAX_NOTIFICATION_BUTTONS`.
const MAX_BUTTONS: usize = 3;

/// The notifications there are, newest first, and the banners still to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tray {
    pub list: Vec<Notification>,
    queue: VecDeque<u32>,
    next_id: u32,
    /// "Do Not Disturb": no banners but critical ones (`show-banners`).
    pub dnd: bool,
}

impl Tray {
    /// A `Notify` call: its id (the replaced one's when that exists).
    /// Low urgency shows no banner, as in GNOME; nor does anything but a
    /// critical one under Do Not Disturb.
    pub fn notify(&mut self, n: Incoming, app_name: Option<String>, now: i64) -> u32 {
        let id = if n.replaces != 0 && self.list.iter().any(|x| x.id == n.replaces) {
            n.replaces
        } else {
            self.next_id += 1;
            self.next_id
        };
        let (actions, default_action) = actions(&n.actions);
        let icon_is_file = n.app_icon.starts_with('/') || n.app_icon.starts_with("file://");
        let notification = Notification {
            id,
            app_name: app_name
                .filter(|a| !a.is_empty())
                .or_else(|| Some(n.app_name.clone()).filter(|a| !a.is_empty()))
                .unwrap_or_else(|| "Unknown App".to_owned()),
            app_icon: Some(n.app_icon.clone()).filter(|i| !i.is_empty() && !icon_is_file),
            image: n.image_path.clone().or_else(|| {
                icon_is_file.then(|| n.app_icon.trim_start_matches("file://").to_owned())
            }),
            title: n.summary.replace('\n', " "),
            body: fix_markup(&n.body.replace('\n', " ")),
            actions,
            default_action,
            urgency: n.urgency,
            transient: n.transient,
            resident: n.resident,
            desktop_entry: n.desktop_entry,
            context: n.context,
            time: now,
            seen: false,
        };
        self.list.retain(|x| x.id != id);
        self.list.insert(0, notification);
        let banner = match n.urgency {
            Urgency::Low => false,
            Urgency::Normal => !self.dnd,
            Urgency::Critical => true,
        };
        self.queue.retain(|q| *q != id);
        if banner {
            // Critical ones first, as GNOME sorts its queue.
            if n.urgency == Urgency::Critical {
                self.queue.push_front(id);
            } else {
                self.queue.push_back(id);
            }
        }
        id
    }

    /// The next banner to show, taken from the queue.
    pub fn next_banner(&mut self) -> Option<Notification> {
        while let Some(id) = self.queue.pop_front() {
            if let Some(n) = self.list.iter_mut().find(|n| n.id == id) {
                n.seen = true;
                return Some(n.clone());
            }
        }
        None
    }

    /// The date menu was opened: everything in it is seen.
    pub fn see_all(&mut self) {
        for n in &mut self.list {
            n.seen = true;
        }
    }

    /// GNOME's messages indicator: notifications neither shown as a banner
    /// nor seen in the date menu, not counting banners still to come; none
    /// under Do Not Disturb.
    pub fn unseen(&self) -> bool {
        !self.dnd
            && self
                .list
                .iter()
                .any(|n| !n.seen && !self.queue.contains(&n.id))
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.list.iter().find(|n| n.id == id)
    }

    /// Gone from the list: whether it was there.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|n| n.id != id);
        self.queue.retain(|q| *q != id);
        self.list.len() != before
    }

    /// A banner went away by itself: transient ones are gone then (GNOME
    /// destroys them as expired); others stay in the list.
    pub fn banner_done(&mut self, id: u32) -> Option<Closed> {
        let transient = self.get(id).is_some_and(|n| n.transient);
        (transient && self.remove(id)).then_some(Closed::Expired)
    }

    /// The notification was clicked or one of its buttons: unless it is
    /// resident it goes (GNOME destroys it as dismissed).
    pub fn activated(&mut self, id: u32) -> Option<Closed> {
        let resident = self.get(id).is_some_and(|n| n.resident);
        (!resident && self.remove(id)).then_some(Closed::Dismissed)
    }

    /// "Clear": every notification but resident ones; their ids.
    pub fn clear(&mut self) -> Vec<u32> {
        let gone: Vec<u32> = self
            .list
            .iter()
            .filter(|n| !n.resident)
            .map(|n| n.id)
            .collect();
        for id in &gone {
            self.remove(*id);
        }
        gone
    }
}

/// The buttons from the flat action list, and whether there is a `default`
/// one; GNOME shows at most three.
pub fn actions(flat: &[String]) -> (Vec<(String, String)>, bool) {
    let mut out = Vec::new();
    let mut default = false;
    for pair in flat.as_chunks::<2>().0 {
        if pair[0] == "default" {
            default = true;
        } else if out.len() < MAX_BUTTONS {
            out.push((pair[0].clone(), pair[1].clone()));
        }
    }
    (out, default)
}

/// GNOME's `fixMarkup` for bodies: `&amp;`, `&quot;`, `&apos;`, `&lt;`,
/// `&gt;` stay, every other `&` is escaped; `<b>`, `<i>`, `<u>` stay, every
/// other `<` is escaped. Unbalanced tags would make Pango fail: then all of
/// it is escaped.
pub fn fix_markup(text: &str) -> String {
    const ENTITIES: [&str; 5] = ["&amp;", "&quot;", "&apos;", "&lt;", "&gt;"];
    const TAGS: [&str; 6] = ["<b>", "</b>", "<i>", "</i>", "<u>", "</u>"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let kept = match c {
            '&' => ENTITIES.iter().find(|e| rest.starts_with(**e)),
            '<' => TAGS.iter().find(|t| rest.starts_with(**t)),
            _ => None,
        };
        if let Some(k) = kept {
            out.push_str(k);
            rest = &rest[k.len()..];
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            c => out.push(c),
        }
        rest = &rest[c.len_utf8()..];
    }
    if balanced(&out) { out } else { escape(text) }
}

fn balanced(markup: &str) -> bool {
    let mut open: Vec<char> = Vec::new();
    let mut rest = markup;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        let close = rest.starts_with("</");
        let Some(tag) = rest.chars().nth(if close { 2 } else { 1 }) else {
            return false;
        };
        if close {
            if open.pop() != Some(tag) {
                return false;
            }
        } else {
            open.push(tag);
        }
        rest = &rest[1..];
    }
    open.is_empty()
}

/// `GLib.markup_escape_text`.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// How long ago, as short as Android words it: `now`, `5m`, `3h`, `2d`,
/// `3w`, `4mo`, `2y`.
pub fn short_span(now: i64, then: i64) -> String {
    let seconds = (now - then).max(0);
    let days = seconds / 86_400;
    if seconds < 60 {
        "now".to_owned()
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if days < 1 {
        format!("{}h", seconds / 3600)
    } else if days < 14 {
        format!("{days}d")
    } else if days < 60 {
        format!("{}w", days / 7)
    } else if days < 365 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
}

/// One app's notifications in one context, as Android groups them: one
/// card in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<'a> {
    /// Stable while the group is there: what its expanded state is kept by.
    pub key: String,
    pub app_name: String,
    /// The newest one's app icon.
    pub app_icon: Option<String>,
    pub context: Option<String>,
    /// Newest first.
    pub items: Vec<&'a Notification>,
}

impl Group<'_> {
    /// Shown expanded unless the user says otherwise: when one of them is
    /// critical (GNOME expands those by themselves).
    pub fn expanded_by_default(&self) -> bool {
        self.items.iter().any(|n| n.urgency == Urgency::Critical)
    }

    /// Collapsed, the card shows the newest in full, the next ones as a
    /// line each, then how many more there are: `(lines, more)`.
    pub fn collapsed(&self) -> (usize, usize) {
        let rest = self.items.len().saturating_sub(1);
        let lines = rest.min(COLLAPSED_LINES);
        (lines, rest - lines)
    }

    pub fn ids(&self) -> Vec<u32> {
        self.items.iter().map(|n| n.id).collect()
    }
}

/// Older notifications a collapsed group shows as a line each.
const COLLAPSED_LINES: usize = 2;

/// Notifications grouped by app and context, the group whose newest
/// notification is newest first.
pub fn groups(list: &[Notification]) -> Vec<Group<'_>> {
    let mut out: Vec<Group<'_>> = Vec::new();
    for n in list {
        let key = format!(
            "{}\u{1f}{}",
            n.app_name,
            n.context.as_deref().unwrap_or_default()
        );
        match out.iter_mut().find(|g| g.key == key) {
            Some(g) => g.items.push(n),
            None => out.push(Group {
                key,
                app_name: n.app_name.clone(),
                app_icon: n.app_icon.clone(),
                context: n.context.clone(),
                items: vec![n],
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(summary: &str) -> Incoming {
        Incoming {
            app_name: "Firefox".into(),
            summary: summary.into(),
            ..Incoming::default()
        }
    }

    #[test]
    fn replacing_keeps_the_id_and_moves_it_up() {
        let mut t = Tray::default();
        let a = t.notify(incoming("a"), None, 0);
        let b = t.notify(incoming("b"), None, 0);
        let a2 = t.notify(
            Incoming {
                replaces: a,
                ..incoming("a again")
            },
            None,
            1,
        );
        assert_eq!(a2, a);
        assert_eq!(t.list.iter().map(|n| n.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(t.list[0].title, "a again");
        // An unknown id to replace: a new one.
        assert_eq!(
            t.notify(
                Incoming {
                    replaces: 99,
                    ..incoming("c")
                },
                None,
                2
            ),
            3
        );
    }

    #[test]
    fn banners_by_urgency_and_do_not_disturb() {
        let mut t = Tray::default();
        t.notify(
            Incoming {
                urgency: Urgency::Low,
                ..incoming("low")
            },
            None,
            0,
        );
        let normal = t.notify(incoming("normal"), None, 0);
        let critical = t.notify(
            Incoming {
                urgency: Urgency::Critical,
                ..incoming("!")
            },
            None,
            0,
        );
        assert_eq!(t.next_banner().map(|n| n.id), Some(critical));
        assert_eq!(t.next_banner().map(|n| n.id), Some(normal));
        assert_eq!(t.next_banner(), None);
        assert_eq!(t.list.len(), 3);
        t.dnd = true;
        t.notify(incoming("quiet"), None, 0);
        assert_eq!(t.next_banner(), None);
    }

    #[test]
    fn the_clocks_dot_for_unseen_ones() {
        let mut t = Tray::default();
        t.notify(incoming("shown"), None, 0);
        assert!(!t.unseen(), "a banner still to come does not count");
        t.next_banner();
        assert!(!t.unseen());
        t.notify(
            Incoming {
                urgency: Urgency::Low,
                ..incoming("quiet")
            },
            None,
            0,
        );
        assert!(t.unseen());
        t.see_all();
        assert!(!t.unseen());
    }

    #[test]
    fn what_stays_in_the_list() {
        let mut t = Tray::default();
        let transient = t.notify(
            Incoming {
                transient: true,
                ..incoming("t")
            },
            None,
            0,
        );
        let resident = t.notify(
            Incoming {
                resident: true,
                ..incoming("r")
            },
            None,
            0,
        );
        let plain = t.notify(incoming("p"), None, 0);
        assert_eq!(t.banner_done(transient), Some(Closed::Expired));
        assert_eq!(t.banner_done(plain), None);
        assert_eq!(t.activated(resident), None);
        assert_eq!(t.clear(), [plain]);
        assert_eq!(t.list.len(), 1);
        assert_eq!(t.activated(resident), None);
    }

    #[test]
    fn actions_as_gnome_shows_them() {
        let flat: Vec<String> = ["default", "Open", "a", "A", "b", "B", "c", "C", "d", "D"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let (buttons, default) = actions(&flat);
        assert!(default);
        assert_eq!(buttons.len(), 3);
        assert_eq!(buttons[0], ("a".to_owned(), "A".to_owned()));
    }

    #[test]
    fn markup_as_gnome_allows_it() {
        assert_eq!(fix_markup("<b>bold</b> & more"), "<b>bold</b> &amp; more");
        assert_eq!(fix_markup("a &lt; b"), "a &lt; b");
        assert_eq!(
            fix_markup("<a href=\"x\">link</a>"),
            "&lt;a href=\"x\">link&lt;/a>"
        );
        assert_eq!(fix_markup("<img src=x>"), "&lt;img src=x>");
        // Unbalanced: all escaped.
        assert_eq!(fix_markup("<b>open"), "&lt;b&gt;open");
    }

    #[test]
    fn times_as_short_as_android_words_them() {
        let m = 60;
        let h = 3600;
        let d = 86_400;
        assert_eq!(short_span(59, 0), "now");
        assert_eq!(short_span(5 * m, 0), "5m");
        assert_eq!(short_span(h, 0), "1h");
        assert_eq!(short_span(d + h, 0), "1d");
        assert_eq!(short_span(20 * d, 0), "2w");
        assert_eq!(short_span(100 * d, 0), "3mo");
        assert_eq!(short_span(800 * d, 0), "2y");
        assert_eq!(short_span(0, 10), "now", "a clock gone back");
    }

    #[test]
    fn groups_by_app_and_context_newest_first() {
        let mut t = Tray::default();
        t.notify(incoming("1"), None, 0);
        t.notify(
            Incoming {
                app_name: "Mail".into(),
                ..incoming("2")
            },
            None,
            0,
        );
        t.notify(incoming("3"), None, 0);
        t.notify(
            Incoming {
                context: Some("work".into()),
                ..incoming("4")
            },
            None,
            0,
        );
        let g = groups(&t.list);
        let shape: Vec<_> = g
            .iter()
            .map(|g| (g.app_name.as_str(), g.context.as_deref(), g.items.len()))
            .collect();
        assert_eq!(
            shape,
            [
                ("Firefox", Some("work"), 1),
                ("Firefox", None, 2),
                ("Mail", None, 1)
            ]
        );
        assert_eq!(g[1].items[0].title, "3");
        assert_ne!(g[0].key, g[1].key);
    }

    #[test]
    fn a_collapsed_group_shows_the_newest_two_lines_and_a_count() {
        let mut t = Tray::default();
        for i in 0..5 {
            t.notify(incoming(&i.to_string()), None, 0);
        }
        let all = groups(&t.list);
        assert_eq!(all[0].collapsed(), (2, 2));
        let one = Group {
            items: vec![&t.list[0]],
            ..all[0].clone()
        };
        assert_eq!(one.collapsed(), (0, 0));
        let two = Group {
            items: vec![&t.list[0], &t.list[1]],
            ..all[0].clone()
        };
        assert_eq!(two.collapsed(), (1, 0));
        assert!(!all[0].expanded_by_default());
        t.notify(
            Incoming {
                urgency: Urgency::Critical,
                ..incoming("!")
            },
            None,
            0,
        );
        assert!(groups(&t.list)[0].expanded_by_default());
    }

    #[test]
    fn names_and_pictures() {
        let mut t = Tray::default();
        let id = t.notify(
            Incoming {
                app_name: String::new(),
                app_icon: "/tmp/avatar.png".into(),
                ..incoming("x")
            },
            None,
            0,
        );
        let n = t.get(id).unwrap();
        assert_eq!(n.app_name, "Unknown App");
        assert_eq!(
            (n.app_icon.clone(), n.image.as_deref()),
            (None, Some("/tmp/avatar.png"))
        );
        let id = t.notify(incoming("y"), Some("Firefox Web Browser".into()), 0);
        assert_eq!(t.get(id).unwrap().app_name, "Firefox Web Browser");
    }
}
