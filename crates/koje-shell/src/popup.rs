//! The bar's dropdowns stay inside the monitor, as GNOME's menus do (M8.88).
//! A popover is centered on its button; near the screen's edge the
//! compositor slides it inside, but when the popover changes size while open
//! (the koje menu's "New context…" form) GTK presents it again and neither
//! Hyprland nor sway slides it back. So the shell moves it itself, with an
//! offset that makes sliding unnecessary.

use std::cell::Cell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;

/// Room kept between a dropdown and the monitor's edge.
const MARGIN: i32 = 4;

/// The horizontal offset for a dropdown `width` wide centered on `center`,
/// so that it stays within `0..screen` and `MARGIN` away from the edges; a
/// dropdown wider than the screen keeps its left edge inside.
pub fn offset(center: i32, width: i32, screen: i32) -> i32 {
    let left = center - width / 2;
    let right = left + width;
    if width > screen - 2 * MARGIN || left < MARGIN {
        MARGIN - left
    } else if right > screen - MARGIN {
        (screen - MARGIN) - right
    } else {
        0
    }
}

/// Keep `popover` inside its window (the bar spans the monitor): checked on
/// every frame while it is shown, so a size change moves it at once.
pub fn keep_inside(popover: &gtk::Popover) {
    let last = Rc::new(Cell::new(None::<(i32, i32, i32)>));
    popover.connect_map(move |popover| {
        last.set(None);
        let last = last.clone();
        popover.add_tick_callback(move |popover, _| {
            if !popover.is_mapped() {
                return gtk4::glib::ControlFlow::Break;
            }
            let (Some(anchor), Some(root)) = (popover.parent(), popover.root()) else {
                return gtk4::glib::ControlFlow::Continue;
            };
            let Some(bounds) = anchor.compute_bounds(&root) else {
                return gtk4::glib::ControlFlow::Continue;
            };
            #[allow(clippy::cast_possible_truncation, reason = "pixels")]
            let center = (bounds.x() + bounds.width() / 2.0).round() as i32;
            let now = (center, popover.width(), root.width());
            if last.get() != Some(now) && now.1 > 0 {
                last.set(Some(now));
                let (_, y) = popover.offset();
                popover.set_offset(offset(now.0, now.1, now.2), y);
            }
            gtk4::glib::ControlFlow::Continue
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdowns_stay_on_the_screen() {
        // In the middle: centered on the button.
        assert_eq!(offset(960, 400, 1920), 0);
        // The koje menu at the right edge: moved left to end 4 px inside.
        assert_eq!(offset(1824, 415, 1920), -(1824 + 207 - 1916 + 1));
        assert_eq!(1824 - 415 / 2 + offset(1824, 415, 1920) + 415, 1916);
        // At the left edge: moved right.
        assert_eq!(offset(20, 300, 1920), 4 - (20 - 150));
        // Wider than the screen: the left edge stays inside.
        assert_eq!(offset(1000, 2000, 1920), 4);
    }
}
