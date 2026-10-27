//! Quick Settings (M8.30), without GTK: what the system reports – sound,
//! brightness, network, battery, keyboard layout, CPU and memory – and what
//! the button and its dropdown show of it, with GNOME's symbolic icons.

use koje_core::media::{Level, brightness_percent};
use koje_core::size::ByteSize;
use serde::Deserialize;

/// The panel's backlight: `/sys/class/backlight/<device>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brightness {
    pub device: String,
    pub raw: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetKind {
    Wired,
    Wifi,
    Vpn,
    Other,
}

/// `NetworkManager`'s view: the primary connection, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    /// `None` when offline.
    pub primary: Option<(NetKind, String)>,
    /// Wi-Fi signal 0–100 of the primary connection.
    pub strength: Option<u8>,
    /// `None` without Wi-Fi hardware.
    pub wifi_enabled: Option<bool>,
}

/// `UPower`'s display device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    /// `UPower`'s own icon name for the state.
    pub icon: String,
}

/// The keyboard layouts of the main keyboard (Hyprland's `j/devices`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyboard {
    /// `us`, `de`, …
    pub layouts: Vec<String>,
    pub active: usize,
    /// `English (US)`.
    pub name: String,
}

/// CPU and memory of the whole machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    pub cpu_percent: u32,
    pub mem_used: u64,
    pub mem_total: u64,
}

/// Everything Quick Settings knows; `None` for what this machine has not or
/// what could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct System {
    pub audio: Option<Level>,
    /// The default input's level (M8.84).
    pub microphone: Option<Level>,
    /// Outputs and inputs to choose from (M8.84).
    pub devices: Option<crate::sound::Devices>,
    /// Wi-Fi networks in range (M8.85).
    pub wifi_networks: Vec<WifiNetwork>,
    pub bluetooth: Option<Bluetooth>,
    pub power: Option<PowerProfiles>,
    pub brightness: Option<Brightness>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
    pub keyboard: Option<Keyboard>,
    pub load: Option<Load>,
}

/// A Wi-Fi network in range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0–100.
    pub strength: u8,
    pub secured: bool,
    /// Saved: it can be joined without a password.
    pub known: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bluetooth {
    pub powered: bool,
    /// Paired devices.
    pub devices: Vec<BtDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    /// `BlueZ`'s object path.
    pub path: String,
    pub name: String,
    /// `BlueZ`'s icon name (`audio-headset`, …).
    pub icon: String,
    pub connected: bool,
}

/// power-profiles-daemon's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerProfiles {
    pub active: String,
    pub available: Vec<String>,
}

/// A row of a tile's menu: what a click picks, its label and icon, checked,
/// and an icon after the label (a lock for secured networks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRow {
    pub key: String,
    pub label: String,
    pub icon: String,
    pub checked: bool,
    pub suffix: Option<&'static str>,
    /// Picking it needs a password koje cannot ask for: GNOME Settings does.
    pub ask: bool,
}

/// GNOME's Wi-Fi signal icons.
pub fn signal_icon(strength: u8) -> &'static str {
    match strength {
        0..=19 => "network-wireless-signal-none-symbolic",
        20..=39 => "network-wireless-signal-weak-symbolic",
        40..=59 => "network-wireless-signal-ok-symbolic",
        60..=79 => "network-wireless-signal-good-symbolic",
        _ => "network-wireless-signal-excellent-symbolic",
    }
}

/// The Wi-Fi menu as GNOME orders it: the active network, then saved ones,
/// then the rest by signal; one row per name (its strongest).
pub fn wifi_rows(networks: &[WifiNetwork]) -> Vec<MenuRow> {
    let mut best: Vec<&WifiNetwork> = Vec::new();
    for n in networks {
        match best.iter_mut().find(|b| b.ssid == n.ssid) {
            Some(b) if n.active || (!b.active && n.strength > b.strength) => *b = n,
            Some(_) => {}
            None => best.push(n),
        }
    }
    best.sort_by_key(|n| (!n.active, !n.known, std::cmp::Reverse(n.strength)));
    best.into_iter()
        .map(|n| MenuRow {
            key: n.ssid.clone(),
            label: n.ssid.clone(),
            icon: signal_icon(n.strength).to_owned(),
            checked: n.active,
            suffix: n.secured.then_some("network-wireless-encrypted-symbolic"),
            ask: n.secured && !n.known && !n.active,
        })
        .collect()
}

