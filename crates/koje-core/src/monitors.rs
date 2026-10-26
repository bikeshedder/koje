//! GNOME's `~/.config/monitors.xml` (ADR 0046): the single source of truth
//! for the screens, in Mutter's format (version 2) – one configuration per set
//! of connected screens. Read and written as a tree so what koje does not use
//! (colour modes, policies, …) survives; the parts koje uses are typed.
//! Also Hyprland's `monitor` lines for a configuration.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use quick_xml::events::Event;

use crate::edid::Identity;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MonitorsError {
    #[error("monitors.xml: {0}")]
    Xml(String),
    #[error("monitors.xml: version {0} is not supported (only 2)")]
    Version(String),
}

/// An XML element; monitors.xml has no mixed content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    fn leaf(name: &str, text: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            text: text.into(),
            ..Self::default()
        }
    }

    fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }
}

/// Parse an XML document into its root element.
pub fn parse_tree(xml: &str) -> Result<Element, MonitorsError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Element> = vec![Element::new("")];
    let err = |e: &dyn std::fmt::Display| MonitorsError::Xml(e.to_string());
    loop {
        match reader.read_event().map_err(|e| err(&e))? {
            Event::Start(s) => {
                let mut e = Element::new(s.name().as_ref());
                for a in s.attributes() {
                    let a = a.map_err(|e| err(&e))?;
                    e.attrs.push((
                        a.key.as_ref().to_owned(),
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|e| err(&e))?
                            .into_owned(),
                    ));
                }
                stack.push(e);
            }
            Event::Empty(s) => {
                let e = Element::new(s.name().as_ref());
                stack
                    .last_mut()
                    .ok_or_else(|| err(&"unbalanced"))?
                    .children
                    .push(e);
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.xml10_content());
                }
            }
            // `&amp;`, `&#38;`: text split around the reference.
            Event::GeneralRef(r) => {
                let c = if let Some(c) = r.resolve_char_ref().map_err(|e| err(&e))? {
                    c.to_string()
                } else {
                    let name: &str = r.as_ref();
                    quick_xml::escape::resolve_predefined_entity(name)
                        .ok_or_else(|| err(&format!("unknown entity &{name};")))?
                        .to_owned()
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&c);
                }
            }
            Event::End(_) => {
                let mut done = stack.pop().ok_or_else(|| err(&"unbalanced"))?;
                // Whitespace around children and values is layout only.
                let raw = std::mem::take(&mut done.text);
                raw.trim().clone_into(&mut done.text);
                stack
                    .last_mut()
                    .ok_or_else(|| err(&"unbalanced"))?
                    .children
                    .push(done);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut doc = stack.pop().ok_or_else(|| err(&"empty"))?;
    if !stack.is_empty() {
        return Err(err(&"unclosed element"));
    }
    doc.children.pop().ok_or_else(|| err(&"no root element"))
}

fn escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Write a tree as Mutter does: two spaces per level. Scales are written
/// shortest (`1`, `1.25`).
pub fn write_tree(root: &Element) -> String {
    fn write(out: &mut String, e: &Element, depth: usize) {
        let pad = "  ".repeat(depth);
        let attrs = e.attrs.iter().fold(String::new(), |mut a, (k, v)| {
            let _ = write!(a, " {k}=\"{}\"", escape(v));
            a
        });
        if e.children.is_empty() {
            let _ = writeln!(
                out,
                "{pad}<{}{attrs}>{}</{}>",
                e.name,
                escape(&e.text),
                e.name
            );
        } else {
            let _ = writeln!(out, "{pad}<{}{attrs}>", e.name);
            for c in &e.children {
                write(out, c, depth + 1);
            }
            let _ = writeln!(out, "{pad}</{}>", e.name);
        }
    }
    let mut out = String::new();
    write(&mut out, root, 0);
    out
}

