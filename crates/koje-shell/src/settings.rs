//! The settings app (M8.9–M8.15, ADR 0036, 0042): what its pages show and
//! what an edited form becomes, without GTK. The data comes from koje's files
//! (through `koje-core`) and kojed; changes go through the `koje` CLI, like
//! the bar's, so the app and the CLI check them the same way.

use koje_core::access::{AccessConfig, Profile};
use koje_core::color::Color;
use koje_core::config::{ContextConfig, GlobalConfig, WorkspaceStyle};
use koje_core::context::ContextSettings;
use koje_core::look::{BarSettings, HostWorkspace, OverlaySettings};
use koje_core::size::ByteSize;
use koje_core::state::State;
use koje_core::states::{StateInfo, Summary};

/// A context's status, as kojed reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Running,
    Paused,
    #[default]
    Stopped,
    /// kojed does not answer.
    Unknown,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "stopped" => Self::Stopped,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::Unknown => "",
        }
    }

    pub fn up(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

/// Everything the app shows about one context.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextItem {
    pub name: String,
    /// Its `context.toml`, or why it cannot be read.
    pub config: Result<ContextConfig, String>,
    pub status: Status,
    /// `(used, soft, hard)` in bytes; 0 is no limit. Running contexts only.
    pub memory: Option<(u64, u64, u64)>,
    pub states: Vec<StateInfo>,
    /// The live state of a running context, else its current state's file.
    pub live: Option<State>,
}

/// What the backend reads every few seconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub contexts: Vec<ContextItem>,
    pub global: GlobalConfig,
    pub kojed: bool,
}

/// The context page's fields as the user sees them, all text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Form {
    pub prefix: String,
    /// Empty: the context's name.
    pub label: String,
    /// Empty: koje's default icon.
    pub icon: String,
    pub color: String,
    /// `None`: the profile from `config.toml`.
    pub profile: Option<Profile>,
    /// Every switch: `(name, override)`; `None` follows the profile.
    pub switches: Vec<(&'static str, Option<bool>)>,
    pub mounts: Vec<String>,
    pub memory_soft: String,
    pub memory_hard: String,
}

/// The form of a context's config.
pub fn form(cfg: &ContextConfig) -> Form {
    let overrides = overrides(&cfg.access);
    Form {
        prefix: cfg.prefix.clone(),
        label: cfg.label.clone().unwrap_or_default(),
        icon: cfg.icon.clone().unwrap_or_default(),
        color: cfg.color.to_string(),
        profile: cfg.access.profile,
        switches: overrides,
        mounts: cfg.mounts.iter().map(|m| m.path.to_string()).collect(),
        memory_soft: cfg
            .limits
            .memory_soft
            .map(|s| s.to_string())
            .unwrap_or_default(),
        memory_hard: cfg
            .limits
            .memory_hard
            .map(|s| s.to_string())
            .unwrap_or_default(),
    }
}

/// The switches a context sets itself.
fn overrides(access: &AccessConfig) -> Vec<(&'static str, Option<bool>)> {
    // A config with every switch on lists the names; the context's own
    // values are read back through serde (the struct is macro-made).
    let set = serde_json::to_value(access).unwrap_or_default();
    koje_core::access::Access::ALL
        .switches()
        .into_iter()
        .map(|(name, _)| (name, set.get(name).and_then(serde_json::Value::as_bool)))
        .collect()
}