/// GNOME's power modes, in its order.
const MODES: [(&str, &str, &str); 3] = [
    (
        "performance",
        "Performance",
        "power-profile-performance-symbolic",
    ),
    ("balanced", "Balanced", "power-profile-balanced-symbolic"),
    (
        "power-saver",
        "Power Saver",
        "power-profile-power-saver-symbolic",
    ),
];

fn mode(profile: &str) -> (&'static str, &'static str) {
    MODES.iter().find(|(p, ..)| *p == profile).map_or(
        ("Balanced", "power-profile-balanced-symbolic"),
        |(_, l, i)| (*l, *i),
    )
}

pub fn power_rows(p: &PowerProfiles) -> Vec<MenuRow> {
    MODES
        .iter()
        .filter(|(m, ..)| p.available.is_empty() || p.available.iter().any(|a| a == m))
        .map(|(m, label, icon)| MenuRow {
            key: (*m).to_owned(),
            label: (*label).to_owned(),
            icon: (*icon).to_owned(),
            checked: p.active == *m,
            suffix: None,
            ask: false,
        })
        .collect()
}

pub fn bluetooth_rows(b: &Bluetooth) -> Vec<MenuRow> {
    b.devices
        .iter()
        .map(|d| MenuRow {
            key: d.path.clone(),
            label: d.name.clone(),
            icon: if d.icon.is_empty() {
                "bluetooth-active-symbolic".to_owned()
            } else {
                format!("{}-symbolic", d.icon)
            },
            checked: d.connected,
            suffix: None,
            ask: false,
        })
        .collect()
}

/// A tile: icon, title, subtitle, on, and its menu.
pub type Tile = (String, String, String, bool, Vec<MenuRow>);

/// GNOME's Power Mode tile: on unless balanced.
pub fn power_tile(p: &PowerProfiles) -> Tile {
    let (label, icon) = mode(&p.active);
    (
        icon.to_owned(),
        "Power Mode".to_owned(),
        label.to_owned(),
        p.active != "balanced",
        power_rows(p),
    )
}

/// GNOME's Bluetooth tile: the connected device (or how many) as subtitle.
pub fn bluetooth_tile(b: &Bluetooth) -> Tile {
    let connected: Vec<&BtDevice> = b.devices.iter().filter(|d| d.connected).collect();
    let subtitle = match connected.as_slice() {
        _ if !b.powered => String::new(),
        [] => String::new(),
        [one] => one.name.clone(),
        many => format!("{} Connected", many.len()),
    };
    (
        if b.powered {
            "bluetooth-active-symbolic"
        } else {
            "bluetooth-disabled-symbolic"
        }
        .to_owned(),
        "Bluetooth".to_owned(),
        subtitle,
        b.powered,
        if b.powered {
            bluetooth_rows(b)
        } else {
            Vec::new()
        },
    )
}

/// GNOME's volume icon.
pub fn volume_icon(level: Level) -> &'static str {
    match (level.muted, level.percent) {
        (true, _) | (false, 0) => "audio-volume-muted-symbolic",
        (false, 1..=33) => "audio-volume-low-symbolic",
        (false, 34..=66) => "audio-volume-medium-symbolic",
        (false, _) => "audio-volume-high-symbolic",
    }
}

/// GNOME's volume slider moves 2 % per wheel step.
const SCROLL_STEP: f64 = 2.0;

