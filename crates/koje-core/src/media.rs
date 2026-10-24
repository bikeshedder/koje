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

/// What the OSD shows: one replacing notification with a progress value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Osd {
    pub summary: String,
    pub icon: &'static str,
    /// 0–100, the bar in the notification.
    pub value: u32,
}

pub fn volume_osd(level: Level) -> Osd {
    let icon = match level.percent {
        _ if level.muted => "audio-volume-muted",
        0 => "audio-volume-muted",
        1..=33 => "audio-volume-low",
        34..=66 => "audio-volume-medium",
        _ => "audio-volume-high",
    };
    Osd {
        summary: if level.muted {
            "Volume muted".to_owned()
        } else {
            format!("Volume {} %", level.percent)
        },
        icon,
        value: if level.muted {
            0
        } else {
            level.percent.min(100)
        },
    }
}

pub fn microphone_osd(level: Level) -> Osd {
    Osd {
        summary: if level.muted {
            "Microphone muted".to_owned()
        } else {
            format!("Microphone on, {} %", level.percent)
        },
        icon: if level.muted {
            "microphone-sensitivity-muted"
        } else {
            "microphone-sensitivity-high"
        },
        value: if level.muted {
            0
        } else {
            level.percent.min(100)
        },
    }
}

pub fn brightness_osd(percent: u32) -> Osd {
    Osd {
        summary: format!("Brightness {percent} %"),
        icon: "display-brightness",
        value: percent.min(100),
    }
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
        let osd = volume_osd(Level {
            percent: 45,
            muted: false,
        });
        assert_eq!(osd.summary, "Volume 45 %");
        assert_eq!(osd.icon, "audio-volume-medium");
        assert_eq!(osd.value, 45);
        let muted = volume_osd(Level {
            percent: 45,
            muted: true,
        });
        assert_eq!((muted.summary.as_str(), muted.value), ("Volume muted", 0));
        assert_eq!(
            microphone_osd(Level {
                percent: 80,
                muted: true
            })
            .icon,
            "microphone-sensitivity-muted"
        );
        assert_eq!(brightness_osd(60).summary, "Brightness 60 %");
    }
}
