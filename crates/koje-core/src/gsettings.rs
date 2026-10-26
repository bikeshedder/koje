//! The gsettings bridge (ADR 0044, M8.43): GNOME's settings, as GNOME
//! Settings writes them, turned into Hyprland's. Pure: kojed reads them with
//! `gsettings list-recursively`, watches them with `gsettings monitor` and
//! applies the result with Hyprland's `keyword`.
//!
//! Values come in `GLib`'s text notation (`GVariant` printed by `gsettings`):
//! `'us'`, `true`, `0.25`, `uint32 500`, `['caps:escape']`, `@as []`,
//! `[('xkb', 'us'), ('xkb', 'de+nodeadkeys')]`.

use std::collections::BTreeMap;

/// A value in `GLib`'s text notation – the shapes GNOME's input settings use.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Int(i64),
    Double(f64),
    Array(Vec<Value>),
    Tuple(Vec<Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(d) => Some(*d),
            #[allow(clippy::cast_precision_loss, reason = "small settings values")]
            Self::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// Parse one value; `None` for anything this does not know.
pub fn parse_value(text: &str) -> Option<Value> {
    let mut p = Parser {
        chars: text.trim().chars().collect(),
        at: 0,
    };
    let v = p.value()?;
    p.skip_space();
    (p.at == p.chars.len()).then_some(v)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_space();
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> String {
        self.skip_space();
        let start = self.at;
        while self.peek().is_some_and(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '@' | '(' | ')' | '_' | '.' | '-' | '+')
        }) {
            // A type annotation's parentheses (`@a(ss)`) belong to it, a
            // tuple's do not: stop at `(` unless inside an `@` word.
            if self.peek() == Some('(') && !self.chars[start..self.at].starts_with(&['@']) {
                break;
            }
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    fn value(&mut self) -> Option<Value> {
        self.skip_space();
        match self.peek()? {
            '\'' | '"' => self.string().map(Value::Str),
            '[' => self.list(']').map(Value::Array),
            '(' => self.list(')').map(Value::Tuple),
            _ => {
                let w = self.word();
                match w.as_str() {
                    "true" => Some(Value::Bool(true)),
                    "false" => Some(Value::Bool(false)),
                    // A type annotation: the value follows.
                    t if t.starts_with('@')
                        || matches!(
                            t,
                            "byte" | "int16" | "uint16" | "int32" | "uint32" | "int64" | "uint64"
                        ) =>
                    {
                        self.value()
                    }
                    n if n.contains(['.', 'e']) && !n.starts_with("0x") => {
                        n.parse().ok().map(Value::Double)
                    }
                    n => n.parse().ok().map(Value::Int),
                }
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.at += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.at += 1;
            match c {
                '\\' => {
                    let e = self.peek()?;
                    self.at += 1;
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
                c if c == quote => return Some(out),
                c => out.push(c),
            }
        }
    }

    fn list(&mut self, close: char) -> Option<Vec<Value>> {
        self.at += 1;
        let mut items = Vec::new();
        if self.eat(close) {
            return Some(items);
        }
        loop {
            items.push(self.value()?);
            if self.eat(close) {
                return Some(items);
            }
            if !self.eat(',') {
                return None;
            }
        }
    }
}

/// `gsettings list-recursively <schema>`: `schema key value` per line, as key
/// → value. Lines it cannot read are left out.
pub fn parse_list(text: &str) -> BTreeMap<String, Value> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            let (_schema, key, value) = (parts.next()?, parts.next()?, parts.next()?);
            Some((key.to_owned(), parse_value(value)?))
        })
        .collect()
}

/// The schemas the input bridge reads.
pub const INPUT_SOURCES: &str = "org.gnome.desktop.input-sources";
pub const KEYBOARD: &str = "org.gnome.desktop.peripherals.keyboard";
pub const MOUSE: &str = "org.gnome.desktop.peripherals.mouse";
pub const TOUCHPAD: &str = "org.gnome.desktop.peripherals.touchpad";
pub const INPUT_SCHEMAS: [&str; 4] = [INPUT_SOURCES, KEYBOARD, MOUSE, TOUCHPAD];

/// GNOME's input settings, schema → key → value.
pub type Settings = BTreeMap<String, BTreeMap<String, Value>>;

