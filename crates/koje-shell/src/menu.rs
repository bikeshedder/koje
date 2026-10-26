//! The koje menu (M8.55), without GTK: what its button shows (the koje icon
//! in the active context's accent and its memory meter) and what its dropdown
//! holds – the active context in detail, or the running contexts on a host
//! workspace.

use koje_core::config::Limits;
use koje_core::memory::{MemoryLevel, classify};
use koje_core::size::ByteSize;

use crate::model::{Accent, MemoryReading, RunningContext};

/// The working set of a context over the last minutes (kojed's
/// `MemoryHistory`), oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryReading {
    pub context: String,
    /// Seconds between readings.
    pub interval: u32,
    pub used: Vec<u64>,
}

impl From<(String, u32, Vec<u64>)> for HistoryReading {
    fn from((context, interval, used): (String, u32, Vec<u64>)) -> Self {
        Self {
            context,
            interval,
            used,
        }
    }
}

/// The memory meter: usage against the hard limit, else the soft one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryView {
    /// The used share of the meter in thousandths; `None` without limits
    /// (no meter, only the size).
    pub fill: Option<u16>,
    /// Where the soft limit lies on the meter, in thousandths – only with a
    /// hard limit too.
    pub soft: Option<u16>,
    pub level: MemoryLevel,
    /// `42%`, or the size without limits.
    pub label: String,
    /// Used, charged, the limits ([`koje_core::memory::details`]).
    pub tooltip: String,
}

fn limits(m: &MemoryReading) -> Limits {
    let limit = |b: u64| (b > 0).then_some(ByteSize(b));
    Limits {
        memory_soft: limit(m.soft),
        memory_hard: limit(m.hard),
    }
}

impl MemoryView {
    pub fn from_reading(m: &MemoryReading) -> Self {
        let limits = limits(m);
        let used = u128::from(m.used);
        let max = limits
            .memory_hard
            .or(limits.memory_soft)
            .map(|l| u128::from(l.0.max(1)));
        let thousandths =
            |v: u128, max: u128| u16::try_from((v * 1000 / max).min(1000)).unwrap_or(1000);
        Self {
            fill: max.map(|max| thousandths(used, max)),
            soft: limits
                .memory_hard
                .and(limits.memory_soft)
                .zip(max)
                .map(|(s, max)| thousandths(u128::from(s.0), max)),
            level: classify(ByteSize(m.used), &limits),
            label: max.map_or_else(
                || ByteSize(m.used).human(),
                |max| format!("{}%", (used * 100 + max / 2) / max),
            ),
            tooltip: koje_core::memory::details(
                &m.context,
                ByteSize(m.used),
                ByteSize(m.charged),
                &limits,
            ),
        }
    }
}

/// The graph of the last minutes, with the limits as lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    /// Bytes, oldest first.
    pub used: Vec<u64>,
    /// Seconds between readings.
    pub interval: u32,
    pub soft: Option<u64>,
    pub hard: Option<u64>,
    /// The value at the top of the graph.
    pub top: u64,
    /// `last 10 minutes`.
    pub span: String,
}

/// The top of the graph: room above the highest reading and both limits.
pub fn graph_top(used: &[u64], soft: Option<u64>, hard: Option<u64>) -> u64 {
    let highest = used
        .iter()
        .copied()
        .chain(soft)
        .chain(hard)
        .max()
        .unwrap_or(0);
    (highest + highest / 10).max(1)
}

fn span(seconds: u64) -> String {
    match seconds {
        0..60 => format!("last {seconds} seconds"),
        60..120 => "last minute".to_owned(),
        _ => format!("last {} minutes", seconds / 60),
    }
}

/// The active context in detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPanel {
    pub name: String,
    pub accent: Accent,
    /// `None` before kojed's first reading.
    pub memory: Option<MemoryView>,
    /// `3.1 GiB of 8.0 GiB`.
    pub summary: String,
    /// `None` before kojed has readings.
    pub graph: Option<Graph>,
    /// The limits as `koje limit` takes them (`6G`); empty for none.
    pub soft: String,
    pub hard: String,
    /// What went wrong changing the limits.
    pub error: Option<String>,
}

