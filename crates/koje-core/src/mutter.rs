//! GNOME Settings' Displays panel on Hyprland (ADR 0046, M8.51): Mutter's
//! view of the screens (`org.gnome.Mutter.DisplayConfig.GetCurrentState`)
//! from Hyprland's `j/monitors all`, and what `ApplyMonitorsConfig` asks for
//! as Hyprland `monitor` lines and as a monitors.xml configuration. Pure:
//! kojed serves the API.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Deserialize;

use crate::edid::Identity;
use crate::monitors::{Configuration, LayoutMode, Logical, Mode, Monitor, Spec};

/// One screen of Hyprland's `j/monitors all` (disabled ones included).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyprMonitor {
    pub name: String,
    #[serde(default)]
    pub make: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub physical_width: u32,
    #[serde(default)]
    pub physical_height: u32,
    #[serde(default)]
    pub refresh_rate: f64,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub transform: u32,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub mirror_of: String,
    #[serde(default)]
    pub vrr: bool,
    /// `1920x1080@60.00Hz`.
    #[serde(default)]
    pub available_modes: Vec<String>,
}

fn one() -> f64 {
    1.0
}

pub fn parse_monitors(json: &str) -> Result<Vec<HyprMonitor>, String> {
    serde_json::from_str(json).map_err(|e| format!("Hyprland's j/monitors: {e}"))
}

/// A mode as Hyprland lists it: `5120x2160@119.99Hz` → width, height, rate.
pub fn parse_mode(text: &str) -> Option<(u32, u32, f64)> {
    let (size, rate) = text.trim().trim_end_matches("Hz").split_once('@')?;
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?, rate.parse().ok()?))
}

/// Mutter-style mode ID: `5120x2160@119.990`.
pub fn mode_id(width: u32, height: u32, rate: f64) -> String {
    format!("{width}x{height}@{rate:.3}")
}

/// Scales Hyprland and Mutter can both use for a mode: from 1 to 4 in
/// quarter steps, where the logical size comes out in whole pixels.
pub fn supported_scales(width: u32, height: u32) -> Vec<f64> {
    (4..=16)
        .map(|q| f64::from(q) / 4.0)
        .filter(|s| {
            let w = f64::from(width) / s;
            let h = f64::from(height) / s;
            w.fract() == 0.0 && h.fract() == 0.0 && w >= 800.0 && h >= 480.0
        })
        .collect()
}

/// GNOME's suggested scale: 2 above about 180 pixels per inch, else 1.
pub fn preferred_scale(width: u32, height: u32, width_mm: u32) -> f64 {
    if width_mm == 0 {
        return 1.0;
    }
    let dpi = f64::from(width) / (f64::from(width_mm) / 25.4);
    if dpi > 180.0 && supported_scales(width, height).contains(&2.0) {
        2.0
    } else {
        1.0
    }
}

/// A mode in Mutter's view.
#[derive(Debug, Clone, PartialEq)]
pub struct ModeView {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub rate: f64,
    pub preferred_scale: f64,
    pub scales: Vec<f64>,
    pub current: bool,
    pub preferred: bool,
}

/// A screen in Mutter's view.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorView {
    pub spec: Spec,
    pub modes: Vec<ModeView>,
    pub display_name: String,
    pub builtin: bool,
    pub width_mm: u32,
    pub height_mm: u32,
}

/// A logical monitor in Mutter's view.
#[derive(Debug, Clone, PartialEq)]
pub struct LogicalView {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u32,
    pub primary: bool,
    pub monitors: Vec<Spec>,
}

/// Built-in panels: laptop screens.
pub fn is_builtin(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"]
        .iter()
        .any(|p| connector.starts_with(p))
}

/// The identity of a screen: Mutter's from its EDID when known, else
/// Hyprland's make, model and serial.
fn spec_of(m: &HyprMonitor, ids: &BTreeMap<String, Identity>) -> Spec {
    Spec {
        connector: m.name.clone(),
        id: ids.get(&m.name).cloned().unwrap_or_else(|| Identity {
            vendor: m.make.clone(),
            product: m.model.clone(),
            serial: m.serial.clone(),
        }),
    }
}

