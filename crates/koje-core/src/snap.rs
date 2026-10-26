//! Snapping a window as GNOME's Super+arrows do (M8.41, ADR 0043; the first
//! zones of M7.3): the left or right half of its monitor below the bar, all
//! of it, and back to where it was. Pure: kojed gives the monitor's area and
//! the window's place and remembers the place from before.

use std::str::FromStr;

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Maximize,
    /// Where it was before it snapped (Super+Down).
    Restore,
}

impl FromStr for Zone {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "left" => Self::Left,
            "right" => Self::Right,
            "maximize" => Self::Maximize,
            "restore" => Self::Restore,
            _ => {
                return Err(format!(
                    "unknown zone {s:?} – left, right, maximize or restore"
                ));
            }
        })
    }
}

/// A monitor in logical pixels, with what bars and panels reserve on each
/// side (`[left, top, right, bottom]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorArea {
    /// Connector name, as in [`crate::layout::Placement::monitor`].
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub reserved: [u32; 4],
}

/// The part of a monitor windows may fill, relative to the monitor.
pub fn usable(m: &MonitorArea) -> Rect {
    let [left, top, right, bottom] = m.reserved;
    Rect {
        x: i32::try_from(left).unwrap_or(0),
        y: i32::try_from(top).unwrap_or(0),
        w: m.width.saturating_sub(left + right).max(1),
        h: m.height.saturating_sub(top + bottom).max(1),
    }
}

fn zone_rect(zone: Zone, area: Rect) -> Option<Rect> {
    let half = area.w / 2;
    Some(match zone {
        Zone::Left => Rect { w: half, ..area },
        Zone::Right => Rect {
            x: area.x + i32::try_from(half).unwrap_or(0),
            w: area.w - half,
            ..area
        },
        Zone::Maximize => area,
        Zone::Restore => return None,
    })
}

/// Where a window at `current` goes for `zone` in `area`, and what to
/// remember of where it was (`None`: forget). `before` is what was
/// remembered; it counts only while the window still sits in a zone – one
/// moved since forgets it, as in GNOME. `None` when nothing moves (restore
/// without a place from before).
pub fn snap(
    zone: Zone,
    area: Rect,
    current: Rect,
    before: Option<Rect>,
) -> Option<(Rect, Option<Rect>)> {
    let snapped = [Zone::Left, Zone::Right, Zone::Maximize]
        .into_iter()
        .any(|z| zone_rect(z, area) == Some(current));
    let before = before.filter(|_| snapped);
    match zone_rect(zone, area) {
        Some(target) => Some((target, Some(before.unwrap_or(current)))),
        None => before.map(|b| (b, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    fn monitor() -> MonitorArea {
        MonitorArea {
            name: "DP-1".into(),
            width: 2560,
            height: 1440,
            reserved: [0, 32, 0, 0],
        }
    }

    #[test]
    fn zones_lie_below_the_bar() {
        let area = usable(&monitor());
        assert_eq!(area, rect(0, 32, 2560, 1408));
        let window = rect(300, 200, 800, 600);
        assert_eq!(
            snap(Zone::Left, area, window, None),
            Some((rect(0, 32, 1280, 1408), Some(window)))
        );
        assert_eq!(
            snap(Zone::Right, area, window, None),
            Some((rect(1280, 32, 1280, 1408), Some(window)))
        );
        assert_eq!(
            snap(Zone::Maximize, area, window, None),
            Some((area, Some(window)))
        );
        assert_eq!("left".parse::<Zone>(), Ok(Zone::Left));
        assert!("up".parse::<Zone>().is_err());
    }

    #[test]
    fn restore_goes_back_to_before_the_first_snap() {
        let area = usable(&monitor());
        let window = rect(300, 200, 800, 600);
        let (left, before) = snap(Zone::Left, area, window, None).unwrap();
        // Left, then maximized: still back to where it was at first.
        let (max, before) = snap(Zone::Maximize, area, left, before).unwrap();
        assert_eq!(before, Some(window));
        assert_eq!(snap(Zone::Restore, area, max, before), Some((window, None)));
        // Nothing to restore.
        assert_eq!(snap(Zone::Restore, area, window, None), None);
    }

    #[test]
    fn a_window_moved_since_forgets_where_it_was() {
        let area = usable(&monitor());
        let window = rect(300, 200, 800, 600);
        let moved = rect(100, 100, 900, 700);
        assert_eq!(snap(Zone::Restore, area, moved, Some(window)), None);
        assert_eq!(
            snap(Zone::Left, area, moved, Some(window)),
            Some((rect(0, 32, 1280, 1408), Some(moved)))
        );
    }

    #[test]
    fn odd_widths_leave_no_gap() {
        let area = rect(0, 0, 1921, 1000);
        let (l, _) = snap(Zone::Left, area, rect(0, 0, 1, 1), None).unwrap();
        let (r, _) = snap(Zone::Right, area, rect(0, 0, 1, 1), None).unwrap();
        assert_eq!(l.w + r.w, 1921);
        assert_eq!(i64::from(r.x), i64::from(l.w));
    }
}
