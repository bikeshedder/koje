//! What `io.koje.Daemon1` reads and changes besides the live windows (G.23,
//! ADR 0060): contexts with their look and status, states, a context's
//! settings as a D-Bus dict, and changes to them. Blocking (files, Podman):
//! [`crate::api`] runs these on tokio's blocking pool.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context as _, Result, bail};
use koje_core::access::{Access, Profile};
use koje_core::color::Color;
use koje_core::config::{ContextConfig, GlobalConfig};
use koje_core::context::{self, ContextSettings};
use koje_core::name::{ContextName, StateName};
use koje_core::paths::Paths;
use koje_core::ports::{ContainerRuntime as _, ContainerStatus};
use koje_core::size::ByteSize;
use koje_core::states::{self, Role, Summary};
use koje_podman::Podman;
use zbus::zvariant::{OwnedValue, Value};

/// `(name, label, icon, color, status, windows)`: `Contexts`.
pub type ContextRow = (String, String, String, String, String, u32);

/// What kojed knows about running contexts.
#[derive(Debug, Default)]
pub struct Live {
    pub running: BTreeSet<ContextName>,
    pub paused: BTreeSet<ContextName>,
    pub windows: BTreeMap<ContextName, usize>,
}

/// Every context in name order. `status`: `running`, `paused`, `stopped`, or
/// `invalid` when its `context.toml` cannot be read (then label is the name,
/// icon and color are empty).
pub fn contexts(paths: &Paths, live: &Live) -> Result<Vec<ContextRow>> {
    let mut rows = Vec::new();
    for name in context::names(paths)? {
        let windows =
            u32::try_from(live.windows.get(&name).copied().unwrap_or(0)).unwrap_or(u32::MAX);
        let status = if live.paused.contains(&name) {
            "paused"
        } else if live.running.contains(&name) {
            "running"
        } else {
            "stopped"
        };
        rows.push(match context::load(paths, &name) {
            Ok(cfg) => {
                let look = koje_core::look::context(&cfg);
                (
                    name.to_string(),
                    look.label,
                    look.icon,
                    cfg.color.to_string(),
                    status.to_owned(),
                    windows,
                )
            }
            Err(_) => (
                name.to_string(),
                name.to_string(),
                String::new(),
                String::new(),
                "invalid".to_owned(),
                windows,
            ),
        });
    }
    Ok(rows)
}

/// `(name, role, current, stored, windows, layouts, modified, error)`: `States`.
pub type StateRow = (String, String, bool, bool, u32, u32, i64, String);

/// A context's states as `koje state list` shows them: stored ones, role
/// states not saved yet, `empty`. `modified` in seconds since the epoch, 0
/// when unknown.
pub fn states(
    paths: &Paths,
    ctx: &ContextName,
    current: Option<&StateName>,
) -> Result<Vec<StateRow>> {
    let cfg = context::load(paths, ctx)?;
    let n = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
    Ok(states::list(paths, ctx, &cfg.states)?
        .into_iter()
        .map(|s| {
            let (stored, windows, layouts, error) = match &s.summary {
                Some(Summary::Ok { windows, layouts }) => {
                    (true, n(*windows), n(*layouts), String::new())
                }
                Some(Summary::Invalid(e)) => (true, 0, 0, e.clone()),
                None => (false, 0, 0, String::new()),
            };
            let modified = s
                .modified
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
            (
                s.name.to_string(),
                s.role.map(Role::as_str).unwrap_or_default().to_owned(),
                current == Some(&s.name),
                stored,
                windows,
                layouts,
                modified,
                error,
            )
        })
        .collect())
}

fn value<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
    // Plain values (strings, numbers, maps of them) have no file
    // descriptors, the only thing that fails to convert.
    v.into()
        .try_to_owned()
        .unwrap_or_else(|_| OwnedValue::from(0u8))
}

fn switches(access: &Access) -> HashMap<String, bool> {
    access
        .switches()
        .into_iter()
        .map(|(name, on)| (name.to_owned(), on))
        .collect()
}

