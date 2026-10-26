//! Volume, brightness and media keys (ADR 0041): the decisions behind
//! `koje media`. The CLI runs `wpctl`, reads `/sys/class/backlight` and calls
//! logind; what to set and what the OSD says is decided here.

/// A sound level as `wpctl get-volume` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub percent: u32,
    pub muted: bool,
}

/// `Volume: 0.45` or `Volume: 0.45 [MUTED]` (`wpctl get-volume`).
pub fn parse_wpctl_volume(output: &str) -> Option<Level> {
    let rest = output.trim().strip_prefix("Volume:")?.trim();
    let (number, flags) = rest.split_once(' ').unwrap_or((rest, ""));
    let volume: f64 = number.parse().ok()?;
    if !volume.is_finite() || volume < 0.0 {
        return None;
    }
    Some(Level {
        percent: to_u32(volume * 100.0),
        muted: flags.contains("[MUTED]"),
    })
}

/// Rounded and kept within `u32`'s range.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to u32's range first"
)]
fn to_u32(x: f64) -> u32 {
    x.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// A backlight device: a folder in `/sys/class/backlight` and its `type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlight {
    pub name: String,
    pub kind: String,
}

/// The device to set, as GNOME picks it: firmware before platform before raw
/// (a raw one may not be the panel's), then by name for a stable choice.
pub fn pick_backlight(devices: &[Backlight]) -> Option<&Backlight> {
    let rank = |kind: &str| match kind {
        "firmware" => 0,
        "platform" => 1,
        "raw" => 2,
        _ => 3,
    };
    devices
        .iter()
        .min_by(|a, b| (rank(&a.kind), &a.name).cmp(&(rank(&b.kind), &b.name)))
}

/// Brightness feels even on a scale where the raw value grows with the
/// fourth power (as `brightnessctl -e4`).
const EXPONENT: f64 = 4.0;
/// Steps of 5 % on that scale.
const STEPS: f64 = 20.0;

/// The perceived level (0–100) of a raw brightness.
pub fn brightness_percent(raw: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let level = (f64::from(raw.min(max)) / f64::from(max)).powf(1.0 / EXPONENT);
    to_u32(level * 100.0)
}

/// The raw brightness for a perceived level (0–100), as a slider sets it –
/// the inverse of [`brightness_percent`]; never below the lowest value
/// [`brightness_step`] goes to.
pub fn brightness_raw(percent: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let lowest = (max / 100).max(1);
    let level = f64::from(percent.min(100)) / 100.0;
    to_u32(level.powf(EXPONENT) * f64::from(max)).clamp(lowest, max)
}

/// The raw brightness one step up or down from `raw`. The screen never goes
/// fully dark: the lowest value is 1 % of `max` (at least 1).
pub fn brightness_step(raw: u32, max: u32, up: bool) -> u32 {
    if max == 0 {
        return 0;
    }
    let lowest = (max / 100).max(1);
    let level = (f64::from(raw.min(max)) / f64::from(max)).powf(1.0 / EXPONENT);
    // On the grid of steps, so repeated presses land on the same values.
    let step = (level * STEPS).round() + if up { 1.0 } else { -1.0 };
    let target = (step / STEPS).clamp(0.0, 1.0).powf(EXPONENT) * f64::from(max);
    let mut next = to_u32(target).clamp(lowest, max);
    // Near the bottom a step can round to the same value: move at least by one.
    if next == raw {
        next = if up {
            (raw + 1).min(max)
        } else {
            raw.saturating_sub(1).max(lowest)
        };
    }
    next
}

/// What the OSD shows, as GNOME's: an icon, the device's name for sound
/// and a level bar, which goes up to 150 % when the volume is above 100 %.
/// `summary` is the text when it falls back to a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Osd {
    /// A symbolic icon name.
    pub icon: String,
    pub label: Option<String>,
    /// Percent; `None`: the icon alone.
    pub level: Option<u32>,
    /// The bar's end: 100, or 150 when the level is above 100.
    pub max: u32,
    pub summary: String,
}

