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
use crate::quick::{self, DeviceRow, MenuRow, QuickView};
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

/// A menu under a slider or tile, as GNOME's quick settings menus: a
/// header, rows (icon, label, a check on the current one, maybe a lock),
/// then "… Settings" opening GNOME Settings' panel. Opened by `button` (an
/// arrow), shown only with more than `min` rows.
#[derive(Clone)]
struct Menu {
    button: gtk::ToggleButton,
    revealer: gtk::Revealer,
    list: gtk::Box,
    pick: Rc<dyn Fn(&MenuRow)>,
    min: usize,
    shown: Rc<RefCell<Vec<MenuRow>>>,
}

impl std::fmt::Debug for Menu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Menu")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Menu {
    fn new(
        icon: &str,
        title: &str,
        footer: (&str, &str),
        min: usize,
        pick: impl Fn(&MenuRow) + 'static,
    ) -> Self {
        let button = gtk::ToggleButton::builder()
            .icon_name("go-next-symbolic")
            .css_classes(["flat", "circular", "quick-menu-button"])
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let header = gtk::Box::builder()
            .spacing(12)
            .css_classes(["quick-menu-header"])
            .build();
        header.append(&gtk::Image::from_icon_name(icon));
        header.append(
            &gtk::Label::builder()
                .label(title)
                .css_classes(["heading"])
                .build(),
        );
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let (label, desktop) = footer;
        let settings = gtk::Button::builder()
            .label(label)
            .css_classes(["flat", "quick-menu-item"])
            .build();
        if let Some(l) = settings.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        match gio_unix::DesktopAppInfo::new(desktop) {
            Some(app) => {
                settings.connect_clicked(move |b| {
                    launch_gnome_settings(&app, &b.display());
                    close_popover(b);
                });
            }
            None => settings.set_visible(false),
        }
        let menu = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["quick-menu"])
            .build();
        menu.append(&header);
        menu.append(&list);
        menu.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        menu.append(&settings);
        let revealer = gtk::Revealer::builder().child(&menu).build();
        {
            let revealer = revealer.clone();
            button.connect_toggled(move |b| revealer.set_reveal_child(b.is_active()));
        }
        Self {
            button,
            revealer,
            list,
            pick: Rc::new(pick),
            min,
            shown: Rc::default(),
        }
    }

    fn set(&self, rows: &[MenuRow]) {
        self.button.set_visible(rows.len() > self.min);
        if rows.len() <= self.min {
            self.button.set_active(false);
        }
        if self.shown.borrow().as_slice() == rows {
            return;
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for r in rows {
            let row = gtk::Box::builder().spacing(12).build();
            row.append(&gtk::Image::from_icon_name(&r.icon));
            row.append(
                &gtk::Label::builder()
                    .label(&r.label)
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build(),
            );
            if let Some(suffix) = r.suffix {
                row.append(&gtk::Image::from_icon_name(suffix));
            }
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_opacity(if r.checked { 1.0 } else { 0.0 });
            row.append(&check);
            let item = gtk::Button::builder()
                .child(&row)
                .css_classes(["flat", "quick-menu-item"])
                .build();
            let (pick, r) = (self.pick.clone(), r.clone());
            item.connect_clicked(move |_| pick(&r));
            self.list.append(&item);
        }
        self.shown.replace(rows.to_vec());
    }
}

/// The grid of tiles; `arrange` places them.
fn tile_grid(tiles: &[gtk::Widget]) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .column_spacing(8)
        .row_spacing(8)
        .column_homogeneous(true)
        .build();
    for t in tiles {
        grid.attach(t, 0, 0, 1, 1);
    }
    grid
}

/// One menu open at a time, as GNOME's.
fn one_open(menus: &[&Menu]) {
    for m in menus {
        let others: Vec<gtk::ToggleButton> = menus
            .iter()
            .filter(|o| o.button != m.button)
            .map(|o| o.button.clone())
            .collect();
        m.button.connect_toggled(move |b| {
            if b.is_active() {
                for o in &others {
                    o.set_active(false);
                }
            }
        });
    }
}