/// A context's settings as `Config` returns them:
///
/// - `name`, `prefix`, `label`, `icon`, `color`, `image`: `s` (`label` and
///   `icon` empty when unset),
/// - `profile`: `s`, empty for `config.toml`'s; `default_profile`: `s`,
///   `config.toml`'s (`full` when unset),
/// - `access`: `a{sb}`, the switches the context sets itself; `effective`:
///   `a{sb}`, every switch as the context gets it; `bases`: `a{sa{sb}}`,
///   every profile's switches by name, `config.toml`'s under `""`,
/// - `mounts`: `as`, paths as written,
/// - `memory_soft`, `memory_hard`: `t`, bytes, 0 for none,
/// - `container`: `b`, whether it has a container (Recreate needs one),
/// - `image_file`: `s`, the Containerfile of its image folder, empty for
///   koje's and registry images; `image_outdated`: `s`, why its container
///   does not run its image as it is now (ADR 0063), empty when it does.
pub fn config(paths: &Paths, ctx: &ContextName) -> Result<HashMap<String, OwnedValue>> {
    let cfg = context::load(paths, ctx)?;
    let global = GlobalConfig::load(&paths.global_config())?;
    let s = ContextSettings::of(&cfg);
    let overrides: HashMap<String, bool> = s
        .switches
        .iter()
        .filter_map(|(name, on)| on.map(|on| ((*name).to_owned(), on)))
        .collect();
    let mut out = HashMap::new();
    let mut put = |key: &str, v: OwnedValue| {
        out.insert(key.to_owned(), v);
    };
    put("name", value(cfg.name.to_string()));
    put("prefix", value(s.prefix));
    put("label", value(s.label.unwrap_or_default()));
    put("icon", value(s.icon.unwrap_or_default()));
    put("color", value(s.color.to_string()));
    put("image", value(cfg.image.clone()));
    put(
        "profile",
        value(s.profile.map(Profile::as_str).unwrap_or_default()),
    );
    put(
        "default_profile",
        value(global.access.profile.unwrap_or_default().as_str()),
    );
    put("access", value(overrides));
    put("effective", value(switches(&cfg.effective_access(&global))));
    // Each profile's switches, and config.toml's under "": a form shows the
    // switches of a profile chosen before it is saved.
    let mut bases: HashMap<String, HashMap<String, bool>> = Profile::ALL
        .into_iter()
        .map(|p| (p.as_str().to_owned(), switches(&p.access())))
        .collect();
    bases.insert(String::new(), switches(&global.access.standalone()));
    put("bases", value(bases));
    put("mounts", value(s.mounts));
    put("memory_soft", value(s.memory_soft.map_or(0, |b| b.0)));
    put("memory_hard", value(s.memory_hard.map_or(0, |b| b.0)));
    let container = koje_podman::Podman::new()
        .status(ctx)
        .is_ok_and(|s| s != ContainerStatus::Missing);
    put("container", value(container));
    // Shown, not refused: a broken image folder is the user's to fix.
    let file = koje_runtime::images::folder_file(paths, &cfg.image)
        .ok()
        .flatten()
        .map(|f| f.display().to_string());
    put("image_file", value(file.unwrap_or_default()));
    let outdated = koje_runtime::images::outdated(paths, ctx, &cfg.image).unwrap_or_else(|e| {
        tracing::debug!(context = %ctx, "comparing the container with its image: {e:#}");
        None
    });
    put("image_outdated", value(outdated.unwrap_or_default()));
    Ok(out)
}

/// Why `SetConfig` refused a change, for `io.koje.Error.InvalidConfig`.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

fn get<'a, T>(dict: &'a HashMap<String, OwnedValue>, key: &str) -> Result<Option<T>>
where
    T: TryFrom<&'a OwnedValue>,
    <T as TryFrom<&'a OwnedValue>>::Error: std::fmt::Display,
{
    dict.get(key)
        .map(|v| T::try_from(v).map_err(|e| Invalid(format!("{key}: {e}")).into()))
        .transpose()
}

