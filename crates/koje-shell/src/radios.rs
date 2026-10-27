//! Quick Settings' system services beyond sound and the backlight (M8.85),
//! read and changed on the system bus as GNOME Shell 50 does: Wi-Fi networks
//! (`NetworkManager`), Bluetooth (`BlueZ`), the power mode
//! (power-profiles-daemon). What they show is in [`crate::quick`].

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::quick::{Bluetooth, BtDevice, PowerProfiles, WifiNetwork};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
/// `NM_DEVICE_TYPE_WIFI`.
const DEVICE_WIFI: u32 = 2;

/// power-profiles-daemon's name since 0.20, then the old one.
const PROFILES: [(&str, &str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    (
        "net.hadess.PowerProfiles",
        "/net/hadess/PowerProfiles",
        "net.hadess.PowerProfiles",
    ),
];

async fn props(
    conn: &zbus::Connection,
    dest: &str,
    path: &str,
) -> Result<zbus::fdo::PropertiesProxy<'static>> {
    Ok(zbus::fdo::PropertiesProxy::builder(conn)
        .destination(dest.to_owned())?
        .path(path.to_owned())?
        .build()
        .await?)
}

async fn get<T>(
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
    let value = props(conn, dest, path)
        .await?
        .get(iface.try_into()?, name)
        .await?;
    T::try_from(value)
        .map_err(Into::into)
        .with_context(|| format!("{iface}.{name}"))
}

/// The active power mode and the ones there are.
pub async fn power(conn: &zbus::Connection) -> Result<PowerProfiles> {
    for (dest, path, iface) in PROFILES {
        let Ok(active) = get::<String>(conn, dest, path, iface, "ActiveProfile").await else {
            continue;
        };
        let list: Vec<HashMap<String, OwnedValue>> = get(conn, dest, path, iface, "Profiles")
            .await
            .unwrap_or_default();
        let available = list
            .iter()
            .filter_map(|p| {
                p.get("Profile")
                    .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            })
            .collect();
        return Ok(PowerProfiles { active, available });
    }
    bail!("no power-profiles-daemon")
}

pub async fn set_power(conn: &zbus::Connection, profile: &str) -> Result<()> {
    for (dest, path, iface) in PROFILES {
        let p = props(conn, dest, path).await?;
        if p.set(iface.try_into()?, "ActiveProfile", Value::from(profile))
            .await
            .is_ok()
        {
            return Ok(());
        }
    }
    bail!("could not set the power mode")
}

type Objects =
    HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>>;

async fn bluez_objects(conn: &zbus::Connection) -> Result<Objects> {
    let om = zbus::fdo::ObjectManagerProxy::builder(conn)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await?;
    Ok(om.get_managed_objects().await?)
}

fn value<T: TryFrom<OwnedValue>>(props: &HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    props
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| T::try_from(v).ok())
}

/// The adapter's power and the paired devices.
pub async fn bluetooth(conn: &zbus::Connection) -> Result<Bluetooth> {
    let objects = bluez_objects(conn).await?;
    let mut adapter = None;
    let mut devices = Vec::new();
    for (path, ifaces) in &objects {
        for (iface, p) in ifaces {
            match iface.as_str() {
                "org.bluez.Adapter1" if adapter.is_none() => {
                    adapter = Some(value::<bool>(p, "Powered").unwrap_or(false));
                }
                "org.bluez.Device1" if value::<bool>(p, "Paired").unwrap_or(false) => {
                    devices.push(BtDevice {
                        path: path.to_string(),
                        name: value::<String>(p, "Alias")
                            .or_else(|| value::<String>(p, "Name"))
                            .unwrap_or_else(|| "Unknown Device".to_owned()),
                        icon: value::<String>(p, "Icon").unwrap_or_default(),
                        connected: value::<bool>(p, "Connected").unwrap_or(false),
                    });
                }
                _ => {}
            }
        }
    }
    let Some(powered) = adapter else {
        bail!("no Bluetooth adapter");
    };
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Bluetooth { powered, devices })
}

pub async fn set_bluetooth(conn: &zbus::Connection, on: bool) -> Result<()> {
    let objects = bluez_objects(conn).await?;
    let adapter = objects
        .iter()
        .find(|(_, i)| i.keys().any(|k| k.as_str() == "org.bluez.Adapter1"))
        .map(|(p, _)| p.to_string())
        .context("no Bluetooth adapter")?;
    props(conn, "org.bluez", &adapter)
        .await?
        .set("org.bluez.Adapter1".try_into()?, "Powered", Value::from(on))
        .await?;
    Ok(())
}