/// A screen: its connector and its identity (`monitorspec`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spec {
    pub connector: String,
    pub id: Identity,
}

impl Spec {
    fn parse(e: &Element) -> Option<Self> {
        Some(Self {
            connector: e.text_of("connector")?.to_owned(),
            id: Identity {
                vendor: e.text_of("vendor")?.to_owned(),
                product: e.text_of("product")?.to_owned(),
                serial: e.text_of("serial")?.to_owned(),
            },
        })
    }

    fn to_element(&self) -> Element {
        let mut e = Element::new("monitorspec");
        e.children = vec![
            Element::leaf("connector", &self.connector),
            Element::leaf("vendor", &self.id.vendor),
            Element::leaf("product", &self.id.product),
            Element::leaf("serial", &self.id.serial),
        ];
        e
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Hz.
    pub rate: f64,
    pub variable: bool,
    pub interlace: bool,
}

/// One screen in a logical monitor; `extra` keeps the elements koje does not
/// use (underscanning, colour mode, …).
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub spec: Spec,
    pub mode: Mode,
    pub extra: Vec<Element>,
}

/// Mutter's transforms, numbered like Wayland's and Hyprland's: 0 normal,
/// 1–3 rotated by 90° steps, 4–7 the same flipped.
pub type Transform = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Positions in logical (scaled) pixels – GNOME's default on Wayland.
    Logical,
    /// Positions in physical pixels.
    Physical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Logical {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub primary: bool,
    pub transform: Transform,
    /// More than one: they mirror each other.
    pub monitors: Vec<Monitor>,
    pub extra: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub layout: Option<LayoutMode>,
    pub logical: Vec<Logical>,
    pub disabled: Vec<Spec>,
    pub extra: Vec<Element>,
}

impl Configuration {
    /// The set of screens this configuration is for.
    pub fn specs(&self) -> BTreeSet<Spec> {
        self.logical
            .iter()
            .flat_map(|l| l.monitors.iter().map(|m| m.spec.clone()))
            .chain(self.disabled.iter().cloned())
            .collect()
    }
}

/// monitors.xml: its configurations, oldest first, and what else it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorsXml {
    pub configurations: Vec<Configuration>,
    pub extra: Vec<Element>,
}

fn yes(text: Option<&str>) -> bool {
    text == Some("yes")
}

fn parse_mode(e: &Element) -> Option<Mode> {
    Some(Mode {
        width: e.text_of("width")?.parse().ok()?,
        height: e.text_of("height")?.parse().ok()?,
        rate: e.text_of("rate")?.parse().ok()?,
        variable: e.text_of("ratemode") == Some("variable"),
        interlace: e.text_of("flag") == Some("interlace"),
    })
}

fn parse_transform(e: Option<&Element>) -> Transform {
    let Some(e) = e else { return 0 };
    let rotation = match e.text_of("rotation") {
        Some("left") => 1,
        Some("upside_down") => 2,
        Some("right") => 3,
        _ => 0,
    };
    rotation + if yes(e.text_of("flipped")) { 4 } else { 0 }
}

fn parse_logical(e: &Element) -> Option<Logical> {
    let mut monitors = Vec::new();
    for m in e.children.iter().filter(|c| c.name == "monitor") {
        monitors.push(Monitor {
            spec: Spec::parse(m.child("monitorspec")?)?,
            mode: parse_mode(m.child("mode")?)?,
            extra: m
                .children
                .iter()
                .filter(|c| c.name != "monitorspec" && c.name != "mode")
                .cloned()
                .collect(),
        });
    }
    Some(Logical {
        x: e.text_of("x")?.parse().ok()?,
        y: e.text_of("y")?.parse().ok()?,
        scale: e.text_of("scale").map_or(Some(1.0), |s| s.parse().ok())?,
        primary: yes(e.text_of("primary")),
        transform: parse_transform(e.child("transform")),
        monitors,
        extra: e
            .children
            .iter()
            .filter(|c| {
                !matches!(
                    c.name.as_str(),
                    "x" | "y" | "scale" | "primary" | "transform" | "monitor"
                )
            })
            .cloned()
            .collect(),
    })
}