/// `settings` with the keys of `dict` applied (the keys of [`config`] that
/// can change: `prefix`, `label`, `icon`, `color`, `profile`, `access`,
/// `mounts`, `memory_soft`, `memory_hard`). Keys not given stay; an empty
/// `label`, `icon` or `profile` removes it; `access` replaces all overrides.
pub fn apply(
    mut settings: ContextSettings,
    dict: &HashMap<String, OwnedValue>,
) -> Result<ContextSettings> {
    const KEYS: [&str; 9] = [
        "prefix",
        "label",
        "icon",
        "color",
        "profile",
        "access",
        "mounts",
        "memory_soft",
        "memory_hard",
    ];
    if let Some(key) = dict.keys().find(|k| !KEYS.contains(&k.as_str())) {
        bail!(Invalid(format!("{key} cannot be set")));
    }
    let text =
        |key: &str| -> Result<Option<String>> { Ok(get::<&str>(dict, key)?.map(str::to_owned)) };
    if let Some(p) = text("prefix")? {
        settings.prefix = p;
    }
    if let Some(l) = text("label")? {
        settings.label = (!l.is_empty()).then_some(l);
    }
    if let Some(i) = text("icon")? {
        settings.icon = (!i.is_empty()).then_some(i);
    }
    if let Some(c) = text("color")? {
        settings.color = c
            .parse::<Color>()
            .map_err(|e| Invalid(format!("color: {e}")))?;
    }
    if let Some(p) = text("profile")? {
        settings.profile = if p.is_empty() {
            None
        } else {
            Some(
                Profile::ALL
                    .into_iter()
                    .find(|x| x.as_str() == p)
                    .ok_or_else(|| Invalid(format!("profile: no profile {p:?}")))?,
            )
        };
    }
    if let Some(v) = dict.get("access") {
        let set: HashMap<String, bool> =
            HashMap::try_from(v.try_clone().map_err(|e| Invalid(format!("access: {e}")))?)
                .map_err(|e| Invalid(format!("access: {e}")))?;
        if let Some(unknown) = set
            .keys()
            .find(|k| !settings.switches.iter().any(|(name, _)| name == k))
        {
            bail!(Invalid(format!("access: no switch {unknown:?}")));
        }
        for (name, on) in &mut settings.switches {
            *on = set.get(*name).copied();
        }
    }
    if let Some(v) = dict.get("mounts") {
        settings.mounts =
            Vec::<String>::try_from(v.try_clone().map_err(|e| Invalid(format!("mounts: {e}")))?)
                .map_err(|e| Invalid(format!("mounts: {e}")))?;
    }
    for (key, slot) in [
        ("memory_soft", &mut settings.memory_soft),
        ("memory_hard", &mut settings.memory_hard),
    ] {
        if let Some(bytes) = get::<u64>(dict, key)? {
            *slot = (bytes > 0).then_some(ByteSize(bytes));
        }
    }
    Ok(settings)
}

/// Write `dict` into the context's `context.toml` (comments kept), checked
/// as `koje edit` checks it, then follow it where a context does live: its
/// mounts, its dotfiles, its kernel memory limit. Returns notes for the user
/// (e.g. a change that needs `recreate`).
pub fn set_config(
    paths: &Paths,
    ctx: &ContextName,
    dict: &HashMap<String, OwnedValue>,
) -> Result<Vec<String>> {
    let before = context::load(paths, ctx)?;
    let path = paths.context_config(ctx);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let settings = apply(ContextSettings::of(&before), dict)?;
    let new = context::with_settings(&text, &settings).map_err(|e| Invalid(e.to_string()))?;
    let after = context::replace_config(paths, ctx, &new).map_err(|e| Invalid(e.to_string()))?;
    Ok(follow(paths, ctx, &before, &after))
}

/// A context's dash favorites (ADR 0053, 0062): desktop file ids, `None`
/// when it has none of its own.
pub fn favorites(paths: &Paths, ctx: &ContextName) -> Result<Option<Vec<String>>> {
    Ok(context::load(paths, ctx)?.favorites)
}

/// Set them (`None`: none of its own) in `context.toml`, comments kept.
pub fn set_favorites(paths: &Paths, ctx: &ContextName, favorites: Option<&[String]>) -> Result<()> {
    context::load(paths, ctx)?;
    let path = paths.context_config(ctx);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let new = context::with_favorites(&text, favorites).map_err(|e| Invalid(e.to_string()))?;
    context::replace_config(paths, ctx, &new).map_err(|e| Invalid(e.to_string()))?;
    Ok(())
}