/// One running context on a host workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewRow {
    pub name: String,
    pub accent: Accent,
    pub memory: Option<MemoryView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Panel {
    Context(Box<ContextPanel>),
    /// The running contexts; empty when none runs.
    Overview(Vec<OverviewRow>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuView {
    /// The icon's color: the active context's, grey on host workspaces.
    pub accent: Accent,
    /// The active context's meter in the button.
    pub meter: Option<MemoryView>,
    pub panel: Panel,
}

/// The limit as the entry shows it: `6G`, empty for none.
fn limit_text(bytes: u64) -> String {
    if bytes == 0 {
        String::new()
    } else {
        ByteSize(bytes).to_string()
    }
}

/// What the user typed as `koje limit` takes it: a size (`6G`), or `none`
/// for an empty field. An error says why the text is no size.
pub fn limit_arg(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return Ok("none".to_owned());
    }
    t.parse::<ByteSize>()
        .map(|b| b.to_string())
        .map_err(|_| format!("“{t}” is no size – e.g. 6G or 512M; empty for none"))
}

/// The koje menu for the running `contexts`, their `memory`, the active
/// one's `history` and the last error changing limits (`(context, error)`).
pub fn menu(
    contexts: &[RunningContext],
    memory: &[MemoryReading],
    history: &HistoryReading,
    error: Option<&(String, String)>,
) -> MenuView {
    let reading = |name: &str| memory.iter().find(|m| m.context == name);
    let Some(active) = contexts.iter().find(|c| c.active) else {
        return MenuView {
            accent: Accent::Host,
            meter: None,
            panel: Panel::Overview(
                contexts
                    .iter()
                    .map(|c| OverviewRow {
                        name: c.name.clone(),
                        accent: Accent::Context(c.name.clone()),
                        memory: reading(&c.name).map(MemoryView::from_reading),
                    })
                    .collect(),
            ),
        };
    };
    let m = reading(&active.name);
    let meter = m.map(MemoryView::from_reading);
    let summary = m.map_or_else(String::new, |m| {
        let l = limits(m);
        match l.memory_hard.or(l.memory_soft) {
            Some(max) => format!("{} of {}", ByteSize(m.used).human(), max.human()),
            None => format!("{}, no limit", ByteSize(m.used).human()),
        }
    });
    let graph = (history.context == active.name && !history.used.is_empty()).then(|| {
        let soft = m.map(|m| m.soft).filter(|&b| b > 0);
        let hard = m.map(|m| m.hard).filter(|&b| b > 0);
        Graph {
            top: graph_top(&history.used, soft, hard),
            span: span(u64::from(history.interval) * history.used.len() as u64),
            used: history.used.clone(),
            interval: history.interval,
            soft,
            hard,
        }
    });
    MenuView {
        accent: Accent::Context(active.name.clone()),
        meter: meter.clone(),
        panel: Panel::Context(Box::new(ContextPanel {
            name: active.name.clone(),
            accent: Accent::Context(active.name.clone()),
            memory: meter,
            summary,
            graph,
            soft: m.map(|m| limit_text(m.soft)).unwrap_or_default(),
            hard: m.map(|m| limit_text(m.hard)).unwrap_or_default(),
            error: error
                .filter(|(ctx, _)| *ctx == active.name)
                .map(|(_, e)| e.clone()),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn ctx(name: &str, active: bool) -> RunningContext {
        RunningContext {
            name: name.into(),
            color: "#3584e4".into(),
            active,
        }
    }

    fn reading(context: &str, used: u64, soft: u64, hard: u64) -> MemoryReading {
        MemoryReading {
            context: context.into(),
            used,
            charged: used + (100 << 20),
            soft,
            hard,
        }
    }

    fn history(context: &str, used: &[u64]) -> HistoryReading {
        HistoryReading {
            context: context.into(),
            interval: 5,
            used: used.to_vec(),
        }
    }

    #[test]
    fn a_context_in_detail() {
        let menu = menu(
            &[ctx("privat", false), ctx("kp", true)],
            &[reading("kp", 3 * GIB, 6 * GIB, 8 * GIB)],
            &history("kp", &[2 * GIB, 3 * GIB]),
            None,
        );
        assert_eq!(menu.accent, Accent::Context("kp".into()));
        let meter = menu.meter.unwrap();
        assert_eq!(
            (meter.fill, meter.soft, meter.label.as_str()),
            (Some(375), Some(750), "38%")
        );
        let Panel::Context(p) = menu.panel else {
            panic!("context panel expected");
        };
        assert_eq!(p.name, "kp");
        assert_eq!(p.summary, "3.0 GiB of 8.0 GiB");
        assert_eq!((p.soft.as_str(), p.hard.as_str()), ("6G", "8G"));
        let g = p.graph.unwrap();
        assert_eq!(
            (g.soft, g.hard, g.top),
            (Some(6 * GIB), Some(8 * GIB), 8 * GIB + 8 * GIB / 10)
        );
        assert_eq!(g.span, "last 10 seconds");
        assert_eq!(p.error, None);
    }

    #[test]
    fn nothing_read_yet_and_a_history_of_another_context() {
        let menu = menu(&[ctx("kp", true)], &[], &history("privat", &[GIB]), None);
        assert_eq!(menu.meter, None);
        let Panel::Context(p) = menu.panel else {
            panic!("context panel expected");
        };
        assert_eq!((p.memory, p.graph, p.summary.as_str()), (None, None, ""));
        assert_eq!((p.soft.as_str(), p.hard.as_str()), ("", ""));
    }

    #[test]
    fn without_limits_the_size() {
        let menu = menu(
            &[ctx("kp", true)],
            &[reading("kp", 3 * GIB, 0, 0)],
            &HistoryReading::default(),
            None,
        );
        let Panel::Context(p) = menu.panel else {
            panic!("context panel expected");
        };
        assert_eq!(p.summary, "3.0 GiB, no limit");
        assert_eq!(p.memory.unwrap().fill, None);
    }

    #[test]
    fn errors_belong_to_their_context() {
        let err = ("kp".to_owned(), "hard limit below soft".to_owned());
        let shown = |active: &str| {
            let m = menu(
                &[ctx(active, true)],
                &[],
                &HistoryReading::default(),
                Some(&err),
            );
            match m.panel {
                Panel::Context(p) => p.error,
                Panel::Overview(_) => None,
            }
        };
        assert_eq!(shown("kp"), Some("hard limit below soft".into()));
        assert_eq!(shown("privat"), None);
    }

    #[test]
    fn host_workspaces_list_the_running_contexts() {
        let menu = menu(
            &[ctx("kp", false), ctx("privat", false)],
            &[reading("privat", GIB, 0, 2 * GIB)],
            &HistoryReading::default(),
            None,
        );
        assert_eq!(
            (menu.accent.clone(), menu.meter.clone()),
            (Accent::Host, None)
        );
        let Panel::Overview(rows) = menu.panel else {
            panic!("overview expected");
        };
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["kp", "privat"]);
        assert_eq!(rows[0].memory, None);
        assert_eq!(rows[1].memory.as_ref().unwrap().label, "50%");
    }

    #[test]
    fn memory_levels_limits_and_none() {
        let view = |used, soft, hard| MemoryView::from_reading(&reading("kp", used, soft, hard));
        // Only a soft limit: the meter goes up to it, no mark.
        let soft_only = view(5 * GIB + GIB / 2, 6 * GIB, 0);
        assert_eq!((soft_only.fill, soft_only.soft), (Some(916), None));
        assert_eq!(soft_only.level, MemoryLevel::NearSoft);
        assert_eq!(view(7 * GIB, 6 * GIB, 8 * GIB).level, MemoryLevel::OverSoft);
        let full = view(9 * GIB, 6 * GIB, 8 * GIB);
        assert_eq!((full.fill, full.level), (Some(1000), MemoryLevel::OverHard));
        assert_eq!(full.label, "113%");
        let free = view(3 * GIB, 0, 0);
        assert_eq!((free.fill, free.label.as_str()), (None, "3.0 GiB"));
    }

    #[test]
    fn graph_room_above_readings_and_limits() {
        assert_eq!(
            graph_top(&[GIB, 9 * GIB], Some(6 * GIB), Some(8 * GIB)),
            9 * GIB + 9 * GIB / 10
        );
        assert_eq!(
            graph_top(&[GIB], None, Some(2 * GIB)),
            2 * GIB + 2 * GIB / 10
        );
        assert_eq!(graph_top(&[], None, None), 1);
        assert_eq!(span(600), "last 10 minutes");
        assert_eq!(span(65), "last minute");
    }

    #[test]
    fn limits_as_koje_limit_takes_them() {
        assert_eq!(limit_arg(" 6G "), Ok("6G".into()));
        assert_eq!(limit_arg("512m"), Ok("512M".into()));
        assert_eq!(limit_arg(""), Ok("none".into()));
        assert_eq!(limit_arg("None"), Ok("none".into()));
        assert!(limit_arg("6 GB").unwrap_err().contains("is no size"));
    }
}
