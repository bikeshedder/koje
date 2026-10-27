//! What Quick Settings reads from the system (M8.30) and the changes it
//! makes: sound with `wpctl` (as `koje media`, ADR 0041), the backlight
//! through `/sys` and logind, `NetworkManager` and `UPower` on the system bus,
//! the keyboard layout from Hyprland, CPU and memory from `/proc`. Read every
//! few seconds and right after a change; every call has a timeout.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use async_channel::Sender;
use koje_core::media::{self, Backlight, Level};
use koje_hyprland::Hyprland;
use tokio::sync::Notify;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::model::Update;
use crate::quick::{self, Battery, Brightness, Keyboard, Load, NetKind, Network, System};

const EVERY: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(2);
const BACKLIGHTS: &str = "/sys/class/backlight";
const SINK: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";

/// A change from Quick Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Percent.
    Volume(u32),
    /// Percent up (positive) or down, from scrolling over the indicator.
    VolumeStep(i32),
    ToggleMute,
    /// The default input's level in percent (M8.84).
    MicrophoneVolume(u32),
    ToggleMicrophoneMute,
    /// Make this `PipeWire` node the default output or input (M8.84).
    DefaultDevice(u32),
    /// Perceived percent.
    Brightness(u32),
    Wifi(bool),
    NextLayout,
}

/// Read the system every [`EVERY`] and when `wake` is notified.
pub async fn watch(
    hypr: &Hyprland,
    conn: Option<zbus::Connection>,
    tx: Sender<Update>,
    wake: Arc<Notify>,
) {
    let mut cpu_before = None;
    loop {
        let cpu_now = std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|s| quick::parse_cpu(&s));
        let load = match (cpu_before, cpu_now) {
            (Some(b), Some(n)) => std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|m| quick::parse_meminfo(&m))
                .map(|(used, total)| Load {
                    cpu_percent: quick::cpu_percent(b, n),
                    mem_used: used,
                    mem_total: total,
                }),
            _ => None,
        };
        cpu_before = cpu_now;
        let system = System {
            audio: within(volume()).await,
            microphone: within(level(SOURCE)).await,
            devices: within(devices()).await,
            brightness: brightness().ok(),
            network: match &conn {
                Some(c) => within(network(c)).await,
                None => None,
            },
            battery: match &conn {
                Some(c) => within(battery(c)).await,
                None => None,
            },
            keyboard: within(keyboard(hypr)).await,
            load,
        };
        if tx.send(Update::System(system)).await.is_err() {
            return;
        }
        tokio::select! {
            () = tokio::time::sleep(EVERY) => {}
            () = wake.notified() => {}
        }
    }
}

/// `None` on an error or a timeout: Quick Settings leaves it out.
async fn within<T>(f: impl Future<Output = Result<T>>) -> Option<T> {
    match tokio::time::timeout(TIMEOUT, f).await {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            tracing::trace!("quick settings: {e:#}");
            None
        }
        Err(_) => None,
    }
}

/// Make a change, then wake the reader. Scrolling over the volume indicator
/// shows the OSD, as in GNOME: what to show.
pub async fn change(
    hypr: &Hyprland,
    conn: Option<&zbus::Connection>,
    c: Change,
) -> Option<media::Osd> {
    let done = match c {
        Change::Volume(p) => wpctl(&["set-volume", SINK, &format!("{}%", p.min(150))])
            .await
            .map(drop),
        Change::VolumeStep(d) => volume_step(d).await,
        Change::ToggleMute => wpctl(&["set-mute", SINK, "toggle"]).await.map(drop),
        Change::MicrophoneVolume(p) => wpctl(&["set-volume", SOURCE, &format!("{}%", p.min(150))])
            .await
            .map(drop),
        Change::ToggleMicrophoneMute => wpctl(&["set-mute", SOURCE, "toggle"]).await.map(drop),
        Change::DefaultDevice(id) => wpctl(&["set-default", &id.to_string()]).await.map(drop),
        Change::Brightness(p) => match conn {
            Some(conn) => set_brightness(conn, p).await,
            None => Err(anyhow::anyhow!("no system bus")),
        },
        Change::Wifi(on) => match conn {
            Some(conn) => set_wifi(conn, on).await,
            None => Err(anyhow::anyhow!("no system bus")),
        },
        Change::NextLayout => hypr
            .request("switchxkblayout all next")
            .await
            .map(drop)
            .map_err(anyhow::Error::from),
    };
    if let Err(e) = done {
        tracing::warn!("quick settings: {c:?}: {e:#}");
        return None;
    }
    match c {
        // GNOME's indicator shows no device name.
        Change::VolumeStep(_) => within(volume()).await.map(|l| media::volume_osd(l, None)),
        _ => None,
    }
}