/// Connect or disconnect a paired device (takes a while: run it alone).
pub async fn bluetooth_device(conn: &zbus::Connection, path: &str, connect: bool) -> Result<()> {
    conn.call_method(
        Some("org.bluez"),
        path,
        Some("org.bluez.Device1"),
        if connect { "Connect" } else { "Disconnect" },
        &(),
    )
    .await?;
    Ok(())
}

async fn wifi_device(conn: &zbus::Connection) -> Result<OwnedObjectPath> {
    let devices: Vec<OwnedObjectPath> = get(conn, NM, NM_PATH, NM, "Devices").await?;
    for d in devices {
        if get::<u32>(conn, NM, d.as_str(), DEVICE, "DeviceType").await? == DEVICE_WIFI {
            return Ok(d);
        }
    }
    bail!("no Wi-Fi device")
}

/// Saved Wi-Fi connections by SSID.
async fn known(conn: &zbus::Connection) -> Result<HashMap<Vec<u8>, OwnedObjectPath>> {
    let reply = conn
        .call_method(
            Some(NM),
            NM_SETTINGS,
            Some("org.freedesktop.NetworkManager.Settings"),
            "ListConnections",
            &(),
        )
        .await?;
    let paths: Vec<OwnedObjectPath> = reply.body().deserialize()?;
    let mut out = HashMap::new();
    for p in paths {
        let Ok(reply) = conn
            .call_method(
                Some(NM),
                p.as_str(),
                Some("org.freedesktop.NetworkManager.Settings.Connection"),
                "GetSettings",
                &(),
            )
            .await
        else {
            continue;
        };
        let Ok(settings) = reply
            .body()
            .deserialize::<HashMap<String, HashMap<String, OwnedValue>>>()
        else {
            continue;
        };
        if let Some(ssid) = settings
            .get("802-11-wireless")
            .and_then(|w| value::<Vec<u8>>(w, "ssid"))
        {
            out.insert(ssid, p);
        }
    }
    Ok(out)
}

/// The networks in range, the active and the saved ones marked.
pub async fn wifi_networks(conn: &zbus::Connection) -> Result<Vec<WifiNetwork>> {
    let device = wifi_device(conn).await?;
    let aps: Vec<OwnedObjectPath> =
        get(conn, NM, device.as_str(), WIRELESS, "AccessPoints").await?;
    let active: OwnedObjectPath =
        get(conn, NM, device.as_str(), WIRELESS, "ActiveAccessPoint").await?;
    let known = known(conn).await.unwrap_or_default();
    let mut out = Vec::new();
    for ap in aps {
        let ssid: Vec<u8> = get(conn, NM, ap.as_str(), AP, "Ssid").await?;
        if ssid.is_empty() {
            continue;
        }
        let strength: u8 = get(conn, NM, ap.as_str(), AP, "Strength")
            .await
            .unwrap_or(0);
        let wpa: u32 = get(conn, NM, ap.as_str(), AP, "WpaFlags")
            .await
            .unwrap_or(0);
        let rsn: u32 = get(conn, NM, ap.as_str(), AP, "RsnFlags")
            .await
            .unwrap_or(0);
        let flags: u32 = get(conn, NM, ap.as_str(), AP, "Flags").await.unwrap_or(0);
        out.push(WifiNetwork {
            known: known.contains_key(&ssid),
            ssid: String::from_utf8_lossy(&ssid).into_owned(),
            strength,
            // `NM_802_11_AP_FLAGS_PRIVACY` or any WPA/RSN flag.
            secured: flags & 1 != 0 || wpa != 0 || rsn != 0,
            active: ap == active,
        });
    }
    Ok(out)
}

/// Join a network: a saved one is activated, an open one added; a secured
/// one never joined needs its password – GNOME Settings asks for it.
pub async fn wifi_connect(conn: &zbus::Connection, ssid: &str) -> Result<()> {
    let device = wifi_device(conn).await?;
    let known = known(conn).await.unwrap_or_default();
    let root = OwnedObjectPath::try_from("/")?;
    if let Some(c) = known.get(ssid.as_bytes()) {
        conn.call_method(
            Some(NM),
            NM_PATH,
            Some(NM),
            "ActivateConnection",
            &(c, &device, &root),
        )
        .await?;
        return Ok(());
    }
    let mut wifi: HashMap<&str, Value<'_>> = HashMap::new();
    wifi.insert("ssid", Value::from(ssid.as_bytes().to_vec()));
    let mut settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::new();
    settings.insert("802-11-wireless", wifi);
    conn.call_method(
        Some(NM),
        NM_PATH,
        Some(NM),
        "AddAndActivateConnection",
        &(settings, &device, &root),
    )
    .await?;
    Ok(())
}
