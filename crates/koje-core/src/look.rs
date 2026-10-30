//! Icons and labels (M8.89, ADR 0055): a context has an icon and a label,
//! from `label` and `icon` in `context.toml`. Unset values have defaults.

use crate::config::ContextConfig;
use crate::error::{Error, Result};

/// A context's icon when none is set.
pub const CONTEXT_ICON: &str = "folder-symbolic";

/// What a context shows: its label and icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub label: String,
    pub icon: String,
}

/// An icon theme name: letters, digits, `-`, `_` and `.`, no path.
pub fn valid_icon(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A label and an icon as written, checked; `what` names the entry.
pub fn check(what: &str, label: Option<&str>, icon: Option<&str>) -> Result<()> {
    let bad = |reason: String| Error::InvalidLook {
        what: what.to_owned(),
        reason,
    };
    if let Some(l) = label
        && l.trim().is_empty()
    {
        return Err(bad("the label is empty".to_owned()));
    }
    if let Some(i) = icon
        && !valid_icon(i)
    {
        return Err(bad(format!("{i:?} is no icon name (like folder-symbolic)")));
    }
    Ok(())
}

/// A context's look: its label, else its name.
pub fn context(cfg: &ContextConfig) -> Look {
    Look {
        label: cfg.label.clone().unwrap_or_else(|| cfg.name.to_string()),
        icon: cfg.icon.clone().unwrap_or_else(|| CONTEXT_ICON.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn contexts_default_to_their_name() {
        let base = "name = \"kp\"\nprefix = \"KP\"\ncolor = \"#2E7D5B\"\n";
        let cfg = ContextConfig::from_toml(base, Path::new("context.toml")).unwrap();
        assert_eq!(
            context(&cfg),
            Look {
                label: "kp".into(),
                icon: CONTEXT_ICON.into()
            }
        );
        let own = format!("{base}label = \"Kundenportal\"\nicon = \"briefcase-symbolic\"\n");
        let cfg = ContextConfig::from_toml(&own, Path::new("context.toml")).unwrap();
        assert_eq!(context(&cfg).label, "Kundenportal");
        assert_eq!(context(&cfg).icon, "briefcase-symbolic");
        let bad = format!("{base}icon = \"../x\"\n");
        assert!(ContextConfig::from_toml(&bad, Path::new("context.toml")).is_err());
    }
}
