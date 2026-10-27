//! Quick Settings' widgets (M8.30), laid out like GNOME's: indicator icons
//! in the bar; on click battery and buttons on top, sliders for volume and
//! brightness, toggles for Wi-Fi, dark style and the keyboard layout, and a
//! line with CPU and memory. It renders a [`QuickView`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::Action;
use crate::quick::{self, QuickView};
use crate::system::Change;

const INTERFACE: &str = "org.gnome.desktop.interface";
/// After the user moved a slider, readings do not move it back for a while.
const HANDS_OFF: Duration = Duration::from_millis(1500);

/// A slider whose value the user moves and the readings follow.
#[derive(Debug, Clone)]
struct Slider {
    row: gtk::Box,
    icon: gtk::Button,
    scale: gtk::Scale,
    /// Set while the value is set from a reading, not by the user.
    updating: Rc<Cell<bool>>,
    touched: Rc<Cell<Option<Instant>>>,
}

impl Slider {
    fn new(icon: &str, on_change: impl Fn(u32) + 'static) -> Self {
        let icon = gtk::Button::builder()
            .icon_name(icon)
            .css_classes(["flat", "circular"])
            .build();
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        let row = gtk::Box::builder().spacing(6).build();
        row.append(&icon);
        row.append(&scale);
        let updating: Rc<Cell<bool>> = Rc::default();
        let touched: Rc<Cell<Option<Instant>>> = Rc::default();
        {
            let (updating, touched) = (updating.clone(), touched.clone());
            scale.connect_value_changed(move |s| {
                if updating.get() {
                    return;
                }
                touched.set(Some(Instant::now()));
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "0–100"
                )]
                on_change(s.value().round().clamp(0.0, 100.0) as u32);
            });
        }
        Self {
            row,
            icon,
            scale,
            updating,
            touched,
        }
    }

    fn set(&self, value: u32) {
        if self.touched.get().is_some_and(|t| t.elapsed() < HANDS_OFF) {
            return;
        }
        self.updating.set(true);
        self.scale.set_value(f64::from(value));
        self.updating.set(false);
    }
}

/// A GNOME quick toggle: icon, title, subtitle; filled with the accent when on.
#[derive(Debug, Clone)]
struct Toggle {
    button: gtk::ToggleButton,
    icon: gtk::Image,
    title: gtk::Label,
    subtitle: gtk::Label,
    updating: Rc<Cell<bool>>,
}