/// Whole percent to change the volume by for a scroll of `dy` wheel steps
/// (down positive) with `carry` left over from before, and the new carry:
/// a touchpad scrolls in fractions of a step.
pub fn volume_scroll(carry: f64, dy: f64) -> (i32, f64) {
    let total = carry - dy * SCROLL_STEP;
    let whole = total.trunc();
    #[allow(clippy::cast_possible_truncation, reason = "a few percent")]
    (whole as i32, total - whole)
}

/// GNOME's network icon.
pub fn network_icon(n: &Network) -> &'static str {
    match &n.primary {
        None if n.wifi_enabled == Some(false) => "network-wireless-disabled-symbolic",
        None => "network-offline-symbolic",
        Some((NetKind::Wired | NetKind::Other, _)) => "network-wired-symbolic",
        Some((NetKind::Vpn, _)) => "network-vpn-symbolic",
        Some((NetKind::Wifi, _)) => match n.strength.unwrap_or(0) {
            0..=19 => "network-wireless-signal-none-symbolic",
            20..=39 => "network-wireless-signal-weak-symbolic",
            40..=59 => "network-wireless-signal-ok-symbolic",
            60..=79 => "network-wireless-signal-good-symbolic",
            _ => "network-wireless-signal-excellent-symbolic",
        },
    }
}

/// The two numbers `/proc/stat`'s `cpu` line gives for a usage between two
/// readings: `(busy, total)` jiffies.
pub fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    // user nice system idle iowait irq softirq steal …
    let idle = v.get(3)? + v.get(4).unwrap_or(&0);
    let total: u64 = v.iter().take(8).sum();
    Some((total.saturating_sub(idle), total))
}

/// CPU use between two `(busy, total)` readings, in percent.
pub fn cpu_percent(before: (u64, u64), now: (u64, u64)) -> u32 {
    let busy = now.0.saturating_sub(before.0);
    let total = now.1.saturating_sub(before.1);
    if total == 0 {
        return 0;
    }
    u32::try_from((busy * 100 + total / 2) / total).unwrap_or(100)
}

/// `(used, total)` bytes from `/proc/meminfo`: used is what is not available.
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kib = |key: &str| -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        line.split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .map(|k| k * 1024)
    };
    let total = kib("MemTotal:")?;
    let available = kib("MemAvailable:")?;
    Some((total.saturating_sub(available), total))
}

#[derive(Debug, Deserialize)]
struct Devices {
    #[serde(default)]
    keyboards: Vec<KeyboardDevice>,
}

#[derive(Debug, Deserialize)]
struct KeyboardDevice {
    #[serde(default)]
    layout: String,
    #[serde(default)]
    active_keymap: String,
    #[serde(default)]
    active_layout_index: Option<usize>,
    #[serde(default)]
    main: bool,
}

/// The main keyboard's layouts from Hyprland's `j/devices`.
pub fn parse_keyboard(json: &str) -> Option<Keyboard> {
    let devices: Devices = serde_json::from_str(json).ok()?;
    let kb = devices
        .keyboards
        .iter()
        .find(|k| k.main)
        .or_else(|| devices.keyboards.first())?;
    let layouts: Vec<String> = kb
        .layout
        .split(',')
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect();
    Some(Keyboard {
        active: kb
            .active_layout_index
            .unwrap_or(0)
            .min(layouts.len().saturating_sub(1)),
        layouts,
        name: kb.active_keymap.clone(),
    })
}