impl Osd {
    /// For D-Bus (the shell's `osd` action): no label is `""`, no level `-1`.
    pub fn to_wire(&self) -> (String, String, i32, u32) {
        (
            self.icon.clone(),
            self.label.clone().unwrap_or_default(),
            self.level
                .map_or(-1, |l| i32::try_from(l).unwrap_or(i32::MAX)),
            self.max,
        )
    }

    pub fn from_wire((icon, label, level, max): (String, String, i32, u32)) -> Self {
        let level = u32::try_from(level).ok();
        Self {
            summary: label.clone(),
            icon,
            label: Some(label).filter(|l| !l.is_empty()),
            level,
            max: max.max(1),
        }
    }

    /// Where the bar's parts end, as fractions of its width: the level up
    /// to 100 %, the level beyond it (GNOME's overdrive, red), and the mark
    /// at 100 % when the bar goes further.
    pub fn bar(&self) -> Option<Bar> {
        let level = self.level?;
        let max = f64::from(self.max.max(1));
        let full = (f64::from(self.max.min(100)) / max).min(1.0);
        let at = (f64::from(level) / max).clamp(0.0, 1.0);
        Some(Bar {
            normal: at.min(full),
            overdrive: at,
            mark: (self.max > 100).then_some(full),
        })
    }
}

/// See [`Osd::bar`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub normal: f64,
    pub overdrive: f64,
    pub mark: Option<f64>,
}

/// The bar's end for a sound level: GNOME's 150 % above 100 %.
fn sound_max(level: Level) -> u32 {
    if level.percent > 100 { 150 } else { 100 }
}

/// `label`: the output's name (`wpctl inspect`, [`parse_wpctl_description`]).
pub fn volume_osd(level: Level, label: Option<String>) -> Osd {
    let icon = match level.percent {
        _ if level.muted => "audio-volume-muted-symbolic",
        0 => "audio-volume-muted-symbolic",
        1..=33 => "audio-volume-low-symbolic",
        34..=66 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    };
    Osd {
        summary: if level.muted {
            "Volume muted".to_owned()
        } else {
            format!("Volume {} %", level.percent)
        },
        icon: icon.to_owned(),
        label,
        level: Some(if level.muted { 0 } else { level.percent }),
        max: sound_max(level),
    }
}

pub fn microphone_osd(level: Level, label: Option<String>) -> Osd {
    let icon = match level.percent {
        _ if level.muted => "microphone-sensitivity-muted-symbolic",
        0 => "microphone-sensitivity-muted-symbolic",
        1..=33 => "microphone-sensitivity-low-symbolic",
        34..=66 => "microphone-sensitivity-medium-symbolic",
        _ => "microphone-sensitivity-high-symbolic",
    };
    Osd {
        summary: if level.muted {
            "Microphone muted".to_owned()
        } else {
            format!("Microphone on, {} %", level.percent)
        },
        icon: icon.to_owned(),
        label,
        level: Some(if level.muted { 0 } else { level.percent }),
        max: sound_max(level),
    }
}

pub fn brightness_osd(percent: u32) -> Osd {
    Osd {
        summary: format!("Brightness {percent} %"),
        icon: "display-brightness-symbolic".to_owned(),
        label: None,
        level: Some(percent.min(100)),
        max: 100,
    }
}

/// The node's name from `wpctl inspect`: `* node.description = "…"`.
pub fn parse_wpctl_description(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line
            .trim_start_matches([' ', '*'])
            .strip_prefix("node.description")?
            .trim_start()
            .strip_prefix('=')?
            .trim();
        let name = rest.strip_prefix('"')?.strip_suffix('"')?;
        Some(name.to_owned()).filter(|n| !n.is_empty())
    })
}

/// What a media key asks of a player (MPRIS `org.mpris.MediaPlayer2.Player`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerAction {
    PlayPause,
    Next,
    Previous,
}

impl PlayerAction {
    /// The MPRIS method, also the name in kojed's `MediaPlayer` call.
    pub fn method(self) -> &'static str {
        match self {
            Self::PlayPause => "PlayPause",
            Self::Next => "Next",
            Self::Previous => "Previous",
        }
    }

    pub fn from_method(method: &str) -> Option<Self> {
        [Self::PlayPause, Self::Next, Self::Previous]
            .into_iter()
            .find(|a| a.method() == method)
    }
}