impl Toggle {
    fn new(on_toggle: impl Fn(bool) + 'static) -> Self {
        let icon = gtk::Image::new();
        let title = gtk::Label::builder()
            .css_classes(["heading"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        let subtitle = gtk::Label::builder()
            .css_classes(["caption"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        text.append(&title);
        text.append(&subtitle);
        let content = gtk::Box::builder().spacing(8).build();
        content.append(&icon);
        content.append(&text);
        let button = gtk::ToggleButton::builder()
            .child(&content)
            .css_classes(["quick-toggle"])
            .hexpand(true)
            .build();
        let updating: Rc<Cell<bool>> = Rc::default();
        {
            let updating = updating.clone();
            button.connect_toggled(move |b| {
                if !updating.get() {
                    on_toggle(b.is_active());
                }
            });
        }
        Self {
            button,
            icon,
            title,
            subtitle,
            updating,
        }
    }

    fn set(&self, icon: &str, title: &str, subtitle: &str, on: bool) {
        self.icon.set_icon_name(Some(icon));
        self.title.set_label(title);
        self.subtitle.set_label(subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
        self.updating.set(true);
        self.button.set_active(on);
        self.updating.set(false);
    }
}

#[derive(Debug)]
pub struct QuickWidget {
    button: gtk::MenuButton,
    indicators: gtk::Box,
    layout: gtk::Label,
    battery: gtk::Box,
    battery_icon: gtk::Image,
    battery_label: gtk::Label,
    volume: Slider,
    brightness: Slider,
    network: Toggle,
    keyboard: Toggle,
    tiles: gtk::Grid,
    /// In their order on the grid.
    tile_order: [gtk::ToggleButton; 3],
    load: gtk::Label,
    actions: UnboundedSender<Action>,
    /// What scrolling over the volume indicator left over (a touchpad's
    /// fractions of a step).
    scrolled: Rc<Cell<f64>>,
    shown: RefCell<Option<QuickView>>,
}

fn interface_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(INTERFACE, true)?;
    Some(gio::Settings::new(INTERFACE))
}

impl QuickWidget {
    pub fn new(actions: &UnboundedSender<Action>) -> Self {
        let send = |change: fn(u32) -> Change| {
            let actions = actions.clone();
            move |v: u32| {
                let _ = actions.send(Action::Quick(change(v)));
            }
        };
        let volume = Slider::new("audio-volume-high-symbolic", send(Change::Volume));
        {
            let actions = actions.clone();
            volume.icon.connect_clicked(move |_| {
                let _ = actions.send(Action::Quick(Change::ToggleMute));
            });
        }
        let brightness = Slider::new("display-brightness-symbolic", send(Change::Brightness));
        let network = {
            let actions = actions.clone();
            Toggle::new(move |on| {
                let _ = actions.send(Action::Quick(Change::Wifi(on)));
            })
        };
        let dark = dark_toggle();
        let keyboard = {
            let actions = actions.clone();
            Toggle::new(move |_| {
                let _ = actions.send(Action::Quick(Change::NextLayout));
            })
        };
        let (top, battery, battery_icon, battery_label) = top_row(actions);
        let tiles = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .column_homogeneous(true)
            .build();
        // Placed by `arrange`, which skips hidden ones.
        for t in [&network.button, &dark.button, &keyboard.button] {
            tiles.attach(t, 0, 0, 1, 1);
        }
        let load = gtk::Label::builder()
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .width_request(340)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        for w in [
            top.upcast_ref::<gtk::Widget>(),
            volume.row.upcast_ref(),
            brightness.row.upcast_ref(),
            tiles.upcast_ref(),
            load.upcast_ref(),
        ] {
            content.append(w);
        }
        let popover = gtk::Popover::builder().child(&content).build();
        crate::popup::keep_inside(&popover);

        let indicators = gtk::Box::builder().spacing(8).build();
        let layout = gtk::Label::builder()
            .css_classes(["heading"])
            .visible(false)
            .build();
        let face = gtk::Box::builder().spacing(8).build();
        face.append(&layout);
        face.append(&indicators);
        let button = gtk::MenuButton::builder()
            .child(&face)
            .popover(&popover)
            .css_classes(["panel-button", "quick-settings"])
            .tooltip_text("Quick Settings")
            .build();

        Self {
            button,
            indicators,
            layout,
            battery,
            battery_icon,
            battery_label,
            volume,
            brightness,
            tile_order: [
                network.button.clone(),
                dark.button.clone(),
                keyboard.button.clone(),
            ],
            network,
            keyboard,
            tiles,
            load,
            actions: actions.clone(),
            scrolled: Rc::default(),
            shown: RefCell::new(None),
        }
    }

    /// Two tiles a row, the visible ones only: no gap for a missing one.
    fn arrange(&self) {
        let mut at = 0;
        for t in &self.tile_order {
            if t.parent().as_ref() == Some(self.tiles.upcast_ref()) {
                self.tiles.remove(t);
            }
            if t.is_visible() {
                self.tiles.attach(t, at % 2, at / 2, 1, 1);
                at += 1;
            }
        }
    }

    pub fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }

    /// Scrolling over the volume indicator changes the volume, as in GNOME.
    fn volume_scroll(&self) -> gtk::EventControllerScroll {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let (actions, carry) = (self.actions.clone(), self.scrolled.clone());
        scroll.connect_scroll(move |c, _, dy| {
            // A touchpad scrolls in pixels; Mutter counts 10 as a wheel step.
            let dy = match c.unit() {
                gtk::gdk::ScrollUnit::Surface => dy / 10.0,
                _ => dy,
            };
            let (step, rest) = quick::volume_scroll(carry.get(), dy);
            carry.set(rest);
            if step != 0 {
                let _ = actions.send(Action::Quick(Change::VolumeStep(step)));
            }
            gtk::glib::Propagation::Stop
        });
        scroll
    }

    pub fn render(&self, q: &QuickView) {
        if self.shown.borrow().as_ref() == Some(q) {
            return;
        }
        while let Some(child) = self.indicators.first_child() {
            self.indicators.remove(&child);
        }
        for icon in &q.indicators {
            let image = gtk::Image::from_icon_name(icon);
            if q.volume.as_ref().is_some_and(|(v, ..)| v == icon) {
                image.add_controller(self.volume_scroll());
            }
            self.indicators.append(&image);
        }
        match &q.layout {
            Some(l) => {
                self.layout.set_label(l);
                self.layout.set_visible(true);
            }
            None => self.layout.set_visible(false),
        }
        // Something to click even when nothing is known yet.
        if q.indicators.is_empty() && q.layout.is_none() {
            self.indicators
                .append(&gtk::Image::from_icon_name("emblem-system-symbolic"));
        }

        self.battery.set_visible(q.battery.is_some());
        if let Some((icon, label)) = &q.battery {
            self.battery_icon.set_icon_name(Some(icon));
            self.battery_label.set_label(label);
        }
        self.volume.row.set_visible(q.volume.is_some());
        if let Some((icon, percent, _muted)) = &q.volume {
            self.volume.icon.set_icon_name(icon);
            self.volume.set(*percent);
        }
        self.brightness.row.set_visible(q.brightness.is_some());
        if let Some(b) = q.brightness {
            self.brightness.set(b);
        }
        self.network.button.set_visible(q.network.is_some());
        if let Some((icon, title, subtitle, wifi)) = &q.network {
            // Without Wi-Fi hardware the tile only shows the connection.
            self.network.button.set_sensitive(wifi.is_some());
            self.network
                .set(icon, title, subtitle, wifi.unwrap_or(true));
        }
        self.keyboard.button.set_visible(q.keyboard.is_some());
        if let Some((short, long, several)) = &q.keyboard {
            self.keyboard.set(
                "input-keyboard-symbolic",
                long,
                &short.to_uppercase(),
                false,
            );
            self.keyboard.button.set_sensitive(*several);
        }
        self.arrange();
        self.load.set_visible(q.load.is_some());
        if let Some(l) = &q.load {
            self.load.set_label(l);
        }
        self.shown.replace(Some(q.clone()));
    }
}

/// Dark style: `org.gnome.desktop.interface color-scheme`, which GTK and the
/// settings portal read (ADR 0043); follows changes made elsewhere.
fn dark_toggle() -> Toggle {
    let settings = interface_settings();
    let dark = {
        let settings = settings.clone();
        Toggle::new(move |on| {
            if let Some(s) = &settings {
                let _ = s.set_string("color-scheme", if on { "prefer-dark" } else { "default" });
            }
        })
    };
    let dark_now = {
        let dark = dark.clone();
        move |s: &gio::Settings| {
            let on = s.string("color-scheme") == "prefer-dark";
            dark.set("weather-clear-night-symbolic", "Dark Style", "", on);
        }
    };
    match &settings {
        Some(s) => {
            dark_now(s);
            s.connect_changed(Some("color-scheme"), move |s, _| dark_now(s));
        }
        None => dark.button.set_visible(false),
    }

    dark
}

/// Battery on the left; GNOME Settings and Lock on the right.
/// Close the popover `widget` is in.
fn close_popover(widget: &impl IsA<gtk::Widget>) {
    let mut w = widget.ancestor(gtk::Popover::static_type());
    while let Some(p) = w {
        let parent = p.parent();
        if let Ok(p) = p.downcast::<gtk::Popover>() {
            p.popdown();
        }
        w = parent.and_then(|p| p.ancestor(gtk::Popover::static_type()));
    }
}

/// GNOME's power menu: suspend at once, the others ask first (M8.47).
fn power_menu(actions: &UnboundedSender<Action>) -> gtk::MenuButton {
    let items = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let item = |label: &str| {
        let b = gtk::Button::builder()
            .label(label)
            .css_classes(["flat"])
            .build();
        if let Some(l) = b.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        items.append(&b);
        b
    };
    let suspend = item("Suspend");
    {
        let actions = actions.clone();
        suspend.connect_clicked(move |b| {
            close_popover(b);
            let _ = actions.send(Action::Suspend);
        });
    }
    for (label, name) in [("Restart…", "app.restart"), ("Power Off…", "app.power-off")] {
        item(label).connect_clicked(move |b| {
            close_popover(b);
            let _ = b.activate_action(name, None);
        });
    }
    items.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    item("Log Out…").connect_clicked(|b| {
        close_popover(b);
        let _ = b.activate_action("app.log-out", None);
    });
    gtk::MenuButton::builder()
        .icon_name("system-shutdown-symbolic")
        .tooltip_text("Power Off / Log Out")
        .css_classes(["circular"])
        .popover(&gtk::Popover::builder().child(&items).build())
        .build()
}

fn top_row(actions: &UnboundedSender<Action>) -> (gtk::Box, gtk::Box, gtk::Image, gtk::Label) {
    let battery_icon = gtk::Image::new();
    let battery_label = gtk::Label::new(None);
    let battery = gtk::Box::builder().spacing(6).build();
    battery.append(&battery_icon);
    battery.append(&battery_label);
    let top = gtk::Box::builder().spacing(6).build();
    top.append(&battery);
    let spacer = gtk::Box::builder().hexpand(true).build();
    top.append(&spacer);
    // A screenshot of an area once the menu is gone (M8.48).
    let shot = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .tooltip_text("Take Screenshot")
        .css_classes(["circular"])
        .build();
    shot.connect_clicked(|b| {
        close_popover(b);
        gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(400), || {
            if let Err(e) = gio::Subprocess::newv(
                &["koje".as_ref(), "screenshot".as_ref(), "area".as_ref()],
                gio::SubprocessFlags::NONE,
            ) {
                tracing::warn!("taking a screenshot: {e}");
            }
        });
    });
    top.append(&shot);
    if let Some(app) = gio_unix::DesktopAppInfo::new("org.gnome.Settings.desktop") {
        let b = gtk::Button::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Settings")
            .css_classes(["circular"])
            .build();
        b.connect_clicked(move |b| {
            launch_gnome_settings(&app, &b.display());
            close_popover(b);
        });
        top.append(&b);
    }
    let lock = gtk::Button::builder()
        .icon_name("system-lock-screen-symbolic")
        .tooltip_text("Lock")
        .css_classes(["circular"])
        .build();
    lock.connect_clicked(|_| {
        if let Err(e) = gio::Subprocess::newv(
            &["loginctl".as_ref(), "lock-session".as_ref()],
            gio::SubprocessFlags::NONE,
        ) {
            tracing::warn!("locking the screen: {e}");
        }
    });
    top.append(&lock);
    top.append(&power_menu(actions));

    (top, battery, battery_icon, battery_label)
}

/// Start GNOME Settings, or one of its panels' desktop files. It exits at
/// once unless `XDG_CURRENT_DESKTOP` names GNOME; set for this process only.
pub fn launch_gnome_settings(app: &gio_unix::DesktopAppInfo, display: &gtk4::gdk::Display) {
    let context = display.app_launch_context();
    let current = std::env::var("XDG_CURRENT_DESKTOP").ok();
    context.setenv("XDG_CURRENT_DESKTOP", gnome_desktop(current.as_deref()));
    if let Err(e) = app.launch(&[], Some(&context)) {
        tracing::warn!("starting GNOME Settings: {e}");
    }
}

/// `XDG_CURRENT_DESKTOP` for GNOME Settings: the session's desktops with
/// `GNOME` added (`Hyprland:GNOME`) – it refuses to run without it.
pub fn gnome_desktop(current: Option<&str>) -> String {
    let mut desktops: Vec<&str> = current
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .collect();
    if !desktops.contains(&"GNOME") {
        desktops.push("GNOME");
    }
    desktops.join(":")
}

/// GNOME's quick toggles: pills, the accent when on.
pub const CSS: &str = "
.quick-toggle { border-radius: 9999px; padding: 8px 14px; }
.quick-toggle:checked { background: var(--accent-bg-color); color: var(--accent-fg-color); }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_settings_sees_gnome() {
        assert_eq!(gnome_desktop(Some("Hyprland")), "Hyprland:GNOME");
        assert_eq!(gnome_desktop(Some("Hyprland:GNOME")), "Hyprland:GNOME");
        assert_eq!(gnome_desktop(Some("")), "GNOME");
        assert_eq!(gnome_desktop(None), "GNOME");
    }
}