/// What the Quick Settings button and its dropdown show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickView {
    /// The icons in the bar, left to right.
    pub indicators: Vec<String>,
    /// The active layout's short name in the bar, when there are several.
    pub layout: Option<String>,
    pub volume: Option<(String, u32, bool)>,
    /// Outputs for the volume slider's menu (M8.84).
    pub outputs: Vec<DeviceRow>,
    /// The microphone slider, only while an app records, as GNOME's.
    pub microphone: Option<(String, u32, bool)>,
    pub inputs: Vec<DeviceRow>,
    /// Perceived level 0–100.
    pub brightness: Option<u32>,
    /// `(icon, title, subtitle, wifi on)`; `wifi on` is `None` without
    /// Wi-Fi hardware.
    pub network: Option<(String, String, String, Option<bool>)>,
    /// `(icon, 87 %, charging)`.
    pub battery: Option<(String, String)>,
    /// For GNOME's "Show Battery Percentage" beside the icon.
    pub battery_percent: Option<u32>,
    /// The Wi-Fi tile's menu (M8.85).
    pub wifi: Vec<MenuRow>,
    pub bluetooth: Option<Tile>,
    pub power: Option<Tile>,
    /// `(short, long, several layouts)`.
    pub keyboard: Option<(String, String, bool)>,
    /// `CPU 12 % · Memory 6.1 GiB of 31.2 GiB`.
    pub load: Option<String>,
}

/// A device in a slider's menu: id, "port – card", icon, the default one.
pub type DeviceRow = (u32, String, &'static str, bool);

/// GNOME's microphone icon.
pub fn microphone_icon(level: Level) -> &'static str {
    match (level.muted, level.percent) {
        (true, _) | (false, 0) => "microphone-sensitivity-muted-symbolic",
        (false, 1..=33) => "microphone-sensitivity-low-symbolic",
        (false, 34..=66) => "microphone-sensitivity-medium-symbolic",
        (false, _) => "microphone-sensitivity-high-symbolic",
    }
}

fn rows(list: &[crate::sound::Device], default: Option<u32>) -> Vec<DeviceRow> {
    list.iter()
        .map(|d| (d.id, d.name.clone(), d.icon, Some(d.id) == default))
        .collect()
}

