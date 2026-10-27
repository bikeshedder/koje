//! The overview, as GNOME's (ADR 0049): what it shows, without GTK – the
//! current workspace's windows, the workspace strip, the context's stash –
//! where the windows go ([`arrange`]) and what clicks and drops on the
//! strip do.

use crate::dash::DashEntry;
use crate::model::{Accent, Action, OverlayButton, WindowState, WorkspaceButton};

/// What one monitor's overview shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewView {
    /// The monitor's workspaces, as in the bar.
    pub workspaces: Vec<WorkspaceButton>,
    /// The overlays, as in the bar: after the workspaces.
    pub overlays: Vec<OverlayButton>,
    /// The overlay open on the monitor: the overview shows its windows.
    pub overlay: Option<String>,
    /// The current workspace's windows (a context's active layout) or the
    /// open overlay's, in the order [`arrange`] places them.
    pub windows: Vec<WindowCard>,
    /// The active context's stash; `None` on a host workspace or while an
    /// overlay is open.
    pub stash: Option<Stash>,
    /// The dash; empty on monitors without the focus.
    pub dash: Vec<DashEntry>,
    /// koje's plugin draws the windows into the cards (ADR 0051); else the
    /// cards show the app's icon.
    pub live: bool,
}

/// A window in the overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCard {
    pub address: String,
    pub title: String,
    /// The app id: its icon and name come from its desktop file.
    pub app: String,
    pub accent: Accent,
    /// The window's context; `None`: a host window.
    pub context: Option<String>,
    /// Its size on screen: thumbnails keep its proportions.
    pub size: (i32, i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub context: String,
    /// Most recently used first.
    pub windows: Vec<WindowCard>,
}

impl WindowCard {
    pub fn new(w: &WindowState, accent: Accent) -> Self {
        Self {
            address: w.address.clone(),
            title: w.title.clone(),
            app: w.class.clone(),
            accent,
            context: w.context.clone(),
            size: w.size,
        }
    }
}

/// A place in the overview's strip.
#[derive(Debug, Clone, Copy)]
pub enum StripTarget<'a> {
    Workspace(&'a WorkspaceButton),
    Overlay(&'a OverlayButton),
}

/// What a click on the strip does: a workspace is shown – the open overlay
/// closed first, so it is seen; an overlay opens or closes, as in the bar.
pub fn strip_click(view: &OverviewView, target: StripTarget<'_>) -> Vec<Action> {
    match target {
        StripTarget::Workspace(w) => view
            .overlay
            .iter()
            .map(|o| Action::ToggleOverlay(o.clone()))
            .chain([w.focus()])
            .collect(),
        StripTarget::Overlay(o) => vec![Action::ToggleOverlay(o.name.clone())],
    }
}

/// What dropping a window's card on the strip does (M8.71): it moves
/// there, the focus stays. A context's window only to its own context's
/// workspace, a host window only to host workspaces – as with Super+Shift
/// and a number; any window to an overlay, as with an overlay's key and
/// Shift. Empty: refused, or where it is already.
pub fn drop_window(view: &OverviewView, card: &WindowCard, target: StripTarget<'_>) -> Vec<Action> {
    let workspace = match target {
        StripTarget::Workspace(w) => {
            let fits = match &w.accent {
                Accent::Context(c) => card.context.as_ref() == Some(c),
                Accent::Host => card.context.is_none(),
            };
            if !fits || (w.active && view.overlay.is_none()) {
                return Vec::new();
            }
            w.name.clone()
        }
        StripTarget::Overlay(o) => {
            if o.open {
                return Vec::new();
            }
            format!("special:{}", o.name)
        }
    };
    vec![Action::MoveWindow {
        address: card.address.clone(),
        workspace,
    }]
}

/// The windows of `workspace`, top to bottom, left to right – the order
/// GNOME's arrangement keeps them in.
pub fn on_workspace<'a>(windows: &'a [WindowState], workspace: &str) -> Vec<&'a WindowState> {
    let mut on: Vec<&WindowState> = windows
        .iter()
        .filter(|w| w.workspace == workspace)
        .collect();
    on.sort_by_key(|w| (w.at.1, w.at.0, w.address.clone()));
    on
}

/// A window's card for the plugin: its address and place, logical pixels
/// relative to the overview's surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub address: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The arguments of `dispatch koje:overview` (plugin/src/main.cpp): `open`
/// flies the windows in, `update` moves them.
pub fn plugin_command(open: bool, monitor: &str, places: &[Place]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("{} {monitor}", if open { "open" } else { "update" });
    for p in places {
        let _ = write!(out, " {}:{},{},{},{}", p.address, p.x, p.y, p.w, p.h);
    }
    out
}