fn parse_configuration(e: &Element) -> Option<Configuration> {
    Some(Configuration {
        layout: match e.text_of("layoutmode") {
            Some("logical") => Some(LayoutMode::Logical),
            Some("physical") => Some(LayoutMode::Physical),
            _ => None,
        },
        logical: e
            .children
            .iter()
            .filter(|c| c.name == "logicalmonitor")
            .map(parse_logical)
            .collect::<Option<_>>()?,
        disabled: e
            .child("disabled")
            .map(|d| d.children.iter().filter_map(Spec::parse).collect())
            .unwrap_or_default(),
        extra: e
            .children
            .iter()
            .filter(|c| {
                !matches!(
                    c.name.as_str(),
                    "layoutmode" | "logicalmonitor" | "disabled"
                )
            })
            .cloned()
            .collect(),
    })
}

/// Read monitors.xml. Configurations koje cannot read are kept as they are
/// (in `extra`), so writing the file back loses nothing.
pub fn parse(xml: &str) -> Result<MonitorsXml, MonitorsError> {
    let root = parse_tree(xml)?;
    if root.name != "monitors" {
        return Err(MonitorsError::Xml(format!(
            "root <{}>, not <monitors>",
            root.name
        )));
    }
    let version = root
        .attrs
        .iter()
        .find(|(k, _)| k == "version")
        .map_or("", |(_, v)| v.as_str());
    if version != "2" {
        return Err(MonitorsError::Version(version.to_owned()));
    }
    let mut out = MonitorsXml {
        configurations: Vec::new(),
        extra: Vec::new(),
    };
    for c in root.children {
        match (c.name.as_str(), parse_configuration(&c)) {
            ("configuration", Some(conf)) => out.configurations.push(conf),
            _ => out.extra.push(c),
        }
    }
    Ok(out)
}

fn mode_element(m: &Mode) -> Element {
    let mut e = Element::new("mode");
    e.children = vec![
        Element::leaf("width", m.width.to_string()),
        Element::leaf("height", m.height.to_string()),
        Element::leaf("rate", format!("{:.3}", m.rate)),
    ];
    if m.variable {
        e.children.push(Element::leaf("ratemode", "variable"));
    }
    if m.interlace {
        e.children.push(Element::leaf("flag", "interlace"));
    }
    e
}

fn configuration_element(c: &Configuration) -> Element {
    let mut e = Element::new("configuration");
    match c.layout {
        Some(LayoutMode::Logical) => e.children.push(Element::leaf("layoutmode", "logical")),
        Some(LayoutMode::Physical) => e.children.push(Element::leaf("layoutmode", "physical")),
        None => {}
    }
    for l in &c.logical {
        let mut le = Element::new("logicalmonitor");
        le.children.push(Element::leaf("x", l.x.to_string()));
        le.children.push(Element::leaf("y", l.y.to_string()));
        le.children.push(Element::leaf("scale", scale_str(l.scale)));
        if l.primary {
            le.children.push(Element::leaf("primary", "yes"));
        }
        if l.transform != 0 {
            let mut t = Element::new("transform");
            let rotation = ["normal", "left", "upside_down", "right"][(l.transform % 4) as usize];
            t.children.push(Element::leaf("rotation", rotation));
            t.children.push(Element::leaf(
                "flipped",
                if l.transform >= 4 { "yes" } else { "no" },
            ));
            le.children.push(t);
        }
        for m in &l.monitors {
            let mut me = Element::new("monitor");
            me.children.push(m.spec.to_element());
            me.children.push(mode_element(&m.mode));
            me.children.extend(m.extra.iter().cloned());
            le.children.push(me);
        }
        le.children.extend(l.extra.iter().cloned());
        e.children.push(le);
    }
    if !c.disabled.is_empty() {
        let mut d = Element::new("disabled");
        d.children = c.disabled.iter().map(Spec::to_element).collect();
        e.children.push(d);
    }
    e.children.extend(c.extra.iter().cloned());
    e
}