/// What each switch is with this form: the profile (the context's, else
/// `config.toml`'s) with the context's overrides.
pub fn effective(form: &Form, global: &GlobalConfig) -> Vec<(&'static str, bool)> {
    let profile = form
        .profile
        .or(global.access.profile)
        .unwrap_or_default()
        .access();
    let base = serde_json::to_value(profile).unwrap_or_default();
    let global_set = serde_json::to_value(&global.access).unwrap_or_default();
    form.switches
        .iter()
        .map(|(name, own)| {
            let from_global = if form.profile.is_none() {
                global_set.get(*name).and_then(serde_json::Value::as_bool)
            } else {
                None
            };
            let base = base
                .get(*name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            (*name, own.or(from_global).unwrap_or(base))
        })
        .collect()
}

/// A field that cannot be saved, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

/// The form as `context.toml` edits; every bad field reported.
pub fn settings(form: &Form) -> Result<ContextSettings, Vec<FieldError>> {
    let mut errors = Vec::new();
    let mut err = |field, message: String| errors.push(FieldError { field, message });
    let prefix = form.prefix.trim().to_owned();
    if prefix.is_empty() {
        err("prefix", "A short label is needed".to_owned());
    }
    let optional = |text: &str| Some(text.trim().to_owned()).filter(|t| !t.is_empty());
    let (label, icon) = (optional(&form.label), optional(&form.icon));
    if icon
        .as_deref()
        .is_some_and(|i| !koje_core::look::valid_icon(i))
    {
        err("icon", "An icon name like folder-symbolic".to_owned());
    }
    let color = form.color.trim().parse::<Color>().map_err(|_| ()).ok();
    if color.is_none() {
        err("color", "A color as #RRGGBB".to_owned());
    }
    let size = |text: &str| -> Result<Option<ByteSize>, ()> {
        let t = text.trim();
        if t.is_empty() {
            Ok(None)
        } else {
            t.parse().map(Some).map_err(|_| ())
        }
    };
    let soft = size(&form.memory_soft);
    if soft.is_err() {
        err("memory_soft", "A size like 6G".to_owned());
    }
    let hard = size(&form.memory_hard);
    if hard.is_err() {
        err("memory_hard", "A size like 8G".to_owned());
    }
    if let (Ok(Some(s)), Ok(Some(h))) = (&soft, &hard)
        && s >= h
    {
        err(
            "memory_soft",
            "The warning has to be below the limit".to_owned(),
        );
    }
    let mounts: Vec<String> = form
        .mounts
        .iter()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ContextSettings {
        prefix,
        label,
        icon,
        color: color.unwrap_or(Color { r: 0, g: 0, b: 0 }),
        profile: form.profile,
        switches: form.switches.clone(),
        mounts,
        memory_soft: soft.ok().flatten(),
        memory_hard: hard.ok().flatten(),
    })
}

/// The bar styles as the Workspaces page offers them.
pub const STYLES: [(WorkspaceStyle, &str); 4] = [
    (WorkspaceStyle::Labels, "Icons and labels"),
    (WorkspaceStyle::ColoredDots, "Dots in the context colors"),
    (WorkspaceStyle::Dots, "Dots"),
    (WorkspaceStyle::Names, "Names"),
];

/// A host workspace on the Workspaces page; empty texts are unset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    pub number: u32,
    pub label: String,
    pub icon: String,
}

/// An overlay on the Workspaces page; empty texts are unset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRow {
    pub name: String,
    pub key: String,
    pub label: String,
    pub icon: String,
}

/// The Workspaces page's fields (ADR 0055).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarForm {
    pub style: WorkspaceStyle,
    pub workspaces: Vec<WorkspaceRow>,
    pub overlays: Vec<OverlayRow>,
    /// An overlay was changed: `[overlay]` is written (with koje's own
    /// overlays spelled out when the file had none).
    pub overlays_changed: bool,
}

/// The Workspaces page of `config.toml`.
pub fn bar_form(cfg: &GlobalConfig) -> BarForm {
    let own = |name: &str| cfg.overlay.as_ref().and_then(|m| m.get(name));
    BarForm {
        style: cfg.bar.workspaces,
        workspaces: koje_core::look::host_workspaces(cfg)
            .unwrap_or_default()
            .into_iter()
            .map(|(number, w)| WorkspaceRow {
                number,
                label: w.label.unwrap_or_default(),
                icon: w.icon.unwrap_or_default(),
            })
            .collect(),
        overlays: koje_core::overlay::overlays(cfg)
            .unwrap_or_default()
            .into_iter()
            .map(|o| OverlayRow {
                key: own(&o.name).map_or_else(|| o.key.to_string(), |c| c.key.clone()),
                label: own(&o.name)
                    .and_then(|c| c.label.clone())
                    .unwrap_or_default(),
                icon: own(&o.name)
                    .and_then(|c| c.icon.clone())
                    .unwrap_or_default(),
                name: o.name,
            })
            .collect(),
        overlays_changed: false,
    }
}

/// The smallest workspace number not on the page yet.
pub fn next_workspace(form: &BarForm) -> u32 {
    (1..=u32::MAX)
        .find(|n| !form.workspaces.iter().any(|w| w.number == *n))
        .unwrap_or(1)
}

