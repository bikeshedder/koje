//! `koje bar` (M5.15, a first part of M8.2): a Waybar custom module showing the
//! running contexts in their colors, the active one highlighted. Runs for as
//! long as Waybar does: one JSON line at start and on every `ContextsChanged`
//! from kojed; while kojed is not there it shows nothing and keeps waiting.

use std::io::Write as _;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt as _;
use koje_core::api::{BUS_NAME, INTERFACE, OBJECT_PATH};
use serde::Serialize;
use zbus::{MatchRule, MessageStream};

/// A running context as kojed reports it: name, color, windows, active.
pub type Running = (String, String, u32, bool);

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Module {
    text: String,
    tooltip: String,
    class: Vec<&'static str>,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}

/// The module's JSON line (Pango markup in `text`).
pub fn module(running: &[Running]) -> String {
    let text = running
        .iter()
        .map(|(name, color, _, active)| {
            let dot = format!("<span color='{}'>●</span>", escape(color));
            if *active {
                format!("{dot} <b><u>{}</u></b>", escape(name))
            } else {
                format!("{dot} {}", escape(name))
            }
        })
        .collect::<Vec<_>>()
        .join("  ");
    let tooltip = if running.is_empty() {
        "no context running".to_owned()
    } else {
        running
            .iter()
            .map(|(name, _, windows, _)| format!("{name}: {windows} window(s)"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\nclick: switch context"
    };
    let class = if running.is_empty() {
        vec!["koje", "empty"]
    } else {
        vec!["koje"]
    };
    serde_json::to_string(&Module {
        text,
        tooltip,
        class,
    })
    .unwrap_or_default()
}

async fn running(conn: &zbus::Connection) -> zbus::Result<Vec<Running>> {
    conn.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "RunningContexts",
        &(),
    )
    .await?
    .body()
    .deserialize()
}

fn print(line: &str) {
    let mut out = std::io::stdout().lock();
    // Waybar reads line by line; a closed pipe means Waybar is gone.
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        std::process::exit(0);
    }
}

/// Follow kojed until the session bus goes away; `Err` to retry.
async fn follow(conn: &zbus::Connection) -> zbus::Result<()> {
    let changed_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)?
        .member("ContextsChanged")?
        .build();
    let restarted = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, BUS_NAME)?
        .build();
    let mut changes = MessageStream::for_match_rule(changed_rule, conn, None).await?;
    let mut owners = MessageStream::for_match_rule(restarted, conn, None).await?;
    loop {
        // An error (kojed not there yet) shows an empty module.
        print(&module(&running(conn).await.unwrap_or_default()));
        tokio::select! {
            m = changes.next() => if m.is_none() { return Ok(()) },
            m = owners.next() => if m.is_none() { return Ok(()) },
        }
    }
}

/// `koje bar`: the module, for Waybar's `exec`.
pub fn run() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        loop {
            match zbus::Connection::session().await {
                Ok(conn) => {
                    if let Err(e) = follow(&conn).await {
                        tracing::debug!("bar: {e}");
                    }
                }
                Err(e) => {
                    tracing::debug!("bar: no session bus: {e}");
                    print(&module(&[]));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

/// `koje bar --names`: the running contexts' names, one per line (pickers).
/// With `memory`, each followed by its use and limit (M8.17).
pub fn names(paths: &koje_core::paths::Paths, memory: bool) -> Result<()> {
    let running: Vec<Running> = crate::kojed::call("RunningContexts", &())?;
    for (name, ..) in running {
        if memory {
            println!("{name}  {}", crate::memory::summary(paths, &name)?);
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_line() {
        let line = module(&[
            ("a".into(), "#E06C75".into(), 0, true),
            ("b".into(), "#61AFEF".into(), 3, false),
        ]);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            v["text"],
            "<span color='#E06C75'>●</span> <b><u>a</u></b>  <span color='#61AFEF'>●</span> b"
        );
        assert_eq!(
            v["tooltip"],
            "a: 0 window(s)\nb: 3 window(s)\nclick: switch context"
        );
        let empty: serde_json::Value = serde_json::from_str(&module(&[])).unwrap();
        assert_eq!(empty["text"], "");
        assert_eq!(empty["class"], serde_json::json!(["koje", "empty"]));
    }
}