/// monitors.xml as Mutter writes it.
pub fn to_xml(m: &MonitorsXml) -> String {
    let mut root = Element::new("monitors");
    root.attrs.push(("version".to_owned(), "2".to_owned()));
    root.children = m.configurations.iter().map(configuration_element).collect();
    root.children.extend(m.extra.iter().cloned());
    write_tree(&root)
}

/// The configuration for exactly the connected screens, as Mutter picks it.
pub fn find<'a>(m: &'a MonitorsXml, connected: &BTreeSet<Spec>) -> Option<&'a Configuration> {
    m.configurations
        .iter()
        .rev()
        .find(|c| &c.specs() == connected)
}

/// Store `conf` for its set of screens: it replaces the one for the same set.
pub fn put(m: &mut MonitorsXml, conf: Configuration) {
    let specs = conf.specs();
    m.configurations.retain(|c| c.specs() != specs);
    m.configurations.push(conf);
}

fn scale_str(s: f64) -> String {
    let r = (s * 1_000_000.0).round() / 1_000_000.0;
    format!("{r}")
}

/// Hyprland's `monitor` values (what follows `monitor =`) for a
/// configuration: each screen by its connector with mode, position, scale,
/// transform; a second screen of a logical monitor mirrors the first;
/// disabled ones off.
pub fn hyprland_lines(c: &Configuration) -> Vec<String> {
    let mut out = Vec::new();
    for l in &c.logical {
        // Hyprland places screens in logical pixels.
        let (x, y) = match c.layout {
            Some(LayoutMode::Physical) if l.scale > 0.0 => {
                #[allow(clippy::cast_possible_truncation, reason = "screen positions")]
                let p = |v: i32| (f64::from(v) / l.scale).round() as i32;
                (p(l.x), p(l.y))
            }
            _ => (l.x, l.y),
        };
        let first = l.monitors.first().map(|m| m.spec.connector.clone());
        for (i, m) in l.monitors.iter().enumerate() {
            let mut line = format!(
                "{}, {}x{}@{:.3}, {x}x{y}, {}",
                m.spec.connector,
                m.mode.width,
                m.mode.height,
                m.mode.rate,
                scale_str(l.scale)
            );
            if l.transform != 0 {
                let _ = write!(line, ", transform, {}", l.transform);
            }
            if m.mode.variable {
                line.push_str(", vrr, 1");
            }
            if i > 0
                && let Some(f) = &first
            {
                let _ = write!(line, ", mirror, {f}");
            }
            out.push(line);
        }
    }
    for d in &c.disabled {
        out.push(format!("{}, disable", d.connector));
    }
    out
}

/// Without a configuration for the connected set: each screen as in the
/// newest configuration that has it, placed automatically.
pub fn fallback_lines(m: &MonitorsXml, connected: &BTreeSet<Spec>) -> Vec<String> {
    let mut out = Vec::new();
    for spec in connected {
        let found = m.configurations.iter().rev().find_map(|c| {
            c.logical.iter().find_map(|l| {
                l.monitors
                    .iter()
                    .find(|mon| &mon.spec == spec)
                    .map(|mon| (l, mon))
            })
        });
        if let Some((l, mon)) = found {
            let mut line = format!(
                "{}, {}x{}@{:.3}, auto, {}",
                spec.connector,
                mon.mode.width,
                mon.mode.height,
                mon.mode.rate,
                scale_str(l.scale)
            );
            if l.transform != 0 {
                let _ = write!(line, ", transform, {}", l.transform);
            }
            out.push(line);
        }
    }
    out
}