/// Mutter's view of Hyprland's screens. `ids`: Mutter's identity per
/// connector (from the EDIDs). The primary logical monitor – Hyprland has
/// none – is the one at the top left.
pub fn current_state(
    hypr: &[HyprMonitor],
    ids: &BTreeMap<String, Identity>,
) -> (Vec<MonitorView>, Vec<LogicalView>) {
    let monitors = hypr
        .iter()
        .map(|m| {
            let current = mode_id(m.width, m.height, m.refresh_rate);
            let mut modes: Vec<ModeView> = m
                .available_modes
                .iter()
                .filter_map(|t| parse_mode(t))
                .enumerate()
                .map(|(i, (w, h, r))| ModeView {
                    id: mode_id(w, h, r),
                    width: w,
                    height: h,
                    rate: r,
                    preferred_scale: preferred_scale(w, h, m.physical_width),
                    scales: supported_scales(w, h),
                    current: false,
                    preferred: i == 0,
                })
                .collect();
            // The current mode by size and nearest rate: Hyprland rounds the
            // listed rates.
            if !m.disabled
                && let Some(best) = modes
                    .iter_mut()
                    .filter(|v| v.width == m.width && v.height == m.height)
                    .min_by(|a, b| {
                        (a.rate - m.refresh_rate)
                            .abs()
                            .total_cmp(&(b.rate - m.refresh_rate).abs())
                    })
            {
                best.current = true;
            } else if !m.disabled && m.width > 0 {
                modes.push(ModeView {
                    id: current,
                    width: m.width,
                    height: m.height,
                    rate: m.refresh_rate,
                    preferred_scale: 1.0,
                    scales: supported_scales(m.width, m.height),
                    current: true,
                    preferred: modes.is_empty(),
                });
            }
            let spec = spec_of(m, ids);
            MonitorView {
                display_name: if m.make.is_empty() && m.model.is_empty() {
                    m.name.clone()
                } else {
                    format!("{} {}", m.make, m.model).trim().to_owned()
                },
                builtin: is_builtin(&m.name),
                width_mm: m.physical_width,
                height_mm: m.physical_height,
                spec,
                modes,
            }
        })
        .collect();

    let mut logical: Vec<LogicalView> = Vec::new();
    for m in hypr.iter().filter(|m| !m.disabled) {
        let spec = spec_of(m, ids);
        let mirrored =
            (!m.mirror_of.is_empty() && m.mirror_of != "none").then(|| m.mirror_of.clone());
        if let Some(of) = mirrored
            && let Some(l) = logical
                .iter_mut()
                .find(|l| l.monitors.iter().any(|s| s.connector == of))
        {
            l.monitors.push(spec);
            continue;
        }
        logical.push(LogicalView {
            x: m.x,
            y: m.y,
            scale: m.scale,
            transform: m.transform,
            primary: false,
            monitors: vec![spec],
        });
    }
    if let Some(first) = logical.iter_mut().min_by_key(|l| (l.y, l.x)) {
        first.primary = true;
    }
    (monitors, logical)
}

/// One logical monitor of `ApplyMonitorsConfig`: position, scale,
/// transform, primary, and its screens as connector and mode ID.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestedLogical {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u32,
    pub primary: bool,
    pub monitors: Vec<(String, String)>,
}