/// The page as `config.toml` edits; every bad field reported.
pub fn bar_settings(form: &BarForm) -> Result<BarSettings, Vec<FieldError>> {
    let optional = |text: &str| Some(text.trim().to_owned()).filter(|t| !t.is_empty());
    let mut errors = Vec::new();
    let mut icon = |text: &str, what: &str| {
        let icon = optional(text);
        if icon
            .as_deref()
            .is_some_and(|i| !koje_core::look::valid_icon(i))
        {
            errors.push(FieldError {
                field: "icon",
                message: format!("{what}: an icon name like folder-symbolic"),
            });
        }
        icon
    };
    let workspaces = form
        .workspaces
        .iter()
        .map(|w| {
            let look = HostWorkspace {
                label: optional(&w.label),
                icon: icon(&w.icon, &format!("Workspace {}", w.number)),
            };
            (w.number, look)
        })
        .collect();
    let overlays: Vec<OverlaySettings> = form
        .overlays
        .iter()
        .map(|o| OverlaySettings {
            name: o.name.clone(),
            key: o.key.trim().to_owned(),
            label: optional(&o.label),
            icon: icon(&o.icon, &o.name),
        })
        .collect();
    for o in &overlays {
        if koje_core::overlay::KeyCombo::parse(&o.key).is_none() {
            errors.push(FieldError {
                field: "key",
                message: format!("{}: a key like SUPER+M", o.name),
            });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(BarSettings {
        style: form.style,
        workspaces,
        overlays: form.overlays_changed.then_some(overlays),
    })
}

/// A state's row: name, its role, what it holds.
pub fn state_row(s: &StateInfo) -> (String, String) {
    let role = s.role.map(|r| match r.as_str() {
        "default" => "Loaded by default",
        _ => "Fresh start",
    });
    let summary = match &s.summary {
        Some(Summary::Ok { windows, layouts }) => {
            format!(
                "{} · {}",
                plural(*windows, "window"),
                plural(*layouts, "layout")
            )
        }
        Some(Summary::Invalid(e)) => format!("Cannot be read: {e}"),
        None if s.name.as_str() == "empty" => "No windows".to_owned(),
        None => "Not saved yet".to_owned(),
    };
    let subtitle = match role {
        Some(role) => format!("{role} · {summary}"),
        None => summary,
    };
    (s.name.to_string(), subtitle)
}

pub fn plural(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// The layouts × windows matrix of a state: per layout its name, whether it
/// is active, autosave, and per window whether it is shown there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub windows: Vec<(String, String)>,
    pub layouts: Vec<(String, bool, bool, Vec<bool>)>,
}

pub fn matrix(state: &State) -> Matrix {
    let windows: Vec<(String, String)> = state
        .windows
        .iter()
        .map(|w| (w.id.to_string(), window_label(w)))
        .collect();
    let layouts = state
        .layout
        .iter()
        .map(|(name, layout)| {
            let shown = state
                .windows
                .iter()
                .map(|w| layout.windows.contains_key(&w.id))
                .collect();
            (
                name.to_string(),
                *name == state.active_layout,
                layout.autosave,
                shown,
            )
        })
        .collect();
    Matrix { windows, layouts }
}

/// What a window is: its integration, else its app id or program.
pub fn window_label(w: &koje_core::state::WindowSpec) -> String {
    if let Some(i) = &w.integration {
        return i.clone();
    }
    if let Some(a) = &w.app_id {
        return a.clone();
    }
    w.command
        .as_deref()
        .and_then(|c| c.split_whitespace().next())
        .map_or_else(
            || "window".to_owned(),
            |c| c.rsplit('/').next().unwrap_or(c).to_owned(),
        )
}

/// Memory as the page shows it: used of the limit (hard, else soft).
pub fn memory_label(used: u64, soft: u64, hard: u64) -> String {
    let limit = if hard > 0 { hard } else { soft };
    if limit == 0 {
        format!("{} used", ByteSize(used).human())
    } else {
        format!("{} of {}", ByteSize(used).human(), ByteSize(limit).human())
    }
}

/// How full, 0–1, against the hard limit (else the soft one).
#[allow(clippy::cast_precision_loss, reason = "a fraction for a bar")]
pub fn memory_fraction(used: u64, soft: u64, hard: u64) -> f64 {
    let limit = if hard > 0 { hard } else { soft };
    if limit == 0 {
        0.0
    } else {
        (used as f64 / limit as f64).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn cfg(extra: &str) -> ContextConfig {
        let name = koje_core::name::ContextName::new("kp").unwrap();
        let text = koje_core::context::new_config_toml(&name, "KP", "#3584e4".parse().unwrap());
        ContextConfig::from_toml(&format!("{text}\n{extra}"), Path::new("context.toml")).unwrap()
    }

    #[test]
    fn the_form_round_trips() {
        let c =
            cfg("[access]\nprofile = \"desktop\"\ngpu = true\n[limits]\nmemory_soft = \"4G\"\n");
        let f = form(&c);
        assert_eq!(f.prefix, "KP");
        assert_eq!(f.profile, Some(Profile::Desktop));
        assert!(f.switches.contains(&("gpu", Some(true))));
        assert!(f.switches.contains(&("sound", None)));
        assert_eq!(f.memory_soft, "4G");
        let s = settings(&f).unwrap();
        assert_eq!(s.memory_soft.map(|b| b.to_string()).as_deref(), Some("4G"));
        assert_eq!(s.switches, f.switches);
    }

    #[test]
    fn switches_follow_profile_global_and_overrides() {
        let mut f = form(&cfg(""));
        let global = GlobalConfig::from_toml(
            "[access]\nprofile = \"headless\"\nsound = true\n",
            Path::new("c"),
        )
        .unwrap();
        let on = |f: &Form, name| {
            effective(f, &global)
                .into_iter()
                .find(|(n, _)| *n == name)
                .unwrap()
                .1
        };
        // The global profile and its override.
        assert!(!on(&f, "wayland"));
        assert!(on(&f, "sound"));
        // The context's own profile replaces the global one with its overrides.
        f.profile = Some(Profile::Full);
        assert!(on(&f, "wayland"));
        f.switches = f
            .switches
            .iter()
            .map(|(n, v)| (*n, if *n == "wayland" { Some(false) } else { *v }))
            .collect();
        assert!(!on(&f, "wayland"));
    }

    #[test]
    fn bad_fields_are_named() {
        let mut f = form(&cfg(""));
        f.prefix = " ".into();
        f.color = "red".into();
        f.memory_soft = "8G".into();
        f.memory_hard = "4G".into();
        let errors = settings(&f).unwrap_err();
        let fields: Vec<&str> = errors.iter().map(|e| e.field).collect();
        assert_eq!(fields, ["prefix", "color", "memory_soft"]);
        f = form(&cfg(""));
        f.memory_hard = "lots".into();
        assert_eq!(settings(&f).unwrap_err()[0].field, "memory_hard");
    }

    #[test]
    fn the_workspaces_page_round_trips() {
        let global = GlobalConfig::from_toml(
            "[bar]\nworkspaces = \"dots\"\n[workspace.2]\nlabel = \"Web\"\n",
            Path::new("c"),
        )
        .unwrap();
        let mut f = bar_form(&global);
        assert_eq!(f.style, WorkspaceStyle::Dots);
        assert_eq!(f.workspaces[0].label, "Web");
        // koje's own overlays, nothing of them in the file.
        assert_eq!(
            f.overlays
                .iter()
                .map(|o| (o.name.as_str(), o.key.as_str(), o.label.as_str()))
                .collect::<Vec<_>>(),
            [("chat", "SUPER+C", ""), ("mail", "SUPER+M", "")]
        );
        assert_eq!(next_workspace(&f), 1);
        let s = bar_settings(&f).unwrap();
        assert_eq!(s.overlays, None, "unchanged overlays stay as they are");
        assert_eq!(s.workspaces[&2].icon, None);

        f.overlays[1].icon = "mail-read-symbolic".into();
        f.overlays_changed = true;
        let s = bar_settings(&f).unwrap();
        assert_eq!(
            s.overlays.unwrap()[1].icon.as_deref(),
            Some("mail-read-symbolic")
        );

        f.overlays[0].key = "SUPER+".into();
        f.workspaces[0].icon = "../x".into();
        let fields: Vec<&str> = bar_settings(&f)
            .unwrap_err()
            .iter()
            .map(|e| e.field)
            .collect();
        assert_eq!(fields, ["icon", "key"]);
    }

    #[test]
    fn state_rows() {
        let roles = koje_core::config::StateRoles::default();
        let tmp = tempfile::tempdir().unwrap();
        let paths = koje_core::paths::Paths::with_roots(
            tmp.path().join("c"),
            tmp.path().join("d"),
            tmp.path().join("s"),
        );
        let name = koje_core::name::ContextName::new("kp").unwrap();
        let list = koje_core::states::list(&paths, &name, &roles).unwrap();
        let rows: Vec<(String, String)> = list.iter().map(state_row).collect();
        assert_eq!(
            rows[0],
            ("default".into(), "Loaded by default · Not saved yet".into())
        );
        assert_eq!(rows[2], ("empty".into(), "No windows".into()));
    }

    #[test]
    fn memory_texts() {
        let g = 1 << 30;
        assert_eq!(memory_fraction(g, 0, 4 * g), 0.25);
        assert_eq!(memory_fraction(g, 0, 0), 0.0);
        assert!(memory_label(g, 0, 4 * g).contains(" of "));
    }
}