/// What a running or created context does after its config changed (as
/// `koje edit`); notes for what it cannot do.
fn follow(
    paths: &Paths,
    ctx: &ContextName,
    before: &ContextConfig,
    after: &ContextConfig,
) -> Vec<String> {
    let mut notes = Vec::new();
    let podman = Podman::new();
    let status = podman.status(ctx).unwrap_or(ContainerStatus::Missing);
    if let Ok(global) = GlobalConfig::load(&paths.global_config()) {
        let fp = |c: &ContextConfig| {
            koje_core::container::fingerprint(&c.image, &c.effective_access(&global))
        };
        if status != ContainerStatus::Missing && fp(before) != fp(after) {
            notes.push("the new image or access apply to a new container: recreate it".to_owned());
        }
        if matches!(status, ContainerStatus::Running | ContainerStatus::Paused)
            && before.limits != after.limits
            && let Ok(limits) = after.effective_limits(&global)
            && let Err(e) = koje_runtime::memory::apply_kernel_limit(&podman, ctx, &limits)
        {
            notes.push(format!("the memory limit applies at the next start: {e:#}"));
        }
    }
    if let Err(e) = koje_runtime::mounts::sync(paths, ctx, after, true) {
        notes.push(format!("mounts not changed: {e:#}"));
    }
    if status == ContainerStatus::Running
        && (after.dotfiles != before.dotfiles || !after.dotfiles.is_empty())
        && let Err(e) = koje_runtime::dotfiles::link(paths, after)
    {
        notes.push(format!("dotfiles not linked: {e:#}"));
    }
    notes
}

/// `ImageFile`: the context's Containerfile, made if it has none (ADR 0063).
pub fn image_file(paths: &Paths, ctx: &ContextName) -> Result<String> {
    let own = koje_runtime::images::own_file(paths, ctx).map_err(|e| Invalid(format!("{e:#}")))?;
    Ok(own.file.display().to_string())
}

/// `NewContext`: a fresh `context.toml` as `koje new` writes it.
pub fn new_context(paths: &Paths, name: &str) -> Result<String> {
    let name = ContextName::new(name).map_err(|e| Invalid(e.to_string()))?;
    let cfg = context::create(paths, &name, None, None).map_err(|e| Invalid(e.to_string()))?;
    Ok(cfg.name.to_string())
}