/// MPRIS `PlaybackStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

impl Playback {
    pub fn parse(status: &str) -> Option<Self> {
        match status {
            "Playing" => Some(Self::Playing),
            "Paused" => Some(Self::Paused),
            "Stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// The players to try for a media key, best first (ADR 0041): the most
/// recently active one (the last that started playing), then playing ones,
/// then paused, then stopped ones – in the given order within each.
pub fn player_order<K: Clone + PartialEq>(last: Option<&K>, players: &[(K, Playback)]) -> Vec<K> {
    let mut order: Vec<&(K, Playback)> = players.iter().collect();
    // Stable: the given order stays within each group.
    order.sort_by_key(|(key, playback)| (Some(key) != last, *playback));
    order.into_iter().map(|(key, _)| key.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_from_a_slider() {
        assert_eq!(brightness_raw(100, 1000), 1000);
        assert_eq!(brightness_raw(0, 1000), 10, "never fully dark");
        assert_eq!(brightness_raw(50, 1000), 63);
        assert_eq!(brightness_raw(200, 1000), 1000);
        assert_eq!(brightness_raw(50, 0), 0);
        // Above the floor (1 % of max is 32 % perceived) it round-trips.
        for p in [35, 70, 90] {
            assert_eq!(brightness_percent(brightness_raw(p, 100_000), 100_000), p);
        }
    }

    #[test]
    fn reads_wpctl() {
        assert_eq!(
            parse_wpctl_volume("Volume: 0.45\n"),
            Some(Level {
                percent: 45,
                muted: false
            })
        );
        assert_eq!(
            parse_wpctl_volume("Volume: 1.00 [MUTED]"),
            Some(Level {
                percent: 100,
                muted: true
            })
        );
        assert_eq!(
            parse_wpctl_volume("Volume: 0.29"),
            Some(Level {
                percent: 29,
                muted: false
            })
        );
        assert_eq!(parse_wpctl_volume(""), None);
        assert_eq!(parse_wpctl_volume("Volume: x"), None);
        assert_eq!(parse_wpctl_volume("Volume: -1"), None);
    }

    fn bl(name: &str, kind: &str) -> Backlight {
        Backlight {
            name: name.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn firmware_backlight_first() {
        let devices = [bl("amdgpu_bl2", "raw"), bl("acpi_video0", "firmware")];
        assert_eq!(pick_backlight(&devices).unwrap().name, "acpi_video0");
        assert_eq!(
            pick_backlight(&[bl("amdgpu_bl2", "raw")]).unwrap().name,
            "amdgpu_bl2"
        );
        assert_eq!(pick_backlight(&[]), None);
    }

    #[test]
    fn brightness_steps_are_even_and_reversible() {
        let max = 62194;
        let mut raw = max;
        let mut seen = vec![brightness_percent(raw, max)];
        for _ in 0..30 {
            raw = brightness_step(raw, max, false);
            seen.push(brightness_percent(raw, max));
        }
        // Down in 5 % steps until the floor of 1 % raw (≈ 32 % perceived).
        assert_eq!(&seen[..4], &[100, 95, 90, 85]);
        assert_eq!(raw, max / 100);
        // Up again from the floor lands back on the grid and at the top.
        for _ in 0..30 {
            raw = brightness_step(raw, max, true);
        }
        assert_eq!(raw, max);
    }

    #[test]
    fn brightness_moves_on_small_ranges() {
        // Few raw values: every press still changes something.
        assert_eq!(brightness_step(1, 7, true), 2);
        assert_eq!(brightness_step(2, 7, false), 1);
        assert_eq!(brightness_step(1, 7, false), 1);
        assert_eq!(brightness_step(7, 7, true), 7);
        assert_eq!(brightness_step(0, 0, true), 0);
    }

    #[test]
    fn last_active_player_first() {
        use Playback::*;
        let players = [("mpv", Stopped), ("firefox", Paused), ("spotify", Playing)];
        assert_eq!(
            player_order(Some(&"firefox"), &players),
            ["firefox", "spotify", "mpv"]
        );
        // Nothing recorded (kojed just started) or the last one is gone:
        // playing before paused before stopped.
        assert_eq!(player_order(None, &players), ["spotify", "firefox", "mpv"]);
        assert_eq!(
            player_order(Some(&"vlc"), &players),
            ["spotify", "firefox", "mpv"]
        );
        assert_eq!(player_order::<&str>(None, &[]), Vec::<&str>::new());
    }

    #[test]
    fn player_actions_and_status() {
        for a in [
            PlayerAction::PlayPause,
            PlayerAction::Next,
            PlayerAction::Previous,
        ] {
            assert_eq!(PlayerAction::from_method(a.method()), Some(a));
        }
        assert_eq!(PlayerAction::from_method("Quit"), None);
        assert_eq!(Playback::parse("Playing"), Some(Playback::Playing));
        assert_eq!(Playback::parse("playing"), None);
    }

    #[test]
    fn osd_texts() {
        let level = |percent, muted| Level { percent, muted };
        let osd = volume_osd(level(45, false), Some("Speakers".to_owned()));
        assert_eq!(osd.summary, "Volume 45 %");
        assert_eq!(osd.icon, "audio-volume-medium-symbolic");
        assert_eq!((osd.level, osd.max), (Some(45), 100));
        assert_eq!(osd.label.as_deref(), Some("Speakers"));
        let muted = volume_osd(level(45, true), None);
        assert_eq!(
            (muted.summary.as_str(), muted.level),
            ("Volume muted", Some(0))
        );
        assert_eq!(muted.icon, "audio-volume-muted-symbolic");
        assert_eq!(volume_osd(level(120, false), None).max, 150);
        assert_eq!(
            microphone_osd(level(80, true), None).icon,
            "microphone-sensitivity-muted-symbolic"
        );
        assert_eq!(brightness_osd(60).summary, "Brightness 60 %");
        assert_eq!(brightness_osd(60).label, None);
    }

    #[test]
    fn osd_bar_as_gnome() {
        let level = |percent, muted| Level { percent, muted };
        let bar = volume_osd(level(50, false), None).bar();
        assert_eq!(
            bar,
            Some(Bar {
                normal: 0.5,
                overdrive: 0.5,
                mark: None
            })
        );
        // Beyond 100 %: the bar ends at 150 %, the rest is overdrive.
        let bar = volume_osd(level(120, false), None).bar().unwrap();
        assert!((bar.normal - 100.0 / 150.0).abs() < 1e-9);
        assert!((bar.overdrive - 0.8).abs() < 1e-9);
        assert_eq!(bar.mark, Some(bar.normal));
        let icon_only = Osd {
            level: None,
            ..brightness_osd(10)
        };
        assert_eq!(icon_only.bar(), None);
    }

    #[test]
    fn osd_over_dbus() {
        let osd = volume_osd(
            Level {
                percent: 30,
                muted: false,
            },
            Some("HDMI".to_owned()),
        );
        let back = Osd::from_wire(osd.to_wire());
        assert_eq!(
            (back.icon, back.label, back.level, back.max),
            (osd.icon, osd.label, osd.level, osd.max)
        );
        let bare = Osd::from_wire(("x".to_owned(), String::new(), -1, 0));
        assert_eq!((bare.label, bare.level, bare.max), (None, None, 1));
    }

    #[test]
    fn device_names_from_wpctl() {
        let out = "id 52, type PipeWire:Interface:Node\n    alsa.card = \"0\"\n  * node.description = \"Built-in Audio Analog Stereo\"\n  * node.name = \"alsa_output\"\n";
        assert_eq!(
            parse_wpctl_description(out).as_deref(),
            Some("Built-in Audio Analog Stereo")
        );
        assert_eq!(
            parse_wpctl_description("  * node.description = \"\"\n"),
            None
        );
        assert_eq!(parse_wpctl_description("nothing"), None);
    }
}