/// Wi-Fi, Bluetooth and Power Mode: the pill toggles, the arrow opens the
/// menu. Power Mode toggles between balanced and power saver.
fn menu_toggles(
    actions: &UnboundedSender<Action>,
    wifi: &Menu,
    bluetooth: &Menu,
    power: &Menu,
) -> (Toggle, Toggle, Toggle) {
    let send = |change: fn(bool) -> Change| {
        let actions = actions.clone();
        move |on| {
            let _ = actions.send(Action::Quick(change(on)));
        }
    };
    let network = Toggle::new(send(Change::Wifi)).with_menu(wifi);
    let bt = Toggle::new(send(Change::Bluetooth)).with_menu(bluetooth);
    let mode = Toggle::new(send(|on| {
        Change::PowerProfile(if on { "power-saver" } else { "balanced" }.to_owned())
    }))
    .with_menu(power);
    (network, bt, mode)
}

/// The Wi-Fi, Bluetooth and power mode menus (GNOME's quick menu toggles).
/// A secured network never joined opens GNOME Settings' Wi-Fi panel: koje
/// has no password dialog yet (M8.45).
fn tile_menus(actions: &UnboundedSender<Action>) -> (Menu, Menu, Menu) {
    let wifi = {
        let actions = actions.clone();
        Menu::new(
            "network-wireless-symbolic",
            "Wi-Fi",
            ("All Networks", "gnome-wifi-panel.desktop"),
            0,
            move |r| {
                if r.ask {
                    if let Some(app) = gio_unix::DesktopAppInfo::new("gnome-wifi-panel.desktop")
                        && let Some(d) = gtk4::gdk::Display::default()
                    {
                        launch_gnome_settings(&app, &d);
                    }
                } else if !r.checked {
                    let _ = actions.send(Action::Quick(Change::WifiConnect(r.key.clone())));
                }
            },
        )
    };
    let bluetooth = {
        let actions = actions.clone();
        Menu::new(
            "bluetooth-active-symbolic",
            "Bluetooth",
            ("Bluetooth Settings", "gnome-bluetooth-panel.desktop"),
            0,
            move |r| {
                let change = Change::BluetoothDevice(r.key.clone(), !r.checked);
                let _ = actions.send(Action::Quick(change));
            },
        )
    };
    let power = {
        let actions = actions.clone();
        Menu::new(
            "power-profile-balanced-symbolic",
            "Power Mode",
            ("Power Settings", "gnome-power-panel.desktop"),
            0,
            move |r| {
                let _ = actions.send(Action::Quick(Change::PowerProfile(r.key.clone())));
            },
        )
    };
    (wifi, bluetooth, power)
}

/// A sound device menu: picking one makes it the default.
fn device_menu(icon: &str, title: &str, actions: &UnboundedSender<Action>) -> Menu {
    let actions = actions.clone();
    Menu::new(
        icon,
        title,
        ("Sound Settings", "gnome-sound-panel.desktop"),
        1,
        move |r| {
            if let Ok(id) = r.key.parse() {
                let _ = actions.send(Action::Quick(Change::DefaultDevice(id)));
            }
        },
    )
}

/// The devices as menu rows.
fn device_rows(rows: &[DeviceRow]) -> Vec<MenuRow> {
    rows.iter()
        .map(|(id, name, icon, active)| MenuRow {
            key: id.to_string(),
            label: name.clone(),
            icon: (*icon).to_owned(),
            checked: *active,
            suffix: None,
            ask: false,
        })
        .collect()
}

/// The volume and microphone sliders with their device menus: a click on
/// the icon mutes.
fn sound_sliders(actions: &UnboundedSender<Action>) -> (Slider, Menu, Slider, Menu) {
    let slider = |icon: &str, set: fn(u32) -> Change, mute: Change| {
        let s = {
            let actions = actions.clone();
            Slider::new(icon, move |v| {
                let _ = actions.send(Action::Quick(set(v)));
            })
        };
        let actions = actions.clone();
        s.icon.connect_clicked(move |_| {
            let _ = actions.send(Action::Quick(mute.clone()));
        });
        s
    };
    let volume = slider(
        "audio-volume-high-symbolic",
        Change::Volume,
        Change::ToggleMute,
    );
    let outputs = device_menu("audio-headphones-symbolic", "Sound Output", actions);
    volume.row.append(&outputs.button);
    let microphone = slider(
        "audio-input-microphone-symbolic",
        Change::MicrophoneVolume,
        Change::ToggleMicrophoneMute,
    );
    let inputs = device_menu("audio-input-microphone-symbolic", "Sound Input", actions);
    microphone.row.append(&inputs.button);
    (volume, outputs, microphone, inputs)
}

