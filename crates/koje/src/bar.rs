//! `koje bar` (M5.15, a first part of M8.2): a Waybar custom module showing the
//! running contexts in their colors, the active one highlighted. Runs for as
//! long as Waybar does: one JSON line at start and on every `ContextsChanged`
//! from kojed; while kojed is not there it shows nothing and keeps waiting.
//! `koje bar --memory` (M8.21) is a second module: the active context's
//! memory as a meter, again on every `MemoryChanged`.

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

/// kojed's `Memory`: `(context, used, charged, soft, hard)`, limits 0 = none.
pub type Memory = (String, u64, u64, u64, u64);

/// Meter cells (box-drawing characters, half-cell steps).
const METER_WIDTH: usize = 16;
/// Used, by level: normal, near the soft limit (90 %), over it, at the hard one.
const FILL: [&str; 4] = ["#2e7d5b", "#e5c07b", "#d19a66", "#e06c75"];
/// Free: a thin line; the soft limit mark when not reached.
const TRACK: &str = "#4a4a52";
const SOFT_MARK: &str = "#9a9aa2";

/// The memory module's JSON line: a meter and the percentage of the limit;
/// without limits just the size; nothing without an active context.
pub fn memory_module(m: &Memory) -> String {
    use koje_core::config::Limits;
    use koje_core::memory::{MemoryLevel, meter};
    use koje_core::size::ByteSize;
    use std::fmt::Write as _;
    let (ctx, used, charged, soft, hard) = m;
    if ctx.is_empty() {
        return serde_json::to_string(&Module {
            text: String::new(),
            tooltip: String::new(),
            class: vec!["memory", "empty"],
        })
        .unwrap_or_default();
    }
    let limit = |b: u64| (b > 0).then_some(ByteSize(b));
    let limits = Limits {
        memory_soft: limit(*soft),
        memory_hard: limit(*hard),
    };
    let used_b = ByteSize(*used);
    let share = |l: Option<ByteSize>| {
        l.map_or_else(
            || "none".to_owned(),
            |l| {
                format!(
                    "{} – {} %",
                    l.human(),
                    u128::from(*used) * 100 / u128::from(l.0.max(1))
                )
            },
        )
    };
    let tooltip = format!(
        "{}: memory\nused {} (charged {})\nsoft limit {}\nhard limit {}",
        escape(ctx),
        used_b.human(),
        ByteSize(*charged).human(),
        share(limits.memory_soft),
        share(limits.memory_hard)
    );
    let Some(m) = meter(used_b, &limits, METER_WIDTH) else {
        return serde_json::to_string(&Module {
            text: used_b.human(),
            tooltip,
            class: vec!["memory", "unlimited"],
        })
        .unwrap_or_default();
    };
    let (fill, level) = match m.level {
        MemoryLevel::Normal => (FILL[0], "normal"),
        MemoryLevel::NearSoft => (FILL[1], "near-soft"),
        MemoryLevel::OverSoft => (FILL[2], "over-soft"),
        MemoryLevel::OverHard => (FILL[3], "over-hard"),
    };
    // A thick line for the used part on a thin one, the soft limit as a
    // crossing: `━ ╾ ─`, at the soft limit `┿ ┽ ┼`.
    let mut cells = String::new();
    for c in &m.cells {
        let (glyph, color) = match (c.halves, c.soft) {
            (2, false) => ('━', fill),
            (1, false) => ('╾', fill),
            (_, false) => ('─', TRACK),
            (2, true) => ('┿', fill),
            (1, true) => ('┽', fill),
            (_, true) => ('┼', SOFT_MARK),
        };
        let _ = write!(cells, "<span color='{color}'>{glyph}</span>");
    }
    serde_json::to_string(&Module {
        text: format!(
            "<span font_family='monospace' size='small'>{cells}</span> {}%",
            m.percent
        ),
        tooltip,
        class: vec!["memory", level],
    })
    .unwrap_or_default()
}

async fn memory(conn: &zbus::Connection) -> zbus::Result<Memory> {
    conn.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "Memory",
        &("",),
    )
    .await?
    .body()
    .deserialize()
}