/// Hyprland's input settings for GNOME's, as `keyword` name and value, in a
/// stable order. Keys missing from `settings` (a schema not installed, an
/// older GNOME) are left to Hyprland's config.
pub fn input_keywords(settings: &Settings) -> Vec<(String, String)> {
    let get = |schema: &str, key: &str| settings.get(schema).and_then(|s| s.get(key));
    let flag = |b: bool| if b { "true" } else { "false" }.to_owned();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut set = |name: &str, value: String| out.push((name.to_owned(), value));

    // Keyboard layouts: XKB sources only (input methods are IBus's).
    if let Some(sources) = get(INPUT_SOURCES, "sources").and_then(Value::as_array) {
        let xkb: Vec<&str> = sources
            .iter()
            .filter_map(|s| match s {
                Value::Tuple(t) if t.first().and_then(Value::as_str) == Some("xkb") => {
                    t.get(1).and_then(Value::as_str)
                }
                _ => None,
            })
            .collect();
        if !xkb.is_empty() {
            let (layouts, variants): (Vec<&str>, Vec<&str>) = xkb
                .iter()
                .map(|s| s.split_once('+').unwrap_or((s, "")))
                .unzip();
            set("input:kb_layout", layouts.join(","));
            set("input:kb_variant", variants.join(","));
        }
    }
    if let Some(options) = get(INPUT_SOURCES, "xkb-options").and_then(Value::as_array) {
        let options: Vec<&str> = options.iter().filter_map(Value::as_str).collect();
        set("input:kb_options", options.join(","));
    }

    // Key repeat: delay in ms, interval in ms between repeats → per second.
    if let Some(delay) = get(KEYBOARD, "delay").and_then(Value::as_int) {
        set("input:repeat_delay", delay.to_string());
    }
    let repeat = get(KEYBOARD, "repeat").and_then(Value::as_bool);
    let interval = get(KEYBOARD, "repeat-interval").and_then(Value::as_int);
    match (repeat, interval) {
        (Some(false), _) => set("input:repeat_rate", "0".to_owned()),
        (_, Some(ms)) if ms > 0 => set("input:repeat_rate", ((1000 + ms / 2) / ms).to_string()),
        _ => {}
    }

    // Mouse: GNOME's speed is libinput's (-1 … 1), as Hyprland's sensitivity.
    if let Some(speed) = get(MOUSE, "speed").and_then(Value::as_f64) {
        set(
            "input:sensitivity",
            format!("{:.3}", speed.clamp(-1.0, 1.0)),
        );
    }
    if let Some(profile) = get(MOUSE, "accel-profile").and_then(Value::as_str) {
        let p = match profile {
            "flat" => "flat",
            "adaptive" => "adaptive",
            _ => "",
        };
        set("input:accel_profile", p.to_owned());
    }
    if let Some(b) = get(MOUSE, "natural-scroll").and_then(Value::as_bool) {
        set("input:natural_scroll", flag(b));
    }
    if let Some(b) = get(MOUSE, "left-handed").and_then(Value::as_bool) {
        set("input:left_handed", flag(b));
    }

    // Touchpad.
    if let Some(b) = get(TOUCHPAD, "natural-scroll").and_then(Value::as_bool) {
        set("input:touchpad:natural_scroll", flag(b));
    }
    if let Some(b) = get(TOUCHPAD, "tap-to-click").and_then(Value::as_bool) {
        set("input:touchpad:tap-to-click", flag(b));
    }
    if let Some(b) = get(TOUCHPAD, "disable-while-typing").and_then(Value::as_bool) {
        set("input:touchpad:disable_while_typing", flag(b));
    }
    if let Some(method) = get(TOUCHPAD, "click-method").and_then(Value::as_str) {
        // `fingers`: one, two, three fingers for left, right, middle; else
        // the button areas.
        set(
            "input:touchpad:clickfinger_behavior",
            flag(method == "fingers"),
        );
    }
    out
}