/// A GNOME quick toggle: icon, title, subtitle; filled with the accent when on.
#[derive(Debug, Clone)]
struct Toggle {
    /// What sits in the grid: the button, or the button and a menu arrow.
    tile: gtk::Widget,
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
            tile: button.clone().upcast(),
            button,
            icon,
            title,
            subtitle,
            updating,
        }
    }

    /// GNOME's menu toggle: the pill toggles, its arrow opens `menu`.
    fn with_menu(mut self, menu: &Menu) -> Self {
        let tile = gtk::Box::builder()
            .css_classes(["quick-menu-toggle"])
            .hexpand(true)
            .build();
        tile.append(&self.button);
        tile.append(&menu.button);
        self.tile = tile.upcast();
        self
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
    outputs: Menu,
    microphone: Slider,
    inputs: Menu,
    brightness: Slider,
    network: Toggle,
    keyboard: Toggle,
    tiles: gtk::Grid,
    /// In their order on the grid.
    tile_order: Vec<gtk::Widget>,
    wifi: Menu,
    bluetooth: Toggle,
    bluetooth_menu: Menu,
    power: Toggle,
    power_menu: Menu,
    /// GNOME's interface settings: "Show Battery Percentage".
    interface: Option<gio::Settings>,
    load: gtk::Label,
    actions: UnboundedSender<Action>,
    /// What scrolling over the volume indicator left over (a touchpad's
    /// fractions of a step).
    scrolled: Rc<Cell<f64>>,
    shown: RefCell<Option<QuickView>>,
    shown_percent: Cell<bool>,
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
        let (volume, outputs, microphone, inputs) = sound_sliders(actions);
        let brightness = Slider::new("display-brightness-symbolic", send(Change::Brightness));
        let (wifi, bluetooth_menu, power_menu) = tile_menus(actions);
        let (network, bluetooth, power) =
            menu_toggles(actions, &wifi, &bluetooth_menu, &power_menu);
        let dark = dark_toggle();
        let dnd = do_not_disturb_toggle();
        let keyboard = {
            let actions = actions.clone();
            Toggle::new(move |_| {
                let _ = actions.send(Action::Quick(Change::NextLayout));
            })
        };
        let (top, battery, battery_icon, battery_label) = top_row(actions);
        // GNOME's order (`panel.js`): network, Bluetooth, power mode, …
        let tile_order: Vec<gtk::Widget> = [&network, &bluetooth, &power, &dark, &dnd, &keyboard]
            .iter()
            .map(|t| t.tile.clone())
            .collect();
        let tiles = tile_grid(&tile_order);
        one_open(&[&wifi, &bluetooth_menu, &power_menu]);
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
            outputs.revealer.upcast_ref(),
            microphone.row.upcast_ref(),
            inputs.revealer.upcast_ref(),
            brightness.row.upcast_ref(),
            tiles.upcast_ref(),
            wifi.revealer.upcast_ref(),
            bluetooth_menu.revealer.upcast_ref(),
            power_menu.revealer.upcast_ref(),
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
            outputs,
            microphone,
            inputs,
            brightness,
            tile_order,
            wifi,
            bluetooth,
            bluetooth_menu,
            power,
            power_menu,
            interface: interface_settings(),
            network,
            keyboard,
            tiles,
            load,
            actions: actions.clone(),
            scrolled: Rc::default(),
            shown: RefCell::new(None),
            shown_percent: Cell::new(false),
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
        let percent = self
            .interface
            .as_ref()
            .is_some_and(|s| s.boolean("show-battery-percentage"));
        if self.shown.borrow().as_ref() == Some(q) && self.shown_percent.get() == percent {
            return;
        }
        self.shown_percent.set(percent);
        while let Some(child) = self.indicators.first_child() {
            self.indicators.remove(&child);
        }
        for icon in &q.indicators {
            let image = gtk::Image::from_icon_name(icon);
            if q.volume.as_ref().is_some_and(|(v, ..)| v == icon) {
                image.add_controller(self.volume_scroll());
            }
            self.indicators.append(&image);
            // GNOME's "Show Battery Percentage" (`show-battery-percentage`).
            if percent
                && q.battery.as_ref().is_some_and(|(b, _)| b == icon)
                && let Some(p) = q.battery_percent
            {
                self.indicators
                    .append(&gtk::Label::new(Some(&format!("{p} %"))));
            }
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
        self.outputs.set(&device_rows(&q.outputs));
        self.microphone.row.set_visible(q.microphone.is_some());
        if let Some((icon, percent, _muted)) = &q.microphone {
            self.microphone.icon.set_icon_name(icon);
            self.microphone.set(*percent);
        } else {
            self.inputs.button.set_active(false);
        }
        self.inputs.set(&device_rows(&q.inputs));
        self.brightness.row.set_visible(q.brightness.is_some());
        if let Some(b) = q.brightness {
            self.brightness.set(b);
        }
        self.network.tile.set_visible(q.network.is_some());
        self.wifi.set(&q.wifi);
        self.bluetooth.tile.set_visible(q.bluetooth.is_some());
        if let Some((icon, title, subtitle, on, rows)) = &q.bluetooth {
            self.bluetooth.set(icon, title, subtitle, *on);
            self.bluetooth_menu.set(rows);
        }
        self.power.tile.set_visible(q.power.is_some());
        if let Some((icon, title, subtitle, on, rows)) = &q.power {
            self.power.set(icon, title, subtitle, *on);
            self.power_menu.set(rows);
        }
        if let Some((icon, title, subtitle, wifi)) = &q.network {
            // Without Wi-Fi hardware the tile only shows the connection.
            self.network.button.set_sensitive(wifi.is_some());
            self.network
                .set(icon, title, subtitle, wifi.unwrap_or(true));
        }
        self.keyboard.tile.set_visible(q.keyboard.is_some());
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

/// GNOME 50's "Do Not Disturb" (`status/doNotDisturb.js`):
/// `org.gnome.desktop.notifications show-banners` turned off; the
/// notification server follows the setting (M8.36).
fn do_not_disturb_toggle() -> Toggle {
    let settings = crate::notifications_ui::notification_settings();
    let toggle = {
        let settings = settings.clone();
        Toggle::new(move |on| {
            if let Some(s) = &settings {
                let _ = s.set_boolean("show-banners", !on);
            }
        })
    };
    let now = {
        let toggle = toggle.clone();
        move |s: &gio::Settings| {
            let on = !s.boolean("show-banners");
            toggle.set("notifications-disabled-symbolic", "Do Not Disturb", "", on);
        }
    };
    match &settings {
        Some(s) => {
            now(s);
            s.connect_changed(Some("show-banners"), move |s, _| now(s));
        }
        None => toggle.tile.set_visible(false),
    }
    toggle
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
        None => dark.tile.set_visible(false),
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
.quick-menu { background: alpha(currentColor, 0.05); border-radius: 18px; padding: 12px; margin-top: 6px; }
.quick-menu-header { padding: 0 6px 6px 6px; }
.quick-menu-item { padding: 6px; border-radius: 9px; font-weight: normal; }
.quick-menu-button:checked { background: alpha(currentColor, 0.15); }
.quick-menu-toggle > .quick-toggle { border-top-right-radius: 0; border-bottom-right-radius: 0; }
.quick-menu-toggle > .quick-menu-button { border-radius: 0 9999px 9999px 0; min-width: 32px; background: alpha(currentColor, 0.1); }
.quick-menu-toggle > .quick-toggle:checked + .quick-menu-button {
  background: var(--accent-bg-color); color: var(--accent-fg-color); border-left: 1px solid alpha(black, 0.1);
}
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
