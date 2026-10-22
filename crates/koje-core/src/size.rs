//! Byte sizes as written in configuration files, e.g. `"6G"` or `"512M"`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A size in bytes. Suffixes `K`, `M`, `G`, `T` are binary (powers of 1024),
/// matching what Podman and cgroups accept.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct ByteSize(pub u64);

const UNITS: [(char, u64); 4] = [
    ('T', 1 << 40),
    ('G', 1 << 30),
    ('M', 1 << 20),
    ('K', 1 << 10),
];

impl FromStr for ByteSize {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let err = || Error::InvalidSize(s.to_owned());
        let t = s.trim();
        let (digits, factor) = match t.chars().last() {
            Some(c) if c.is_ascii_alphabetic() => {
                let unit = c.to_ascii_uppercase();
                let factor = UNITS
                    .iter()
                    .find(|(u, _)| *u == unit)
                    .map(|(_, f)| *f)
                    .ok_or_else(err)?;
                (&t[..t.len() - 1], factor)
            }
            _ => (t, 1),
        };
        let n: u64 = digits.parse().map_err(|_| err())?;
        n.checked_mul(factor).map(ByteSize).ok_or_else(err)
    }
}

impl TryFrom<String> for ByteSize {
    type Error = Error;
    fn try_from(s: String) -> Result<Self> {
        s.parse()
    }
}

impl From<ByteSize> for String {
    fn from(b: ByteSize) -> String {
        b.to_string()
    }
}

impl fmt::Display for ByteSize {
    /// Uses the largest unit that divides the value exactly.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (unit, factor) in UNITS {
            if self.0 >= factor && self.0.is_multiple_of(factor) {
                return write!(f, "{}{unit}", self.0 / factor);
            }
        }
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_suffixes() {
        assert_eq!("6G".parse::<ByteSize>().unwrap().0, 6 << 30);
        assert_eq!("512m".parse::<ByteSize>().unwrap().0, 512 << 20);
        assert_eq!("1024".parse::<ByteSize>().unwrap().0, 1024);
    }

    #[test]
    fn round_trips() {
        for s in ["6G", "512M", "3K", "1T", "1000"] {
            assert_eq!(s.parse::<ByteSize>().unwrap().to_string(), s);
        }
    }

    #[test]
    fn rejects_malformed() {
        for s in ["", "G", "6X", "-1G", "1.5G", "99999999999T"] {
            assert!(s.parse::<ByteSize>().is_err(), "{s:?}");
        }
    }
}