/// Which module `koje bar` runs.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Contexts,
    Memory,
}

impl Kind {
    async fn line(self, conn: &zbus::Connection) -> String {
        // An error (kojed not there yet) shows an empty module.
        match self {
            Self::Contexts => module(&running(conn).await.unwrap_or_default()),
            Self::Memory => memory_module(&memory(conn).await.unwrap_or_default()),
        }
    }

    fn empty(self) -> String {
        match self {
            Self::Contexts => module(&[]),
            Self::Memory => memory_module(&Memory::default()),
        }
    }
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

/// Follow kojed until the session bus goes away; `Err` to retry. Both
/// modules redraw on `ContextsChanged` (the active context may have changed);
/// the memory module also on `MemoryChanged`.
async fn follow(conn: &zbus::Connection, kind: Kind) -> zbus::Result<()> {
    let changed_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)?
        .member("ContextsChanged")?
        .build();
    let memory_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)?
        .member("MemoryChanged")?
        .build();
    let restarted = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, BUS_NAME)?
        .build();
    let mut changes = MessageStream::for_match_rule(changed_rule, conn, None).await?;
    let mut owners = MessageStream::for_match_rule(restarted, conn, None).await?;
    let mut readings = MessageStream::for_match_rule(memory_rule, conn, None).await?;
    let memory = matches!(kind, Kind::Memory);
    loop {
        print(&kind.line(conn).await);
        tokio::select! {
            m = changes.next() => if m.is_none() { return Ok(()) },
            m = owners.next() => if m.is_none() { return Ok(()) },
            m = readings.next(), if memory => if m.is_none() { return Ok(()) },
        }
    }
}

/// `koje bar [--memory]`: the module, for Waybar's `exec`.
pub fn run(kind: Kind) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        loop {
            match zbus::Connection::session().await {
                Ok(conn) => {
                    if let Err(e) = follow(&conn, kind).await {
                        tracing::debug!("bar: {e}");
                    }
                }
                Err(e) => {
                    tracing::debug!("bar: no session bus: {e}");
                    print(&kind.empty());
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

    #[test]
    fn memory_meter_line() {
        let gib = |n: u64| n << 30;
        let v =
            |m: &Memory| -> serde_json::Value { serde_json::from_str(&memory_module(m)).unwrap() };
        // No active context: an empty module (Waybar hides it).
        assert_eq!(v(&Memory::default())["text"], "");

        let m = v(&("kp".into(), gib(3), gib(4), gib(6), gib(8)));
        let text = m["text"].as_str().unwrap();
        assert!(text.ends_with("</span> 38%"), "{text}");
        assert_eq!(
            text.matches("<span color='#2e7d5b'>━</span>").count(),
            6,
            "{text}"
        );
        assert_eq!(
            text.matches("<span color='#4a4a52'>─</span>").count(),
            9,
            "{text}"
        );
        assert_eq!(
            text.matches("<span color='#9a9aa2'>┼</span>").count(),
            1,
            "the soft limit"
        );
        assert_eq!(
            m["tooltip"],
            "kp: memory\nused 3.0 GiB (charged 4.0 GiB)\nsoft limit 6.0 GiB – 50 %\nhard limit 8.0 GiB – 37 %"
        );
        assert_eq!(m["class"], serde_json::json!(["memory", "normal"]));

        // Near the soft limit: yellow; over it: orange, the mark filled.
        let m = v(&("kp".into(), gib(11) / 2, gib(6), gib(6), gib(8)));
        assert!(m["text"].as_str().unwrap().contains("#e5c07b"));
        assert_eq!(m["class"][1], "near-soft");
        let m = v(&("kp".into(), gib(7), gib(7), gib(6), gib(8)));
        let text = m["text"].as_str().unwrap();
        assert!(text.contains("<span color='#d19a66'>┿</span>"), "{text}");
        assert_eq!(m["class"][1], "over-soft");

        // Without limits only the size.
        let m = v(&("kp".into(), gib(1), gib(2), 0, 0));
        assert_eq!(m["text"], "1.0 GiB");
        assert!(m["tooltip"].as_str().unwrap().ends_with("hard limit none"));
    }
}
