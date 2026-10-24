//! `koje media` (ADR 0041): the volume, microphone and brightness keys change
//! the level and show it as an OSD; the media keys go to kojed, which knows
//! the most recently active player. The decisions are in `koje_core::media`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use koje_core::media::{self, Backlight, Level, Osd, PlayerAction};

use crate::cli::MediaAction;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";
const BACKLIGHTS: &str = "/sys/class/backlight";
/// How long the OSD stays.
const OSD_MS: i32 = 1500;
const TIMEOUT: Duration = Duration::from_secs(3);

pub fn run(action: MediaAction) -> Result<()> {
    let osd = match action {
        MediaAction::VolumeUp => {
            wpctl(&["set-volume", "-l", "1", SINK, "5%+"])?;
            media::volume_osd(level(SINK)?)
        }
        MediaAction::VolumeDown => {
            wpctl(&["set-volume", SINK, "5%-"])?;
            media::volume_osd(level(SINK)?)
        }
        MediaAction::Mute => {
            wpctl(&["set-mute", SINK, "toggle"])?;
            media::volume_osd(level(SINK)?)
        }
        MediaAction::MicMute => {
            wpctl(&["set-mute", SOURCE, "toggle"])?;
            media::microphone_osd(level(SOURCE)?)
        }
        MediaAction::BrightnessUp => brightness(true)?,
        MediaAction::BrightnessDown => brightness(false)?,
        MediaAction::PlayPause => return player(PlayerAction::PlayPause),
        MediaAction::Next => return player(PlayerAction::Next),
        MediaAction::Previous => return player(PlayerAction::Previous),
    };
    // The level is set; a missing notification daemon only costs the OSD.
    if let Err(e) = show(&osd) {
        tracing::warn!("showing the OSD: {e:#}");
    }
    Ok(())
}

/// kojed knows the most recently active player, also in the contexts.
fn player(action: PlayerAction) -> Result<()> {
    let player: String = crate::kojed::call("MediaPlayer", &(action.method(),))?;
    if player.is_empty() {
        println!("no media player");
    } else {
        println!("{}: {player}", action.method());
    }
    Ok(())
}

fn wpctl(args: &[&str]) -> Result<String> {
    let out = Command::new("wpctl")
        .args(args)
        .output()
        .context("running wpctl (package wireplumber)")?;
    if !out.status.success() {
        bail!(
            "wpctl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn level(node: &str) -> Result<Level> {
    let out = wpctl(&["get-volume", node])?;
    media::parse_wpctl_volume(&out)
        .with_context(|| format!("unexpected answer from wpctl get-volume: {out:?}"))
}

fn read_number(path: &Path) -> Result<u32> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.trim()
        .parse()
        .with_context(|| format!("{}: not a number: {text:?}", path.display()))
}

/// One step on the panel's backlight, set through logind like GNOME does: the
/// owner of the active session may, without the group `video`.
fn brightness(up: bool) -> Result<Osd> {
    let root = Path::new(BACKLIGHTS);
    let mut devices = Vec::new();
    for entry in std::fs::read_dir(root).with_context(|| format!("reading {BACKLIGHTS}"))? {
        let entry = entry?;
        let kind = std::fs::read_to_string(entry.path().join("type")).unwrap_or_default();
        devices.push(Backlight {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: kind.trim().to_owned(),
        });
    }
    let Some(device) = media::pick_backlight(&devices) else {
        bail!("no backlight in {BACKLIGHTS}");
    };
    let dir = root.join(&device.name);
    let max = read_number(&dir.join("max_brightness"))?;
    let raw = read_number(&dir.join("brightness"))?;
    let next = media::brightness_step(raw, max, up);
    block_on(async {
        let conn = zbus::Connection::system()
            .await
            .context("connecting to the system bus")?;
        conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &("backlight", device.name.as_str(), next),
        )
        .await
        .with_context(|| format!("setting the brightness of {} through logind", device.name))?;
        Ok(())
    })?;
    Ok(media::brightness_osd(media::brightness_percent(next, max)))
}

/// The OSD as a notification. The `synchronous` hint makes it replace the
/// previous one; `replaces_id` with that hint crashes mako 1.10.
fn show(osd: &Osd) -> Result<()> {
    block_on(async {
        use zbus::zvariant::Value;
        let conn = zbus::Connection::session()
            .await
            .context("connecting to the session bus")?;
        let value = i32::try_from(osd.value).unwrap_or(100);
        let hints: std::collections::HashMap<&str, Value<'_>> = [
            ("value", Value::from(value)),
            ("x-canonical-private-synchronous", Value::from("koje-osd")),
            // Low urgency; not kept in the notification history.
            ("urgency", Value::from(0u8)),
            ("transient", Value::from(true)),
        ]
        .into_iter()
        .collect();
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "koje",
                0u32,
                osd.icon,
                osd.summary.as_str(),
                "",
                Vec::<&str>::new(),
                hints,
                OSD_MS,
            ),
        )
        .await
        .context("sending the OSD notification")?;
        Ok(())
    })
}

fn block_on<T>(f: impl Future<Output = Result<T>>) -> Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the D-Bus client")?;
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, f)
            .await
            .context("no answer on D-Bus in time")?
    })
}
