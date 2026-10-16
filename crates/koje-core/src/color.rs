//! Context colors, used for window borders and UI accents.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// An opaque RGB color, written as `#RRGGBB` in configuration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Hyprland's color notation, e.g. `rgb(2e7d5b)`.
    pub fn to_hyprland(self) -> String {
        format!("rgb({:02x}{:02x}{:02x})", self.r, self.g, self.b)
    }
}

impl FromStr for Color {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let err = || Error::InvalidColor(s.to_owned());
        let hex = s.strip_prefix('#').ok_or_else(err)?;
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(err());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        Ok(Self {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
        })
    }
}

impl TryFrom<String> for Color {
    type Error = Error;
    fn try_from(s: String) -> Result<Self> {
        s.parse()
    }
}

impl From<Color> for String {
    fn from(c: Color) -> String {
        c.to_string()
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        let c: Color = "#2E7D5B".parse().unwrap();
        assert_eq!((c.r, c.g, c.b), (0x2e, 0x7d, 0x5b));
        assert_eq!(c.to_string(), "#2E7D5B");
        assert_eq!(c.to_hyprland(), "rgb(2e7d5b)");
        assert_eq!("#2e7d5b".parse::<Color>().unwrap(), c);
    }

    #[test]
    fn rejects_malformed() {
        for s in ["2E7D5B", "#2E7D5", "#2E7D5BFF", "#GGGGGG", ""] {
            assert!(s.parse::<Color>().is_err(), "{s:?}");
        }
    }
}
