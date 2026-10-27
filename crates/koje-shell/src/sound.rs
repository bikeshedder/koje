//! Sound devices for Quick Settings (M8.84), as GNOME Shell 50's volume
//! menus list them (`status/volume.js`): every output and input with its
//! name as "port – card" ("Speakers – Built-in Audio"), the default one
//! checked, and whether an app records (GNOME shows the input slider only
//! then). Read from `PipeWire`'s `pw-dump`; without GTK.

use serde_json::Value;

/// One output (sink) or input (source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// `PipeWire`'s node id (`wpctl set-default <id>`).
    pub id: u32,
    pub name: String,
    pub icon: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Devices {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub default_output: Option<u32>,
    pub default_input: Option<u32>,
    /// An app records (not counting volume meters GNOME skips).
    pub recording: bool,
}

/// Apps that only show input levels: GNOME does not count them as recording.
const METERS: [&str; 2] = ["org.gnome.VolumeControl", "org.PulseAudio.pavucontrol"];

fn props(o: &Value) -> Option<&serde_json::Map<String, Value>> {
    o.pointer("/info/props")?.as_object()
}

fn text<'a>(p: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    p.get(key)?.as_str()
}

fn number(p: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    let v = p.get(key)?;
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

/// GNOME's device icons, from the port's name and the direction.
fn icon(port: &str, output: bool) -> &'static str {
    let port = port.to_lowercase();
    if port.contains("headphone") {
        "audio-headphones-symbolic"
    } else if port.contains("headset") {
        "audio-headset-symbolic"
    } else if port.contains("hdmi") || port.contains("displayport") {
        "video-display-symbolic"
    } else if output {
        "audio-speakers-symbolic"
    } else {
        "audio-input-microphone-symbolic"
    }
}

/// The devices in `pw-dump`'s JSON; `None` if it is not that.
pub fn parse_pw_dump(json: &str) -> Option<Devices> {
    let objects: Vec<Value> = serde_json::from_str(json).ok()?;
    let mut d = Devices::default();
    let mut default_names: (Option<String>, Option<String>) = (None, None);
    for o in &objects {
        match o.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Metadata") => {
                if o.pointer("/props/metadata.name").and_then(Value::as_str) != Some("default") {
                    continue;
                }
                for entry in o
                    .get("metadata")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let name = entry
                        .pointer("/value/name")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    match entry.get("key").and_then(Value::as_str) {
                        Some("default.audio.sink") => default_names.0 = name,
                        Some("default.audio.source") => default_names.1 = name,
                        _ => {}
                    }
                }
            }
            Some("PipeWire:Interface:Node") => {
                let Some(p) = props(o) else { continue };
                let class = text(p, "media.class").unwrap_or_default();
                if class == "Stream/Input/Audio" {
                    let app = text(p, "application.id")
                        .or_else(|| text(p, "pipewire.access.portal.app_id"))
                        .unwrap_or_default();
                    d.recording |= !METERS.contains(&app);
                    continue;
                }
                let output = match class {
                    "Audio/Sink" => true,
                    "Audio/Source" => false,
                    _ => continue,
                };
                let Some(id) = o
                    .get("id")
                    .and_then(Value::as_u64)
                    .and_then(|i| u32::try_from(i).ok())
                else {
                    continue;
                };
                let (name, port) = device_name(&objects, p, output);
                // The port's name, else the device's own (an HDMI sink
                // without routes).
                let device = Device {
                    id,
                    icon: icon(&format!("{port} {name}"), output),
                    name,
                };
                let node_name = text(p, "node.name").map(str::to_owned);
                if output {
                    if node_name.is_some() && node_name == default_names.0 {
                        d.default_output = Some(id);
                    }
                    d.outputs.push(device);
                } else {
                    if node_name.is_some() && node_name == default_names.1 {
                        d.default_input = Some(id);
                    }
                    d.inputs.push(device);
                }
            }
            _ => {}
        }
    }
    // The metadata may come after the nodes.
    let find = |list: &[Device], name: &Option<String>| -> Option<u32> {
        let name = name.as_deref()?;
        objects.iter().find_map(|o| {
            let p = props(o)?;
            (text(p, "node.name") == Some(name))
                .then(|| o.get("id")?.as_u64()?.try_into().ok())
                .flatten()
                .filter(|id| list.iter().any(|d| d.id == *id))
        })
    };
    d.default_output = d
        .default_output
        .or_else(|| find(&d.outputs, &default_names.0));
    d.default_input = d
        .default_input
        .or_else(|| find(&d.inputs, &default_names.1));
    Some(d)
}

