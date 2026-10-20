//! Starting a state's programs again (M5.3, ADR 0031): from each window's spec
//! and its integration's `launch` template to a command line for the context.

use std::collections::BTreeSet;

use crate::capture::{program, shell_join};
use crate::integration::Integration;
use crate::name::WindowId;
use crate::state::{Where, WindowSpec};

/// One program to start in the context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The first window this start is for.
    pub id: WindowId,
    /// For `sh -c`.
    pub command: String,
    /// The working directory to start in (a remembered `cwd`).
    pub workdir: Option<String>,
}

/// What [`plan`] decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub launches: Vec<Launch>,
    /// Windows that come with another window's start (`launch_once`).
    pub with_others: Vec<WindowId>,
    /// Windows koje cannot start (no command, or not supported yet).
    pub skipped: Vec<(WindowId, &'static str)>,
}

fn quote(value: &str) -> String {
    shell_join(&[value.to_owned()])
}

/// Fill `template`: `{program}`, `{command}` and remembered values (quoted).
/// A placeholder without a value becomes empty.
fn fill(template: &str, spec: &WindowSpec) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = &after[..end];
        let value = match key {
            "program" => spec.command.as_deref().map(|c| quote(&program(c))),
            // Already a shell command line.
            "command" => spec.command.clone(),
            other => spec
                .remembered
                .get(other)
                .and_then(|v| v.as_str())
                .map(quote),
        };
        out.push_str(&value.unwrap_or_default());
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `~` and `~/…` relative to `home` (the context's home is the user's).
fn expand(path: &str, home: &str) -> String {
    if path == "~" {
        home.to_owned()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        path.to_owned()
    }
}

/// The starts for `windows`, in their order.
pub fn plan(windows: &[WindowSpec], integrations: &[Integration], home: &str) -> Plan {
    let mut plan = Plan::default();
    let mut started_once: BTreeSet<&str> = BTreeSet::new();
    for spec in windows {
        if spec.where_ == Where::Dev {
            plan.skipped
                .push((spec.id.clone(), "windows in dev containers come later"));
            continue;
        }
        let integration = spec
            .integration
            .as_deref()
            .and_then(|name| integrations.iter().find(|i| i.name == name));
        if let Some(i) = integration.filter(|i| i.launch_once)
            && !started_once.insert(i.name.as_str())
        {
            plan.with_others.push(spec.id.clone());
            continue;
        }
        if spec.command.is_none() {
            plan.skipped
                .push((spec.id.clone(), "no command line remembered"));
            continue;
        }
        let command = match integration {
            Some(i) => fill(&i.launch, spec),
            None => spec.command.clone().unwrap_or_default(),
        };
        let workdir = spec
            .remembered
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|c| expand(c, home));
        plan.launches.push(Launch {
            id: spec.id.clone(),
            command,
            workdir,
        });
    }
    plan
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::integration::built_in;

    fn spec(
        id: &str,
        integration: Option<&str>,
        command: &str,
        remembered: &[(&str, &str)],
    ) -> WindowSpec {
        WindowSpec {
            id: WindowId::new(id).unwrap(),
            integration: integration.map(str::to_owned),
            command: (!command.is_empty()).then(|| command.to_owned()),
            app_id: None,
            where_: Where::Context,
            remembered: remembered
                .iter()
                .map(|(k, v)| ((*k).to_owned(), toml::Value::String((*v).to_owned())))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn launches_from_templates() {
        let all = built_in();
        let windows = [
            spec(
                "terminal",
                Some("terminal"),
                "foot -D /home/mj",
                &[("cwd", "/tmp")],
            ),
            spec(
                "terminal-2",
                Some("terminal"),
                "foot",
                &[("cwd", "~/src/kp")],
            ),
            spec(
                "editor",
                Some("editor"),
                "/usr/share/code/code /old",
                &[("path", "/home/mj/src/my project")],
            ),
            spec(
                "browser",
                Some("browser"),
                "/usr/lib64/firefox/firefox",
                &[],
            ),
            spec(
                "browser-2",
                Some("browser"),
                "/usr/lib64/firefox/firefox",
                &[],
            ),
            spec("viewer", None, "evince 'spec v2.pdf'", &[]),
            spec("lost", None, "", &[]),
        ];
        let p = plan(&windows, &all, "/home/mj");
        let got: Vec<(&str, &str, Option<&str>)> = p
            .launches
            .iter()
            .map(|l| (l.id.as_str(), l.command.as_str(), l.workdir.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("terminal", "foot", Some("/tmp")),
                ("terminal-2", "foot", Some("/home/mj/src/kp")),
                (
                    "editor",
                    "/usr/share/code/code --new-window '/home/mj/src/my project'",
                    None
                ),
                ("browser", "/usr/lib64/firefox/firefox", None),
                ("viewer", "evince 'spec v2.pdf'", None),
            ]
        );
        assert_eq!(
            p.with_others,
            [WindowId::new("browser-2").unwrap()],
            "one browser start restores both"
        );
        assert_eq!(p.skipped.len(), 1);
    }

    #[test]
    fn editor_without_a_folder() {
        let p = plan(
            &[spec("editor", Some("editor"), "code", &[])],
            &built_in(),
            "/h",
        );
        assert_eq!(p.launches[0].command, "code --new-window");
    }
}