/// What to send the plugin now, if anything: the command, and the key that
/// tells next time whether anything changed (`sent`). `opened`: the plugin
/// has the overview already. A workspace without windows is sent as well –
/// the plugin then stops drawing the previous one's (M8.79).
pub fn plugin_send(
    opened: bool,
    sent: &str,
    monitor: &str,
    places: &[Place],
) -> Option<(String, String)> {
    let key = plugin_command(false, monitor, places);
    if opened && sent == key {
        return None;
    }
    Some((plugin_command(!opened, monitor, places), key))
}

/// A window's place in the overview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Windows are shown at most at their real size.
const MAX_SCALE: f64 = 1.0;

/// Place windows of the given sizes in an area of `width` × `height` as
/// GNOME does: in rows, in their order, all at one scale – the number of
/// rows that allows the largest – centred, `gap` apart.
pub fn arrange(sizes: &[(f64, f64)], width: f64, height: f64, gap: f64) -> Vec<Rect> {
    if sizes.is_empty() || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let sizes: Vec<(f64, f64)> = sizes
        .iter()
        .map(|&(w, h)| (w.max(1.0), h.max(1.0)))
        .collect();
    let mut best: Option<(f64, Vec<Vec<usize>>)> = None;
    for rows in 1..=sizes.len() {
        let split = split(&sizes, rows);
        let scale = fit(&split, &sizes, width, height, gap);
        if best.as_ref().is_none_or(|(s, _)| scale > *s + 1e-9) {
            best = Some((scale, split));
        }
    }
    let Some((scale, rows)) = best else {
        return Vec::new();
    };
    let row_height = |row: &[usize]| row.iter().map(|&i| sizes[i].1).fold(0.0, f64::max) * scale;
    let total = rows.iter().map(|r| row_height(r)).sum::<f64>() + gap * count(rows.len() - 1);
    let mut out = vec![
        Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0
        };
        sizes.len()
    ];
    let mut y = (height - total) / 2.0;
    for row in &rows {
        let h = row_height(row);
        let w = row.iter().map(|&i| sizes[i].0 * scale).sum::<f64>() + gap * count(row.len() - 1);
        let mut x = (width - w) / 2.0;
        for &i in row {
            let (ww, wh) = (sizes[i].0 * scale, sizes[i].1 * scale);
            out[i] = Rect {
                x,
                y: y + (h - wh) / 2.0,
                w: ww,
                h: wh,
            };
            x += ww + gap;
        }
        y += h + gap;
    }
    out
}

fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The windows in at most `rows` rows of about equal width, in order.
fn split(sizes: &[(f64, f64)], rows: usize) -> Vec<Vec<usize>> {
    let target = sizes.iter().map(|s| s.0).sum::<f64>() / count(rows);
    let mut out: Vec<Vec<usize>> = vec![Vec::new()];
    let mut width = 0.0;
    for (i, &(w, _)) in sizes.iter().enumerate() {
        let left = sizes.len() - i;
        let rows_left = rows - out.len();
        let full = width + w / 2.0 > target;
        // A new row when this one is full, or when every window left needs
        // a row of its own.
        if let Some(row) = out.last()
            && !row.is_empty()
            && rows_left > 0
            && (full || left <= rows_left)
        {
            out.push(Vec::new());
            width = 0.0;
        }
        if let Some(row) = out.last_mut() {
            row.push(i);
        }
        width += w;
    }
    out
}

