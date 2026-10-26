//! The bar's layout, as GNOME's top bar: the middle child (the clock) always
//! centred; the start child (workspaces, tasks) gets the room up to it and
//! shrinks – titles ellipsize – rather than push it aside, as `GtkCenterBox`
//! does; the end child likewise from the right.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

/// Room between the middle child and its neighbours.
const GAP: i32 = 12;

mod imp {
    use super::{GAP, glib, gtk, middle_x};
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Debug, Default)]
    pub struct BarLayout;

    #[glib::object_subclass]
    impl ObjectSubclass for BarLayout {
        const NAME: &'static str = "KojeBarLayout";
        type Type = super::BarLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for BarLayout {}

    fn children(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut out = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.should_layout() {
                out.push(c);
            }
        }
        out
    }

    impl LayoutManagerImpl for BarLayout {
        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let sizes: Vec<(i32, i32)> = children(widget)
                .iter()
                .map(|c| {
                    let (min, nat, _, _) = c.measure(orientation, for_size);
                    (min, nat)
                })
                .collect();
            let (min, nat) = if orientation == gtk::Orientation::Horizontal {
                let gaps = GAP * 2;
                (
                    sizes.iter().map(|s| s.0).sum::<i32>() + gaps,
                    sizes.iter().map(|s| s.1).sum::<i32>() + gaps,
                )
            } else {
                (
                    sizes.iter().map(|s| s.0).max().unwrap_or(0),
                    sizes.iter().map(|s| s.1).max().unwrap_or(0),
                )
            };
            (min, nat, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, _baseline: i32) {
            let [start, middle, end] = &children(widget)[..] else {
                return;
            };
            let natural = |c: &gtk::Widget| c.measure(gtk::Orientation::Horizontal, height);
            let (_, middle_w, _, _) = natural(middle);
            let middle_w = middle_w.min(width);
            let (start_min, start_nat, _, _) = natural(start);
            let (end_min, end_nat, _, _) = natural(end);
            let middle_x = middle_x(width, middle_w, start_min, end_min);
            let start_w = start_nat.min(middle_x - GAP).max(start_min);
            let end_w = end_nat.min(width - middle_x - middle_w - GAP).max(end_min);
            let place = |c: &gtk::Widget, x: i32, w: i32| {
                c.size_allocate(&gtk::Allocation::new(x, 0, w, height), -1);
            };
            place(start, 0, start_w);
            place(middle, middle_x, middle_w);
            place(end, width - end_w, end_w);
        }
    }
}

/// Where the middle child goes: centred, unless the sides' minimum widths
/// do not fit beside it – then just far enough aside.
pub fn middle_x(width: i32, middle_w: i32, start_min: i32, end_min: i32) -> i32 {
    let centred = (width - middle_w) / 2;
    let leftmost = start_min + GAP;
    let rightmost = width - end_min - GAP - middle_w;
    if leftmost > rightmost {
        // Not even the minimums fit: share the shortfall.
        return i32::midpoint(leftmost, rightmost);
    }
    centred.clamp(leftmost, rightmost)
}

glib::wrapper! {
    pub struct BarLayout(ObjectSubclass<imp::BarLayout>)
        @extends gtk::LayoutManager;
}

impl Default for BarLayout {
    fn default() -> Self {
        glib::Object::new()
    }
}

/// A box laid out by [`BarLayout`]: `start`, `middle`, `end`.
pub fn bar_box(
    start: &impl IsA<gtk::Widget>,
    middle: &impl IsA<gtk::Widget>,
    end: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_layout_manager(Some(BarLayout::default()));
    b.append(start);
    b.append(middle);
    b.append(end);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_stays_centred_while_the_sides_fit() {
        assert_eq!(middle_x(1280, 100, 300, 200), 590);
        // The start side needs more than half: the middle moves right.
        assert_eq!(middle_x(1000, 100, 600, 150), 612);
        // The end side needs more: it moves left.
        assert_eq!(middle_x(1000, 100, 100, 600), 288);
        // Nothing fits: halfway between.
        assert_eq!(middle_x(500, 100, 300, 300), 200);
    }
}
