//! Every request string koje sends to Hyprland, in one place (M2.2: the
//! syntax changed in 0.53 and will change again). Pure; tested here and
//! against a live Hyprland in M4.8.

use koje_core::ports::CompositorCommand;

use crate::clients::Monitor;
use crate::{Error, Result};

/// Arguments end up inside `cmd;cmd` batches and `arg,address:…` lists.
fn arg(s: &str) -> Result<&str> {
    if s.is_empty() || s.contains([';', ',', '\n', '\r']) {
        return Err(Error::InvalidArgument(s.to_owned()));
    }
    Ok(s)
}

/// Hyprland's selector for a workspace name as [`koje_core::ports::WindowInfo`]
/// reports it: `special:x` and numbers as they are, other names as `name:x`.
pub fn workspace_selector(name: &str) -> Result<String> {
    let name = arg(name)?;
    Ok(
        if name.starts_with("special:") || name.chars().all(|c| c.is_ascii_digit()) {
            name.to_owned()
        } else {
            format!("name:{name}")
        },
    )
}

/// The dispatcher lines for one command.
pub fn command(c: &CompositorCommand, monitors: &[Monitor]) -> Result<Vec<String>> {
    use CompositorCommand as C;
    Ok(match c {
        // Adding the rule again overrides the earlier one: later rules win.
        C::ContextRule {
            tag,
            active,
            inactive,
        } => vec![format!(
            "keyword windowrule match:tag {}, border_color {} {}",
            arg(tag)?,
            active.to_hyprland(),
            inactive.to_hyprland()
        )],
        // Proven in M2.2: both properties, or the other state keeps the default.
        C::BorderColors {
            address,
            active,
            inactive,
        } => {
            let address = arg(address)?;
            vec![
                format!(
                    "dispatch setprop address:{address} active_border_color {}",
                    active.to_hyprland()
                ),
                format!(
                    "dispatch setprop address:{address} inactive_border_color {}",
                    inactive.to_hyprland()
                ),
            ]
        }
        C::TagWindow { address, tag } => vec![format!(
            "dispatch tagwindow +{} address:{}",
            arg(tag)?,
            arg(address)?
        )],
        C::Place { address, placement } => {
            let address = arg(address)?;
            let m = monitors
                .iter()
                .find(|m| m.name == placement.monitor)
                .ok_or_else(|| Error::UnknownMonitor(placement.monitor.clone()))?;
            let r = placement.rect;
            vec![
                format!(
                    "dispatch movewindowpixel exact {} {},address:{address}",
                    i64::from(m.x) + i64::from(r.x),
                    i64::from(m.y) + i64::from(r.y)
                ),
                format!(
                    "dispatch resizewindowpixel exact {} {},address:{address}",
                    r.w, r.h
                ),
            ]
        }
        C::MoveToWorkspace { address, workspace } => vec![format!(
            "dispatch movetoworkspacesilent {},address:{}",
            workspace_selector(workspace)?,
            arg(address)?
        )],
        C::FocusWindow { address } => {
            vec![format!("dispatch focuswindow address:{}", arg(address)?)]
        }
        C::ToggleSpecial { name } => {
            vec![format!("dispatch togglespecialworkspace {}", arg(name)?)]
        }
        C::CloseWindow { address } => {
            vec![format!("dispatch closewindow address:{}", arg(address)?)]
        }
        C::FocusWorkspace { workspace } => {
            vec![format!(
                "dispatch workspace {}",
                workspace_selector(workspace)?
            )]
        }
    })
}

pub fn commands(cs: &[CompositorCommand], monitors: &[Monitor]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for c in cs {
        out.extend(command(c, monitors)?);
    }
    Ok(out)
}

/// One request for all lines (`[[BATCH]]a;b`); a single line is sent as is.
pub fn batch(lines: &[String]) -> String {
    match lines {
        [one] => one.clone(),
        many => format!("[[BATCH]]{}", many.join(";")),
    }
}

/// Hyprland answers `ok` per command; anything else is an error message.
pub fn check_reply(request: &str, reply: &str) -> Result<()> {
    let errors: Vec<&str> = reply
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "ok")
        .collect();
    if errors.is_empty() && !reply.trim().is_empty() {
        return Ok(());
    }
    Err(Error::Rejected {
        request: request.to_owned(),
        reply: if errors.is_empty() {
            "(empty reply)".to_owned()
        } else {
            errors.join("; ")
        },
    })
}

#[cfg(test)]
mod tests {
    use koje_core::color::Color;
    use koje_core::layout::{Placement, Rect};

    use super::*;