/// Up to 100 % at most and unmuted, down to 0 %, as GNOME's slider.
async fn volume_step(d: i32) -> Result<()> {
    let by = format!("{}%{}", d.unsigned_abs(), if d > 0 { "+" } else { "-" });
    if d > 0 {
        wpctl(&["set-volume", "-l", "1", SINK, &by]).await?;
        wpctl(&["set-mute", SINK, "0"]).await?;
    } else {
        wpctl(&["set-volume", SINK, &by]).await?;
    }
    Ok(())
}

async fn wpctl(args: &[&str]) -> Result<String> {
    let out = tokio::time::timeout(
        TIMEOUT,
        tokio::process::Command::new("wpctl")
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("wpctl did not answer")?
    .context("running wpctl")?;
    if !out.status.success() {
        bail!(
            "wpctl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn volume() -> Result<Level> {
    level(SINK).await
}

async fn level(node: &str) -> Result<Level> {
    let out = wpctl(&["get-volume", node]).await?;
    media::parse_wpctl_volume(&out).context("wpctl's volume")
}

/// Outputs, inputs and whether an app records, from `pw-dump`.
async fn devices() -> Result<crate::sound::Devices> {
    let out = tokio::time::timeout(
        TIMEOUT,
        tokio::process::Command::new("pw-dump")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("pw-dump did not answer")?
    .context("running pw-dump")?;
    crate::sound::parse_pw_dump(&String::from_utf8_lossy(&out.stdout)).context("pw-dump's output")
}

fn read_number(path: &Path) -> Result<u32> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.trim()
        .parse()
        .with_context(|| format!("{}: not a number", path.display()))
}

/// The backlight GNOME would pick (`koje_core::media::pick_backlight`).
fn brightness() -> Result<Brightness> {
    let mut devices = Vec::new();
    for entry in std::fs::read_dir(BACKLIGHTS)? {
        let entry = entry?;
        let kind = std::fs::read_to_string(entry.path().join("type")).unwrap_or_default();
        devices.push(Backlight {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: kind.trim().to_owned(),
        });
    }
    let device = media::pick_backlight(&devices).context("no backlight")?;
    let dir = Path::new(BACKLIGHTS).join(&device.name);
    Ok(Brightness {
        device: device.name.clone(),
        raw: read_number(&dir.join("brightness"))?,
        max: read_number(&dir.join("max_brightness"))?,
    })
}

/// Through logind like `koje media` and GNOME: no group `video` needed.
async fn set_brightness(conn: &zbus::Connection, percent: u32) -> Result<()> {
    let b = brightness()?;
    let raw = media::brightness_raw(percent, b.max);
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/session/auto",
        Some("org.freedesktop.login1.Session"),
        "SetBrightness",
        &("backlight", b.device.as_str(), raw),
    )
    .await
    .context("logind SetBrightness")?;
    Ok(())
}

async fn prop<T>(
    conn: &zbus::Connection,
    dest: &str,
    path: &str,
    iface: &str,
    name: &str,
) -> Result<T>
where
    T: TryFrom<OwnedValue>,
    <T as TryFrom<OwnedValue>>::Error: Into<zbus::zvariant::Error>,
{
    let props = zbus::fdo::PropertiesProxy::builder(conn)
        .destination(dest.to_owned())?
        .path(path.to_owned())?
        .build()
        .await?;
    let value = props.get(iface.try_into()?, name).await?;
    T::try_from(value)
        .map_err(Into::into)
        .with_context(|| format!("{iface}.{name}"))
}

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// `NM_DEVICE_TYPE_WIFI`.
const DEVICE_WIFI: u32 = 2;

async fn network(conn: &zbus::Connection) -> Result<Network> {
    let devices: Vec<OwnedObjectPath> = prop(conn, NM, NM_PATH, NM, "Devices").await?;
    let mut has_wifi = false;
    for d in &devices {
        let kind: u32 = prop(
            conn,
            NM,
            d.as_str(),
            "org.freedesktop.NetworkManager.Device",
            "DeviceType",
        )
        .await?;
        has_wifi |= kind == DEVICE_WIFI;
    }
    let wifi_enabled = if has_wifi {
        Some(prop::<bool>(conn, NM, NM_PATH, NM, "WirelessEnabled").await?)
    } else {
        None
    };
    let primary: OwnedObjectPath = prop(conn, NM, NM_PATH, NM, "PrimaryConnection").await?;
    if primary.as_str() == "/" {
        return Ok(Network {
            primary: None,
            strength: None,
            wifi_enabled,
        });
    }
    let active = "org.freedesktop.NetworkManager.Connection.Active";
    let kind: String = prop(conn, NM, primary.as_str(), active, "Type").await?;
    let id: String = prop(conn, NM, primary.as_str(), active, "Id").await?;
    let kind = match kind.as_str() {
        "802-11-wireless" => NetKind::Wifi,
        "802-3-ethernet" => NetKind::Wired,
        "vpn" | "wireguard" => NetKind::Vpn,
        _ => NetKind::Other,
    };
    let strength = if kind == NetKind::Wifi {
        let ap: OwnedObjectPath =
            prop(conn, NM, primary.as_str(), active, "SpecificObject").await?;
        prop::<u8>(
            conn,
            NM,
            ap.as_str(),
            "org.freedesktop.NetworkManager.AccessPoint",
            "Strength",
        )
        .await
        .ok()
    } else {
        None
    };
    Ok(Network {
        primary: Some((kind, id)),
        strength,
        wifi_enabled,
    })
}

async fn set_wifi(conn: &zbus::Connection, on: bool) -> Result<()> {
    let props = zbus::fdo::PropertiesProxy::builder(conn)
        .destination(NM)?
        .path(NM_PATH)?
        .build()
        .await?;
    props
        .set(NM.try_into()?, "WirelessEnabled", on.into())
        .await
        .context("NetworkManager WirelessEnabled")?;
    Ok(())
}

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const UPOWER_DEVICE: &str = "org.freedesktop.UPower.Device";
/// `UPower`'s `State`: charging, fully charged, pending charge.
const CHARGING: [u32; 3] = [1, 4, 5];

async fn battery(conn: &zbus::Connection) -> Result<Battery> {
    let present: bool = prop(conn, UPOWER, DISPLAY_DEVICE, UPOWER_DEVICE, "IsPresent").await?;
    if !present {
        bail!("no battery");
    }
    let percent: f64 = prop(conn, UPOWER, DISPLAY_DEVICE, UPOWER_DEVICE, "Percentage").await?;
    let state: u32 = prop(conn, UPOWER, DISPLAY_DEVICE, UPOWER_DEVICE, "State").await?;
    let icon: String = prop(conn, UPOWER, DISPLAY_DEVICE, UPOWER_DEVICE, "IconName").await?;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0–100"
    )]
    let percent = percent.round().clamp(0.0, 100.0) as u32;
    Ok(Battery {
        percent,
        charging: CHARGING.contains(&state),
        icon,
    })
}

async fn keyboard(hypr: &Hyprland) -> Result<Keyboard> {
    let json = hypr.request("j/devices").await?;
    quick::parse_keyboard(&json).context("no keyboard")
}