/// What changed since `applied`: the keywords to send again.
pub fn changed<'a>(
    now: &'a [(String, String)],
    applied: &[(String, String)],
) -> Vec<&'a (String, String)> {
    now.iter().filter(|kv| !applied.contains(kv)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_in_glibs_notation() {
        assert_eq!(parse_value("'us'"), Some(Value::Str("us".into())));
        assert_eq!(parse_value("true"), Some(Value::Bool(true)));
        assert_eq!(parse_value("uint32 500"), Some(Value::Int(500)));
        assert_eq!(parse_value("-0.25"), Some(Value::Double(-0.25)));
        assert_eq!(parse_value("@as []"), Some(Value::Array(vec![])));
        assert_eq!(parse_value("@a(ss) []"), Some(Value::Array(vec![])));
        assert_eq!(
            parse_value("['caps:escape', 'compose:ralt']"),
            Some(Value::Array(vec![
                Value::Str("caps:escape".into()),
                Value::Str("compose:ralt".into())
            ]))
        );
        assert_eq!(
            parse_value("[('xkb', 'us'), ('xkb', 'de+nodeadkeys'), ('ibus', 'anthy')]")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(parse_value(r"'it\'s'"), Some(Value::Str("it's".into())));
        assert_eq!(parse_value("[unclosed"), None);
        assert_eq!(parse_value("'a' trailing"), None);
    }

    fn settings(lines: &str) -> Settings {
        let mut s = Settings::new();
        for l in lines.lines() {
            let schema = l.split(' ').next().unwrap().to_owned();
            s.entry(schema).or_default().extend(parse_list(l));
        }
        s
    }

    #[test]
    fn gnomes_input_settings_as_hyprlands() {
        let s = settings(
            "org.gnome.desktop.input-sources sources [('xkb', 'de+nodeadkeys'), ('ibus', 'anthy'), ('xkb', 'us')]
org.gnome.desktop.input-sources xkb-options ['caps:escape']
org.gnome.desktop.peripherals.keyboard repeat true
org.gnome.desktop.peripherals.keyboard delay uint32 500
org.gnome.desktop.peripherals.keyboard repeat-interval uint32 30
org.gnome.desktop.peripherals.mouse speed 0.25
org.gnome.desktop.peripherals.mouse accel-profile 'flat'
org.gnome.desktop.peripherals.mouse natural-scroll false
org.gnome.desktop.peripherals.mouse left-handed false
org.gnome.desktop.peripherals.touchpad natural-scroll true
org.gnome.desktop.peripherals.touchpad tap-to-click true
org.gnome.desktop.peripherals.touchpad disable-while-typing true
org.gnome.desktop.peripherals.touchpad click-method 'fingers'",
        );
        let kw: Vec<String> = input_keywords(&s)
            .into_iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        assert_eq!(
            kw,
            [
                "input:kb_layout de,us",
                "input:kb_variant nodeadkeys,",
                "input:kb_options caps:escape",
                "input:repeat_delay 500",
                "input:repeat_rate 33",
                "input:sensitivity 0.250",
                "input:accel_profile flat",
                "input:natural_scroll false",
                "input:left_handed false",
                "input:touchpad:natural_scroll true",
                "input:touchpad:tap-to-click true",
                "input:touchpad:disable_while_typing true",
                "input:touchpad:clickfinger_behavior true",
            ]
        );
    }

    #[test]
    fn no_repeat_no_layouts_missing_schemas() {
        let s = settings(
            "org.gnome.desktop.input-sources sources @a(ss) []
org.gnome.desktop.input-sources xkb-options @as []
org.gnome.desktop.peripherals.keyboard repeat false
org.gnome.desktop.peripherals.keyboard repeat-interval uint32 30",
        );
        let kw = input_keywords(&s);
        assert_eq!(
            kw,
            [
                ("input:kb_options".to_owned(), String::new()),
                ("input:repeat_rate".to_owned(), "0".to_owned())
            ],
            "no layouts in GNOME: Hyprland's config keeps them"
        );
        assert_eq!(input_keywords(&Settings::new()), []);
    }

    #[test]
    fn only_changes_are_sent_again() {
        let applied = vec![("input:kb_layout".to_owned(), "us".to_owned())];
        let now = vec![
            ("input:kb_layout".to_owned(), "us".to_owned()),
            ("input:sensitivity".to_owned(), "0.100".to_owned()),
        ];
        assert_eq!(changed(&now, &applied), [&now[1]]);
    }
}