    fn monitors() -> Vec<Monitor> {
        vec![
            Monitor {
                id: 0,
                name: "DP-1".into(),
                x: 0,
                y: 0,
            },
            Monitor {
                id: 1,
                name: "HDMI-A-1".into(),
                x: 2560,
                y: 0,
            },
        ]
    }

    #[allow(clippy::needless_pass_by_value)]
    fn one(c: CompositorCommand) -> Vec<String> {
        command(&c, &monitors()).unwrap()
    }

    #[test]
    fn syntax_053() {
        assert_eq!(
            one(CompositorCommand::ContextRule {
                tag: "koje-work".into(),
                active: Color::rgb(0x2e, 0x7d, 0x5b),
                inactive: Color::rgb(0x17, 0x3e, 0x2d),
            }),
            ["keyword windowrule match:tag koje-work, border_color rgb(2e7d5b) rgb(173e2d)"]
        );
        assert_eq!(
            one(CompositorCommand::TagWindow {
                address: "0x63b2".into(),
                tag: "koje-work".into()
            }),
            ["dispatch tagwindow +koje-work address:0x63b2"]
        );
        assert_eq!(
            one(CompositorCommand::BorderColors {
                address: "0x63b2".into(),
                active: Color::rgb(0xff, 0, 0),
                inactive: Color::rgb(0x80, 0, 0),
            }),
            [
                "dispatch setprop address:0x63b2 active_border_color rgb(ff0000)",
                "dispatch setprop address:0x63b2 inactive_border_color rgb(800000)"
            ]
        );
        assert_eq!(
            one(CompositorCommand::FocusWindow {
                address: "0x63b2".into()
            }),
            ["dispatch focuswindow address:0x63b2"]
        );
        assert_eq!(
            one(CompositorCommand::ToggleSpecial {
                name: "stash-kp".into()
            }),
            ["dispatch togglespecialworkspace stash-kp"]
        );
        assert_eq!(
            one(CompositorCommand::CloseWindow {
                address: "0x63b2".into()
            }),
            ["dispatch closewindow address:0x63b2"]
        );
    }

    #[test]
    fn placement_is_relative_to_the_monitor() {
        let lines = one(CompositorCommand::Place {
            address: "0xa".into(),
            placement: Placement {
                monitor: "HDMI-A-1".into(),
                rect: Rect {
                    x: 40,
                    y: -10,
                    w: 800,
                    h: 600,
                },
            },
        });
        assert_eq!(
            lines,
            [
                "dispatch movewindowpixel exact 2600 -10,address:0xa",
                "dispatch resizewindowpixel exact 800 600,address:0xa"
            ]
        );
        let unknown = command(
            &CompositorCommand::Place {
                address: "0xa".into(),
                placement: Placement {
                    monitor: "eDP-1".into(),
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 1,
                        h: 1,
                    },
                },
            },
            &monitors(),
        );
        assert!(matches!(unknown, Err(Error::UnknownMonitor(m)) if m == "eDP-1"));
    }

    #[test]
    fn workspaces() {
        assert_eq!(workspace_selector("work").unwrap(), "name:work");
        assert_eq!(workspace_selector("3").unwrap(), "3");
        assert_eq!(
            workspace_selector("special:koje-stash").unwrap(),
            "special:koje-stash"
        );
        assert_eq!(
            one(CompositorCommand::MoveToWorkspace {
                address: "0xa".into(),
                workspace: "work".into()
            }),
            ["dispatch movetoworkspacesilent name:work,address:0xa"]
        );
        assert_eq!(
            one(CompositorCommand::FocusWorkspace {
                workspace: "special:koje-stash".into()
            }),
            ["dispatch workspace special:koje-stash"]
        );
    }

    #[test]
    fn arguments_cannot_break_out_of_a_batch() {
        for bad in ["a;dispatch exit", "a,b", "a\nb", ""] {
            let c = CompositorCommand::FocusWorkspace {
                workspace: bad.into(),
            };
            assert!(
                matches!(command(&c, &[]), Err(Error::InvalidArgument(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn batches_and_replies() {
        assert_eq!(batch(&["a".into()]), "a");
        assert_eq!(batch(&["a".into(), "b".into()]), "[[BATCH]]a;b");
        assert!(check_reply("r", "ok").is_ok());
        assert!(check_reply("r", "ok\n\nok\n\n").is_ok());
        let err = check_reply("r", "ok\n\nprop not found\n\n").unwrap_err();
        assert!(err.to_string().contains("prop not found"), "{err}");
        assert!(check_reply("r", "").is_err());
    }
}