/// What a state operation names.
pub fn state_name(name: &str) -> Result<StateName> {
    Ok(StateName::new(name).map_err(|e| Invalid(e.to_string()))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tmp: &std::path::Path) -> Paths {
        Paths::with_roots(tmp.join("c"), tmp.join("d"), tmp.join("s"))
    }

    fn dict(entries: &[(&str, OwnedValue)]) -> HashMap<String, OwnedValue> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.try_clone().unwrap()))
            .collect()
    }

    #[test]
    fn favorites_set_kept_and_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        new_context(&p, "kp").unwrap();
        let kp = ContextName::new("kp").unwrap();
        assert_eq!(favorites(&p, &kp).unwrap(), None);
        let ids = ["firefox.desktop".to_owned(), "foot.desktop".to_owned()];
        set_favorites(&p, &kp, Some(&ids)).unwrap();
        assert_eq!(favorites(&p, &kp).unwrap().as_deref(), Some(&ids[..]));
        // Nothing pinned is not the same as none of its own.
        set_favorites(&p, &kp, Some(&[])).unwrap();
        assert_eq!(favorites(&p, &kp).unwrap(), Some(vec![]));
        set_favorites(&p, &kp, None).unwrap();
        assert_eq!(favorites(&p, &kp).unwrap(), None);
        let err = set_favorites(&p, &kp, Some(&["firefox".to_owned()])).unwrap_err();
        assert!(err.downcast_ref::<Invalid>().is_some(), "{err:#}");
        assert!(favorites(&p, &ContextName::new("nope").unwrap()).is_err());
    }

    #[test]
    fn contexts_with_look_and_status() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        new_context(&p, "kp").unwrap();
        new_context(&p, "shop").unwrap();
        let broken = p.context_config(&ContextName::new("zz").unwrap());
        std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
        std::fs::write(&broken, "name = 1\n").unwrap();
        let kp = ContextName::new("kp").unwrap();
        let live = Live {
            running: [kp.clone()].into(),
            paused: BTreeSet::new(),
            windows: [(kp, 3)].into(),
        };
        let rows = contexts(&p, &live).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("kp", "kp"));
        assert_eq!(rows[0].2, koje_core::look::CONTEXT_ICON);
        assert!(rows[0].3.starts_with('#'));
        assert_eq!((rows[0].4.as_str(), rows[0].5), ("running", 3));
        assert_eq!(rows[1].4, "stopped");
        assert_eq!(rows[2].4, "invalid");
    }

    #[test]
    fn config_and_set_config() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        new_context(&p, "kp").unwrap();
        let c = config(&p, &kp).unwrap();
        assert_eq!(
            String::try_from(c["prefix"].try_clone().unwrap()).unwrap(),
            "KP"
        );
        assert_eq!(
            String::try_from(c["profile"].try_clone().unwrap()).unwrap(),
            ""
        );
        assert_eq!(u64::try_from(&c["memory_soft"]).unwrap(), 0);
        let bases: HashMap<String, HashMap<String, bool>> =
            HashMap::try_from(c["bases"].try_clone().unwrap()).unwrap();
        assert!(!bases["desktop"]["gpu"]);
        assert!(bases[""]["gpu"]);
        let effective: HashMap<String, bool> =
            HashMap::try_from(c["effective"].try_clone().unwrap()).unwrap();
        assert_eq!(effective.get("gpu"), Some(&true));

        let access: HashMap<String, bool> = [("gpu".to_owned(), false)].into();
        let change = dict(&[
            ("label", value("Kundenportal")),
            ("memory_soft", value(4u64 << 30)),
            ("access", value(access)),
        ]);
        set_config(&p, &kp, &change).unwrap();
        let cfg = context::load(&p, &kp).unwrap();
        assert_eq!(cfg.label.as_deref(), Some("Kundenportal"));
        assert_eq!(cfg.limits.memory_soft, Some(ByteSize(4 << 30)));
        assert_eq!(cfg.access.gpu, Some(false));
        assert_eq!(cfg.prefix, "KP", "keys not given stay");

        // Refused: nothing changes.
        let text = std::fs::read_to_string(p.context_config(&kp)).unwrap();
        for bad in [
            dict(&[("color", value("green"))]),
            dict(&[("profile", value("all"))]),
            dict(&[("name", value("other"))]),
            dict(&[("memory_soft", value("4G"))]),
            dict(&[
                ("memory_soft", value(8u64 << 30)),
                ("memory_hard", value(6u64 << 30)),
            ]),
        ] {
            let err = set_config(&p, &kp, &bad).unwrap_err();
            assert!(err.downcast_ref::<Invalid>().is_some(), "{err:#}");
            assert_eq!(
                std::fs::read_to_string(p.context_config(&kp)).unwrap(),
                text
            );
        }
        // Empty removes.
        set_config(&p, &kp, &dict(&[("label", value(""))])).unwrap();
        assert_eq!(context::load(&p, &kp).unwrap().label, None);
    }

    #[test]
    fn states_with_roles_and_the_current_one() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        new_context(&p, "kp").unwrap();
        let default = state_name("default").unwrap();
        states::copy(&p, &kp, &state_name("empty").unwrap(), &default).unwrap();
        let rows = states(&p, &kp, Some(&default)).unwrap();
        let d = rows.iter().find(|r| r.0 == "default").unwrap();
        assert_eq!((d.1.as_str(), d.2, d.3), ("default", true, true));
        assert!(d.6 > 0, "modified");
        let fresh = rows.iter().find(|r| r.0 == "fresh").unwrap();
        assert_eq!(
            (fresh.1.as_str(), fresh.2, fresh.3),
            ("fresh", false, false)
        );
        assert!(rows.iter().any(|r| r.0 == "empty"));
    }
}