/// `dnd`: GNOME's Do Not Disturb, an icon in the bar while on.
pub fn quick(s: &System, dnd: bool) -> QuickView {
    let recording = s.devices.as_ref().is_some_and(|d| d.recording);
    let microphone = s
        .microphone
        .filter(|_| recording)
        .map(|l| (microphone_icon(l).to_owned(), l.percent.min(100), l.muted));
    let network = s.network.as_ref().map(|n| {
        let (title, subtitle) = match &n.primary {
            Some((NetKind::Wifi, name)) => ("Wi-Fi".to_owned(), name.clone()),
            Some((NetKind::Wired, name)) => ("Wired".to_owned(), name.clone()),
            Some((NetKind::Vpn, name)) => ("VPN".to_owned(), name.clone()),
            Some((NetKind::Other, name)) => ("Network".to_owned(), name.clone()),
            None if n.wifi_enabled == Some(false) => ("Wi-Fi".to_owned(), "Off".to_owned()),
            None => ("Network".to_owned(), "Offline".to_owned()),
        };
        (network_icon(n).to_owned(), title, subtitle, n.wifi_enabled)
    });
    let battery = s.battery.as_ref().map(|b| {
        let label = if b.charging {
            format!("{} % – charging", b.percent)
        } else {
            format!("{} %", b.percent)
        };
        (b.icon.clone(), label)
    });
    let keyboard = s.keyboard.as_ref().and_then(|k| {
        let short = k.layouts.get(k.active)?.clone();
        Some((short, k.name.clone(), k.layouts.len() > 1))
    });
    let mut indicators = Vec::new();
    if let Some((icon, ..)) = &network {
        indicators.push(icon.clone());
    }
    // GNOME's order: the microphone (a privacy indicator) first, …
    if let Some((icon, ..)) = &microphone {
        indicators.insert(0, icon.clone());
    }
    if dnd {
        indicators.push("notifications-disabled-symbolic".to_owned());
    }
    if let Some(level) = s.audio {
        indicators.push(volume_icon(level).to_owned());
    }
    if let Some((icon, _)) = &battery {
        indicators.push(icon.clone());
    }
    QuickView {
        indicators,
        layout: keyboard.as_ref().filter(|k| k.2).map(|k| k.0.clone()),
        volume: s
            .audio
            .map(|l| (volume_icon(l).to_owned(), l.percent.min(100), l.muted)),
        outputs: s
            .devices
            .as_ref()
            .map(|d| rows(&d.outputs, d.default_output))
            .unwrap_or_default(),
        microphone,
        inputs: s
            .devices
            .as_ref()
            .map(|d| rows(&d.inputs, d.default_input))
            .unwrap_or_default(),
        brightness: s
            .brightness
            .as_ref()
            .map(|b| brightness_percent(b.raw, b.max)),
        network,
        battery,
        battery_percent: s.battery.as_ref().map(|b| b.percent),
        wifi: if s.network.as_ref().and_then(|n| n.wifi_enabled) == Some(true) {
            wifi_rows(&s.wifi_networks)
        } else {
            Vec::new()
        },
        bluetooth: s.bluetooth.as_ref().map(bluetooth_tile),
        power: s.power.as_ref().map(power_tile),
        keyboard,
        load: s.load.map(|l| {
            format!(
                "CPU {} % · Memory {} of {}",
                l.cpu_percent,
                ByteSize(l.mem_used).human(),
                ByteSize(l.mem_total).human()
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_like_gnome() {
        let level = |percent, muted| Level { percent, muted };
        assert_eq!(volume_icon(level(50, true)), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(level(0, false)), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(level(20, false)), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(level(120, false)), "audio-volume-high-symbolic");
        let net = |primary, strength, wifi| Network {
            primary,
            strength,
            wifi_enabled: wifi,
        };
        assert_eq!(
            network_icon(&net(
                Some((NetKind::Wifi, "home".into())),
                Some(70),
                Some(true)
            )),
            "network-wireless-signal-good-symbolic"
        );
        assert_eq!(
            network_icon(&net(Some((NetKind::Wired, "eth".into())), None, None)),
            "network-wired-symbolic"
        );
        assert_eq!(
            network_icon(&net(None, None, Some(false))),
            "network-wireless-disabled-symbolic"
        );
        assert_eq!(
            network_icon(&net(None, None, None)),
            "network-offline-symbolic"
        );
    }

    #[test]
    fn cpu_and_memory_from_proc() {
        let before = parse_cpu("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3\n").unwrap();
        let now = parse_cpu("cpu  200 0 100 1100 100 0 0 0 0 0\n").unwrap();
        assert_eq!(before, (150, 1000));
        assert_eq!(cpu_percent(before, now), 30);
        assert_eq!(cpu_percent(now, now), 0);
        let mem = parse_meminfo(
            "MemTotal:       32000000 kB\nMemFree:  1 kB\nMemAvailable:   20000000 kB\n",
        );
        assert_eq!(mem, Some((12_000_000 * 1024, 32_000_000 * 1024)));
        assert_eq!(parse_meminfo("nothing"), None);
    }

    #[test]
    fn keyboard_from_hyprland() {
        let json = r#"{"mice":[],"keyboards":[
            {"name":"virtual","layout":"us","active_keymap":"English (US)","main":false},
            {"name":"at","layout":"us,de","active_keymap":"German","active_layout_index":1,"main":true}]}"#;
        assert_eq!(
            parse_keyboard(json),
            Some(Keyboard {
                layouts: vec!["us".into(), "de".into()],
                active: 1,
                name: "German".into()
            })
        );
        assert_eq!(parse_keyboard("{}"), None);
    }

    #[test]
    fn wifi_menu_as_gnome_orders_it() {
        let net = |ssid: &str, strength, known, active| WifiNetwork {
            ssid: ssid.into(),
            strength,
            secured: ssid != "cafe",
            known,
            active,
        };
        let rows = wifi_rows(&[
            net("cafe", 90, false, false),
            net("home", 40, true, false),
            net("office", 30, false, true),
            net("cafe", 50, false, false),
        ]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["office", "home", "cafe"]);
        assert!(rows[0].checked);
        assert_eq!(rows[2].icon, "network-wireless-signal-excellent-symbolic");
        assert_eq!(rows[2].suffix, None);
        assert_eq!(rows[1].suffix, Some("network-wireless-encrypted-symbolic"));
    }

    #[test]
    fn power_and_bluetooth_tiles() {
        let p = PowerProfiles {
            active: "power-saver".into(),
            available: vec!["power-saver".into(), "balanced".into()],
        };
        let (icon, title, subtitle, on, rows) = power_tile(&p);
        assert_eq!(
            (title.as_str(), subtitle.as_str(), on),
            ("Power Mode", "Power Saver", true)
        );
        assert_eq!(icon, "power-profile-power-saver-symbolic");
        assert_eq!(rows.len(), 2, "only what the machine offers");
        let b = Bluetooth {
            powered: true,
            devices: vec![
                BtDevice {
                    path: "/a".into(),
                    name: "Headphones".into(),
                    icon: "audio-headphones".into(),
                    connected: true,
                },
                BtDevice {
                    path: "/b".into(),
                    name: "Mouse".into(),
                    icon: String::new(),
                    connected: false,
                },
            ],
        };
        let (_, _, subtitle, on, rows) = bluetooth_tile(&b);
        assert_eq!((subtitle.as_str(), on), ("Headphones", true));
        assert_eq!(rows[0].icon, "audio-headphones-symbolic");
        let off = bluetooth_tile(&Bluetooth {
            powered: false,
            ..b
        });
        assert_eq!(
            (off.0.as_str(), off.4.len()),
            ("bluetooth-disabled-symbolic", 0)
        );
    }

    #[test]
    fn the_view() {
        let view = quick(
            &System {
                microphone: None,
                devices: None,
                wifi_networks: Vec::new(),
                bluetooth: None,
                power: None,
                audio: Some(Level {
                    percent: 45,
                    muted: false,
                }),
                brightness: Some(Brightness {
                    device: "intel_backlight".into(),
                    raw: 1000,
                    max: 1000,
                }),
                network: Some(Network {
                    primary: Some((NetKind::Wifi, "home".into())),
                    strength: Some(90),
                    wifi_enabled: Some(true),
                }),
                battery: Some(Battery {
                    percent: 87,
                    charging: true,
                    icon: "battery-level-90-charging-symbolic".into(),
                }),
                keyboard: Some(Keyboard {
                    layouts: vec!["us".into(), "de".into()],
                    active: 0,
                    name: "English (US)".into(),
                }),
                load: Some(Load {
                    cpu_percent: 12,
                    mem_used: 6 << 30,
                    mem_total: 32 << 30,
                }),
            },
            false,
        );
        assert_eq!(
            view.indicators,
            [
                "network-wireless-signal-excellent-symbolic",
                "audio-volume-medium-symbolic",
                "battery-level-90-charging-symbolic"
            ]
        );
        assert_eq!(view.layout, Some("us".into()));
        assert_eq!(view.brightness, Some(100));
        assert_eq!(
            view.network,
            Some((
                "network-wireless-signal-excellent-symbolic".into(),
                "Wi-Fi".into(),
                "home".into(),
                Some(true)
            ))
        );
        assert_eq!(view.battery.unwrap().1, "87 % – charging");
        assert_eq!(view.load.unwrap(), "CPU 12 % · Memory 6.0 GiB of 32 GiB");
    }

    #[test]
    fn a_desktop_without_battery_and_one_layout() {
        let view = quick(
            &System {
                keyboard: Some(Keyboard {
                    layouts: vec!["us".into()],
                    active: 0,
                    name: "English (US)".into(),
                }),
                ..System::default()
            },
            false,
        );
        assert_eq!((view.layout, view.battery), (None, None));
        assert_eq!(view.indicators, Vec::<String>::new());
    }
}