/// What GNOME Settings asked for, checked against the screens: Hyprland's
/// lines (screens not in the request are switched off) and the
/// configuration to store in monitors.xml. An error says what is wrong.
pub fn apply_request(
    request: &[RequestedLogical],
    current: &[MonitorView],
) -> Result<(Vec<String>, Configuration), String> {
    if request.is_empty() {
        return Err("no screen left on".to_owned());
    }
    let mut lines = Vec::new();
    let mut logical = Vec::new();
    let mut used = Vec::new();
    for l in request {
        if !(0.5..=4.0).contains(&l.scale) {
            return Err(format!("scale {} is out of range", l.scale));
        }
        if l.transform > 7 {
            return Err(format!("transform {} is unknown", l.transform));
        }
        let mut monitors = Vec::new();
        for (i, (connector, id)) in l.monitors.iter().enumerate() {
            let m = current
                .iter()
                .find(|m| &m.spec.connector == connector)
                .ok_or_else(|| format!("no screen {connector}"))?;
            let mode = m
                .modes
                .iter()
                .find(|v| &v.id == id)
                .ok_or_else(|| format!("{connector} has no mode {id}"))?;
            let mut line = format!(
                "{connector}, {}x{}@{:.3}, {}x{}, {}",
                mode.width, mode.height, mode.rate, l.x, l.y, l.scale
            );
            if l.transform != 0 {
                let _ = write!(line, ", transform, {}", l.transform);
            }
            if i > 0 {
                let _ = write!(line, ", mirror, {}", l.monitors[0].0);
            }
            lines.push(line);
            used.push(connector.clone());
            monitors.push(Monitor {
                spec: m.spec.clone(),
                mode: Mode {
                    width: mode.width,
                    height: mode.height,
                    rate: mode.rate,
                    variable: false,
                    interlace: false,
                },
                extra: Vec::new(),
            });
        }
        logical.push(Logical {
            x: l.x,
            y: l.y,
            scale: l.scale,
            primary: l.primary,
            transform: l.transform,
            monitors,
            extra: Vec::new(),
        });
    }
    let disabled: Vec<Spec> = current
        .iter()
        .filter(|m| !used.contains(&m.spec.connector))
        .map(|m| m.spec.clone())
        .collect();
    for d in &disabled {
        lines.push(format!("{}, disable", d.connector));
    }
    Ok((
        lines,
        Configuration {
            layout: Some(LayoutMode::Logical),
            logical,
            disabled,
            extra: Vec::new(),
        },
    ))
}