/// "port – card" from the node's device and its active route, as GNOME
/// names devices; the node's own description without them. Also the port's
/// name, for the icon.
fn device_name(
    objects: &[Value],
    node: &serde_json::Map<String, Value>,
    output: bool,
) -> (String, String) {
    let fallback = text(node, "node.description")
        .or_else(|| text(node, "node.nick"))
        .or_else(|| text(node, "node.name"))
        .unwrap_or("Unknown")
        .to_owned();
    let Some(device_id) = number(node, "device.id") else {
        return (fallback, String::new());
    };
    let Some(device) = objects
        .iter()
        .find(|o| o.get("id").and_then(Value::as_u64) == Some(device_id))
    else {
        return (fallback, String::new());
    };
    let card = device
        .pointer("/info/props/device.description")
        .and_then(Value::as_str);
    let profile_device = number(node, "card.profile.device");
    let direction = if output { "Output" } else { "Input" };
    let route = device
        .pointer("/info/params/Route")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|r| {
            r.get("direction").and_then(Value::as_str) == Some(direction)
                && (profile_device.is_none()
                    || r.get("device").and_then(Value::as_u64) == profile_device)
        });
    let port = route
        .and_then(|r| r.get("description").and_then(Value::as_str))
        .map(str::to_owned);
    let port_name = route
        .and_then(|r| r.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    match (port, card) {
        (Some(port), Some(card)) => (format!("{port} – {card}"), port_name),
        (None, Some(card)) => (card.to_owned(), port_name),
        _ => (fallback, port_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts of `pw-dump` that count, as `PipeWire` 1.x writes them.
    const DUMP: &str = r#"[
      {"id": 40, "type": "PipeWire:Interface:Device",
       "info": {"props": {"device.description": "Built-in Audio", "device.api": "alsa"},
                "params": {"Route": [
                  {"index": 2, "direction": "Output", "device": 1, "name": "analog-output-speaker", "description": "Speakers"},
                  {"index": 0, "direction": "Input", "device": 0, "name": "analog-input-internal-mic", "description": "Internal Microphone"}
                ]}}},
      {"id": 52, "type": "PipeWire:Interface:Node",
       "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci.analog-stereo",
                          "node.description": "Built-in Audio Analog Stereo", "device.id": 40, "card.profile.device": 1}}},
      {"id": 53, "type": "PipeWire:Interface:Node",
       "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_input.pci.analog-stereo",
                          "node.description": "Built-in Audio Analog Stereo", "device.id": 40, "card.profile.device": 0}}},
      {"id": 60, "type": "PipeWire:Interface:Node",
       "info": {"props": {"media.class": "Audio/Sink", "node.name": "hdmi-out",
                          "node.description": "HDMI / DisplayPort"}}},
      {"id": 70, "type": "PipeWire:Interface:Node",
       "info": {"props": {"media.class": "Stream/Input/Audio", "application.id": "org.PulseAudio.pavucontrol"}}},
      {"id": 1, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
       "metadata": [
         {"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON", "value": {"name": "hdmi-out"}},
         {"subject": 0, "key": "default.audio.source", "type": "Spa:String:JSON", "value": {"name": "alsa_input.pci.analog-stereo"}}
       ]}
    ]"#;

    #[test]
    fn devices_named_as_gnome() {
        let d = parse_pw_dump(DUMP).unwrap();
        assert_eq!(
            d.outputs
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            ["Speakers – Built-in Audio", "HDMI / DisplayPort"]
        );
        assert_eq!(d.outputs[0].icon, "audio-speakers-symbolic");
        assert_eq!(d.outputs[1].icon, "video-display-symbolic");
        assert_eq!(d.inputs[0].name, "Internal Microphone – Built-in Audio");
        assert_eq!(d.inputs[0].icon, "audio-input-microphone-symbolic");
        assert_eq!((d.default_output, d.default_input), (Some(60), Some(53)));
        // pavucontrol's meter is not recording.
        assert!(!d.recording);
    }

    #[test]
    fn an_app_recording() {
        let dump = DUMP.replace("org.PulseAudio.pavucontrol", "org.mozilla.firefox");
        assert!(parse_pw_dump(&dump).unwrap().recording);
        assert_eq!(parse_pw_dump("not json"), None);
    }

    #[test]
    fn icons_from_ports() {
        assert_eq!(
            icon("analog-output-headphones", true),
            "audio-headphones-symbolic"
        );
        assert_eq!(icon("hdmi-output-0", true), "video-display-symbolic");
        assert_eq!(icon("", false), "audio-input-microphone-symbolic");
    }
}
