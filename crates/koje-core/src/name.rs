//! Validated identifiers: context, state, layout and window names.
//!
//! All names share one rule so they are safe as file names, container labels
//! and Hyprland workspace names: 1–48 characters, lowercase ASCII letters,
//! digits and `-`, starting with a letter.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MAX_LEN: usize = 48;

fn validate(kind: &'static str, value: &str) -> Result<()> {
    let invalid = |reason| Error::InvalidName {
        kind,
        value: value.to_owned(),
        reason,
    };
    if value.is_empty() {
        return Err(invalid("must not be empty"));
    }
    if value.len() > MAX_LEN {
        return Err(invalid("must be at most 48 characters"));
    }
    if !value.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(invalid("must start with a lowercase letter"));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(invalid("may only contain a-z, 0-9 and '-'"));
    }
    Ok(())
}

macro_rules! name_type {
    ($(#[$meta:meta])* $ty:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $ty(String);

        impl $ty {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate($kind, &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $ty {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> {
                Self::new(s)
            }
        }

        impl TryFrom<String> for $ty {
            type Error = Error;
            fn try_from(s: String) -> Result<Self> {
                Self::new(s)
            }
        }

        impl From<$ty> for String {
            fn from(n: $ty) -> String {
                n.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // `pad` honours width and alignment, e.g. `{:<24}` in listings.
                f.pad(&self.0)
            }
        }
    };
}

name_type!(
    /// Name of a context, e.g. `kundenportal`.
    ContextName,
    "context"
);
name_type!(
    /// Name of a state, e.g. `default`, `fresh` or the reserved `empty`.
    StateName,
    "state"
);
name_type!(
    /// Name of a layout, e.g. `code` or `review`.
    LayoutName,
    "layout"
);
name_type!(
    /// Stable identifier of a window within a state, e.g. `editor` or `term-2`.
    WindowId,
    "window"
);

impl StateName {
    /// The reserved, virtual, read-only state that starts no windows.
    pub const EMPTY: &'static str = "empty";

    pub fn is_empty_state(&self) -> bool {
        self.0 == Self::EMPTY
    }
}

impl LayoutName {
    /// Name of the single layout a new state starts with (ADR 0011).
    pub const INITIAL: &'static str = "main";

    pub fn initial() -> Self {
        Self(Self::INITIAL.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_honours_width() {
        let n = ContextName::new("kp").unwrap();
        assert_eq!(format!("[{n:<4}]"), "[kp  ]");
    }

    #[test]
    fn accepts_valid_names() {
        for n in ["kp", "kundenportal", "shop-relaunch", "a1", "x-2-y"] {
            assert!(ContextName::new(n).is_ok(), "{n}");
        }
    }

    #[test]
    fn rejects_invalid_names() {
        for n in ["", "1abc", "-a", "Abc", "a_b", "a b", "ä", &"a".repeat(49)] {
            assert!(ContextName::new(n).is_err(), "{n:?}");
        }
    }

    #[test]
    fn empty_state_is_detected() {
        assert!(StateName::new("empty").unwrap().is_empty_state());
        assert!(!StateName::new("default").unwrap().is_empty_state());
    }

    #[test]
    fn deserializes_with_validation() {
        #[derive(Deserialize)]
        struct T {
            n: ContextName,
        }
        assert_eq!(toml::from_str::<T>("n = \"ok\"").unwrap().n.as_str(), "ok");
        assert!(toml::from_str::<T>("n = \"NOT ok\"").is_err());
    }
}