/// The screens as they are now, as Hyprland lines – what a revert goes back
/// to.
pub fn current_lines(monitors: &[MonitorView], logical: &[LogicalView]) -> Vec<String> {
    let mut lines = Vec::new();
    for l in logical {
        for (i, spec) in l.monitors.iter().enumerate() {
            let Some(mode) = monitors
                .iter()
                .find(|m| &m.spec == spec)
                .and_then(|m| m.modes.iter().find(|v| v.current))
            else {
                continue;
            };
            let mut line = format!(
                "{}, {}x{}@{:.3}, {}x{}, {}",
                spec.connector, mode.width, mode.height, mode.rate, l.x, l.y, l.scale
            );
            if l.transform != 0 {
                let _ = write!(line, ", transform, {}", l.transform);
            }
            if i > 0 {
                let _ = write!(line, ", mirror, {}", l.monitors[0].connector);
            }
            lines.push(line);
        }
    }
    for m in monitors {
        if !logical.iter().any(|l| l.monitors.contains(&m.spec)) {
            lines.push(format!("{}, disable", m.spec.connector));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"[
      {"id":0,"name":"DP-3","description":"Dell Inc. DELL U4025QW CKJ5734","make":"Dell Inc.","model":"DELL U4025QW","serial":"CKJ5734",
       "width":5120,"height":2160,"physicalWidth":940,"physicalHeight":400,"refreshRate":119.99600,"x":0,"y":0,"scale":1.25,"transform":0,
       "focused":true,"disabled":false,"mirrorOf":"none","vrr":false,
       "availableModes":["5120x2160@120.00Hz","5120x2160@60.00Hz","3840x2160@60.00Hz","2560x1440@59.95Hz"]},
      {"id":1,"name":"eDP-1","make":"BOE","model":"0x0A1B","serial":"","width":1920,"height":1200,"physicalWidth":300,"physicalHeight":190,
       "refreshRate":60.0,"x":0,"y":0,"scale":1.0,"transform":0,"disabled":true,"mirrorOf":"none",
       "availableModes":["1920x1200@60.00Hz"]}
    ]"#;

    fn ids() -> BTreeMap<String, Identity> {
        BTreeMap::from([(
            "DP-3".to_owned(),
            Identity {
                vendor: "DEL".into(),
                product: "DELL U4025QW".into(),
                serial: "CKJ5734".into(),
            },
        )])
    }

    #[test]
    fn modes_and_scales() {
        assert_eq!(parse_mode("5120x2160@119.99Hz"), Some((5120, 2160, 119.99)));
        assert_eq!(parse_mode("junk"), None);
        assert_eq!(mode_id(5120, 2160, 120.0), "5120x2160@120.000");
        assert_eq!(supported_scales(5120, 2160), [1.0, 1.25, 2.0, 2.5, 4.0]);
        assert_eq!(supported_scales(1920, 1080), [1.0, 1.25, 1.5, 2.0]);
        // A 15.6" 4K laptop panel; the 40" ultrawide; an unknown size.
        assert_eq!(preferred_scale(3840, 2160, 340), 2.0);
        assert_eq!(preferred_scale(5120, 2160, 940), 1.0);
        assert_eq!(preferred_scale(1920, 1080, 0), 1.0);
    }

    #[test]
    fn mutters_view_of_hyprlands_screens() {
        let hypr = parse_monitors(JSON).unwrap();
        let (monitors, logical) = current_state(&hypr, &ids());
        let dell = &monitors[0];
        assert_eq!(
            dell.spec.id.vendor, "DEL",
            "Mutter's identity from the EDID"
        );
        assert_eq!(dell.display_name, "Dell Inc. DELL U4025QW");
        let current: Vec<&str> = dell
            .modes
            .iter()
            .filter(|m| m.current)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(current, ["5120x2160@120.000"]);
        assert!(dell.modes[0].preferred);
        let panel = &monitors[1];
        assert!(panel.builtin);
        assert_eq!(
            panel.spec.id.vendor, "BOE",
            "Hyprland's make without an EDID"
        );
        assert!(panel.modes.iter().all(|m| !m.current), "switched off");
        assert_eq!(logical.len(), 1);
        assert!(logical[0].primary);
        assert_eq!(logical[0].scale, 1.25);
    }

    #[test]
    fn applying_gnome_settings_request() {
        let hypr = parse_monitors(JSON).unwrap();
        let (monitors, _) = current_state(&hypr, &ids());
        let request = [RequestedLogical {
            x: 0,
            y: 0,
            scale: 1.25,
            transform: 0,
            primary: true,
            monitors: vec![("DP-3".into(), "5120x2160@120.000".into())],
        }];
        let (lines, conf) = apply_request(&request, &monitors).unwrap();
        assert_eq!(
            lines,
            ["DP-3, 5120x2160@120.000, 0x0, 1.25", "eDP-1, disable"]
        );
        assert_eq!(conf.specs().len(), 2);
        assert_eq!(conf.disabled[0].connector, "eDP-1");
        assert_eq!(conf.logical[0].monitors[0].spec.id.serial, "CKJ5734");
    }

    #[test]
    fn the_current_state_as_lines() {
        let hypr = parse_monitors(JSON).unwrap();
        let (monitors, logical) = current_state(&hypr, &ids());
        assert_eq!(
            current_lines(&monitors, &logical),
            ["DP-3, 5120x2160@120.000, 0x0, 1.25", "eDP-1, disable"]
        );
    }

    #[test]
    fn requests_that_cannot_be() {
        let hypr = parse_monitors(JSON).unwrap();
        let (monitors, _) = current_state(&hypr, &ids());
        let req = |connector: &str, id: &str, scale: f64| {
            vec![RequestedLogical {
                x: 0,
                y: 0,
                scale,
                transform: 0,
                primary: true,
                monitors: vec![(connector.into(), id.into())],
            }]
        };
        assert!(
            apply_request(&req("DP-9", "1x1@1.000", 1.0), &monitors)
                .unwrap_err()
                .contains("no screen")
        );
        assert!(
            apply_request(&req("DP-3", "1x1@1.000", 1.0), &monitors)
                .unwrap_err()
                .contains("no mode")
        );
        assert!(
            apply_request(&req("DP-3", "5120x2160@120.000", 9.0), &monitors)
                .unwrap_err()
                .contains("scale")
        );
        assert!(apply_request(&[], &monitors).is_err());
    }
}
