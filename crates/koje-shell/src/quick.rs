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
    pub brightness: Option<Brightness>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
    pub keyboard: Option<Keyboard>,
    pub load: Option<Load>,
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
    /// Perceived level 0–100.
    pub brightness: Option<u32>,
    /// `(icon, title, subtitle, wifi on)`; `wifi on` is `None` without
    /// Wi-Fi hardware.
    pub network: Option<(String, String, String, Option<bool>)>,
    /// `(icon, 87 %, charging)`.
    pub battery: Option<(String, String)>,
    /// `(short, long, several layouts)`.
    pub keyboard: Option<(String, String, bool)>,
    /// `CPU 12 % · Memory 6.1 GiB of 31.2 GiB`.
    pub load: Option<String>,
}

pub fn quick(s: &System) -> QuickView {
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
        brightness: s
            .brightness
            .as_ref()
            .map(|b| brightness_percent(b.raw, b.max)),
        network,
        battery,
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
    fn the_view() {
        let view = quick(&System {
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
        });
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
        let view = quick(&System {
            keyboard: Some(Keyboard {
                layouts: vec!["us".into()],
                active: 0,
                name: "English (US)".into(),
            }),
            ..System::default()
        });
        assert_eq!((view.layout, view.battery), (None, None));
        assert_eq!(view.indicators, Vec::<String>::new());
    }
}