/// The connector of a `/sys/class/drm` entry: `card1-DP-3` → `DP-3`; not
/// for the cards themselves, render nodes or writeback connectors.
pub fn connector_of(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("card")?;
    let (card, connector) = rest.split_once('-')?;
    (!card.is_empty()
        && card.chars().all(|c| c.is_ascii_digit())
        && !connector.starts_with("Writeback"))
    .then_some(connector)
}

/// Hyprland's config file for the screens: `monitor =` lines.
pub fn hyprland_conf(lines: &[String]) -> String {
    let mut out = String::from(
        "# The screens from ~/.config/monitors.xml (ADR 0046), written by koje\n\
         # at login – edits are lost; change screens in GNOME Settings.\n",
    );
    for l in lines {
        let _ = writeln!(out, "monitor = {l}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<monitors version="2">
  <configuration>
    <layoutmode>logical</layoutmode>
    <logicalmonitor>
      <x>0</x>
      <y>0</y>
      <scale>1.25</scale>
      <primary>yes</primary>
      <monitor>
        <monitorspec>
          <connector>DP-1</connector>
          <vendor>DEL</vendor>
          <product>DELL U4025QW</product>
          <serial>7XYZ123</serial>
        </monitorspec>
        <mode>
          <width>5120</width>
          <height>2160</height>
          <rate>119.996</rate>
          <ratemode>variable</ratemode>
        </mode>
        <colormode>bt2100</colormode>
      </monitor>
    </logicalmonitor>
    <logicalmonitor>
      <x>4096</x>
      <y>0</y>
      <scale>1</scale>
      <transform>
        <rotation>left</rotation>
        <flipped>no</flipped>
      </transform>
      <monitor>
        <monitorspec>
          <connector>HDMI-A-1</connector>
          <vendor>GSM</vendor>
          <product>LG HDR 4K</product>
          <serial>0x0001b2c3</serial>
        </monitorspec>
        <mode>
          <width>3840</width>
          <height>2160</height>
          <rate>60.000</rate>
        </mode>
      </monitor>
    </logicalmonitor>
    <disabled>
      <monitorspec>
        <connector>eDP-1</connector>
        <vendor>BOE</vendor>
        <product>0x0a1b</product>
        <serial>0x00000000</serial>
      </monitorspec>
    </disabled>
  </configuration>
  <policy>
    <stores>
      <store>user</store>
    </stores>
  </policy>
</monitors>
"#;

    fn spec(connector: &str, vendor: &str, product: &str, serial: &str) -> Spec {
        Spec {
            connector: connector.into(),
            id: Identity {
                vendor: vendor.into(),
                product: product.into(),
                serial: serial.into(),
            },
        }
    }

    fn dell() -> Spec {
        spec("DP-1", "DEL", "DELL U4025QW", "7XYZ123")
    }

    fn lg() -> Spec {
        spec("HDMI-A-1", "GSM", "LG HDR 4K", "0x0001b2c3")
    }

    fn panel() -> Spec {
        spec("eDP-1", "BOE", "0x0a1b", "0x00000000")
    }

    #[test]
    fn reads_mutters_file() {
        let m = parse(XML).unwrap();
        assert_eq!(m.configurations.len(), 1);
        let c = &m.configurations[0];
        assert_eq!(c.layout, Some(LayoutMode::Logical));
        assert_eq!(c.logical[0].scale, 1.25);
        assert!(c.logical[0].primary);
        assert_eq!(c.logical[0].monitors[0].mode.rate, 119.996);
        assert!(c.logical[0].monitors[0].mode.variable);
        assert_eq!(c.logical[1].transform, 1);
        assert_eq!(c.specs(), BTreeSet::from([dell(), lg(), panel()]));
        // `<policy>` is not koje's: kept.
        assert_eq!(m.extra[0].name, "policy");
    }

    #[test]
    fn writes_back_what_it_read() {
        let m = parse(XML).unwrap();
        let again = parse(&to_xml(&m)).unwrap();
        assert_eq!(again, m);
        assert!(to_xml(&m).contains("<colormode>bt2100</colormode>"));
    }

    #[test]
    fn picks_the_configuration_of_the_connected_screens() {
        let m = parse(XML).unwrap();
        assert!(find(&m, &BTreeSet::from([dell(), lg(), panel()])).is_some());
        assert!(find(&m, &BTreeSet::from([dell()])).is_none());
    }

    #[test]
    fn hyprlands_monitor_lines() {
        let m = parse(XML).unwrap();
        assert_eq!(
            hyprland_lines(&m.configurations[0]),
            [
                "DP-1, 5120x2160@119.996, 0x0, 1.25, vrr, 1",
                "HDMI-A-1, 3840x2160@60.000, 4096x0, 1, transform, 1",
                "eDP-1, disable",
            ]
        );
        assert!(hyprland_conf(&["DP-1, disable".into()]).ends_with("monitor = DP-1, disable\n"));
    }

    #[test]
    fn a_new_set_gets_each_screens_last_settings() {
        let m = parse(XML).unwrap();
        assert_eq!(
            fallback_lines(&m, &BTreeSet::from([dell()])),
            ["DP-1, 5120x2160@119.996, auto, 1.25"]
        );
        assert_eq!(
            fallback_lines(&m, &BTreeSet::from([spec("DP-2", "X", "Y", "Z")])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn mirrored_and_physical_layouts() {
        let mut m = parse(XML).unwrap();
        let c = &mut m.configurations[0];
        c.layout = Some(LayoutMode::Physical);
        c.logical[1].scale = 2.0;
        let mirror = c.logical[1].monitors[0].clone();
        c.logical[0].monitors.push(Monitor {
            spec: spec("DP-2", "A", "B", "C"),
            ..mirror
        });
        let lines = hyprland_lines(c);
        assert_eq!(lines[1], "DP-2, 3840x2160@60.000, 0x0, 1.25, mirror, DP-1");
        assert_eq!(
            lines[2],
            "HDMI-A-1, 3840x2160@60.000, 2048x0, 2, transform, 1"
        );
    }

    #[test]
    fn storing_replaces_the_same_set() {
        let mut m = parse(XML).unwrap();
        let mut c = m.configurations[0].clone();
        c.logical[0].scale = 1.5;
        put(&mut m, c);
        assert_eq!(m.configurations.len(), 1);
        assert_eq!(m.configurations[0].logical[0].scale, 1.5);
    }

    #[test]
    fn entities_in_names() {
        let xml = XML.replace("DELL U4025QW", "Dell &amp; Co &#8211; 40\"");
        let m = parse(&xml).unwrap();
        let name = &m.configurations[0].logical[0].monitors[0].spec.id.product;
        assert_eq!(name, "Dell & Co – 40\"");
        assert!(to_xml(&m).contains("<product>Dell &amp; Co – 40&quot;</product>"));
        assert_eq!(parse(&to_xml(&m)).unwrap(), m);
    }

    #[test]
    fn connectors_from_sysfs_names() {
        assert_eq!(connector_of("card1-DP-3"), Some("DP-3"));
        assert_eq!(connector_of("card0-HDMI-A-1"), Some("HDMI-A-1"));
        assert_eq!(connector_of("card1"), None);
        assert_eq!(connector_of("renderD128"), None);
        assert_eq!(connector_of("card1-Writeback-1"), None);
        assert_eq!(connector_of("cardX-DP-1"), None);
    }

    #[test]
    fn rejects_other_files() {
        assert!(matches!(
            parse("<monitors version=\"1\"></monitors>"),
            Err(MonitorsError::Version(_))
        ));
        assert!(parse("<other/>").is_err());
        assert!(parse("<monitors version=\"2\">").is_err());
    }
}