/// The largest scale at which the rows fit.
fn fit(rows: &[Vec<usize>], sizes: &[(f64, f64)], width: f64, height: f64, gap: f64) -> f64 {
    let mut scale = MAX_SCALE;
    let mut heights = 0.0;
    for row in rows {
        let w: f64 = row.iter().map(|&i| sizes[i].0).sum();
        let room = width - gap * count(row.len() - 1);
        scale = scale.min(room / w);
        heights += row.iter().map(|&i| sizes[i].1).fold(0.0, f64::max);
    }
    let room = height - gap * count(rows.len() - 1);
    scale.min(room / heights).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(r: &Rect, width: f64, height: f64) -> bool {
        r.x >= -1e-6 && r.y >= -1e-6 && r.x + r.w <= width + 1e-6 && r.y + r.h <= height + 1e-6
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.w - 1e-6
            && b.x < a.x + a.w - 1e-6
            && a.y < b.y + b.h - 1e-6
            && b.y < a.y + a.h - 1e-6
    }

    #[test]
    fn one_window_is_centred_at_its_size_or_smaller() {
        let r = arrange(&[(800.0, 600.0)], 1600.0, 1000.0, 20.0);
        assert_eq!(
            r,
            [Rect {
                x: 400.0,
                y: 200.0,
                w: 800.0,
                h: 600.0
            }]
        );
        // Larger than the area: scaled down, proportions kept.
        let r = arrange(&[(2000.0, 1000.0)], 1000.0, 1000.0, 20.0)[0];
        assert!((r.w - 1000.0).abs() < 1e-6 && (r.h - 500.0).abs() < 1e-6);
        assert!((r.y - 250.0).abs() < 1e-6);
    }

    #[test]
    fn two_halves_side_by_side() {
        // A tiled screen: two windows of half its width.
        let r = arrange(&[(960.0, 1040.0), (960.0, 1040.0)], 1600.0, 800.0, 20.0);
        assert_eq!(r.len(), 2);
        assert!((r[0].y - r[1].y).abs() < 1e-6, "one row: {r:?}");
        assert!(r[0].x < r[1].x);
        assert!(!overlap(&r[0], &r[1]));
    }

    #[test]
    fn many_windows_take_rows_inside_the_area_without_overlap() {
        let sizes: Vec<(f64, f64)> = (0..7)
            .map(|i| (600.0 + 100.0 * f64::from(i), 500.0))
            .collect();
        let r = arrange(&sizes, 1600.0, 900.0, 24.0);
        assert_eq!(r.len(), 7);
        assert!(r.iter().any(|a| (a.y - r[0].y).abs() > 1.0), "rows: {r:?}");
        for (i, a) in r.iter().enumerate() {
            assert!(inside(a, 1600.0, 900.0), "{a:?}");
            for b in &r[i + 1..] {
                assert!(!overlap(a, b), "{a:?} {b:?}");
            }
            // Proportions kept.
            let (w, h) = sizes[i];
            assert!((a.w / a.h - w / h).abs() < 1e-6);
        }
    }

    #[test]
    fn the_plugin_gets_the_cards() {
        let p = |a: &str, x| Place {
            address: a.into(),
            x,
            y: 80,
            w: 640,
            h: 400,
        };
        assert_eq!(
            plugin_command(true, "DP-1", &[p("0xa", 10), p("0xb", 700)]),
            "open DP-1 0xa:10,80,640,400 0xb:700,80,640,400"
        );
        assert_eq!(plugin_command(false, "DP-1", &[]), "update DP-1");
    }

    #[test]
    fn an_empty_workspace_clears_the_plugins_windows() {
        let p = |a: &str| Place {
            address: a.to_owned(),
            x: 1,
            y: 2,
            w: 3,
            h: 4,
        };
        let (open, key) = plugin_send(false, "", "DP-1", &[p("0xa")]).unwrap();
        assert!(open.starts_with("open DP-1 0xa:"));
        assert_eq!(plugin_send(true, &key, "DP-1", &[p("0xa")]), None);
        // Switched to an empty workspace: the plugin is told so.
        assert_eq!(
            plugin_send(true, &key, "DP-1", &[]),
            Some(("update DP-1".to_owned(), "update DP-1".to_owned()))
        );
    }

    #[test]
    fn nothing_to_arrange() {
        assert_eq!(arrange(&[], 100.0, 100.0, 10.0), []);
        assert_eq!(arrange(&[(10.0, 10.0)], 0.0, 100.0, 10.0), []);
    }

    #[test]
    fn rows_keep_the_order_and_none_is_empty() {
        let sizes = [(100.0, 100.0); 5];
        for rows in 1..=5 {
            let split = split(&sizes, rows);
            assert_eq!(split.len(), rows, "{split:?}");
            assert!(split.iter().all(|r| !r.is_empty()));
            let flat: Vec<usize> = split.concat();
            assert_eq!(flat, [0, 1, 2, 3, 4]);
        }
    }

    #[test]
    fn windows_in_reading_order() {
        let w = |address: &str, ws: &str, x: i32, y: i32| WindowState {
            pinnable: true,
            address: address.into(),
            class: "foot".into(),
            title: String::new(),
            workspace: ws.into(),
            at: (x, y),
            size: (100, 100),
            focus_history: 0,
            context: None,
        };
        let all = [
            w("0x1", "kp", 960, 40),
            w("0x2", "kp", 0, 40),
            w("0x3", "kp", 960, 560),
            w("0x4", "1", 0, 0),
        ];
        let on: Vec<&str> = on_workspace(&all, "kp")
            .iter()
            .map(|w| w.address.as_str())
            .collect();
        assert_eq!(on, ["0x2", "0x1", "0x3"]);
    }
}
