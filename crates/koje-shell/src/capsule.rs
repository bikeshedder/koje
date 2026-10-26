//! The active context's capsule (M8.29): its name as a pill in its color, and
//! its memory as a meter with the share of its limit – details on hover.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::cairo;
use gtk4::prelude::*;
use koje_core::memory::MemoryLevel;

use crate::model::{Capsule, MemoryView};

/// The meter's size in pixels.
const METER_WIDTH: i32 = 64;
const TRACK: f64 = 4.0;
const SOFT_MARK: f64 = 10.0;

/// The used part by level (GNOME's palette): normal, near the soft limit
/// (90 %), over it, at the hard one (M8.21's levels).
fn level_color(level: MemoryLevel) -> (f64, f64, f64) {
    let rgb = |r: u8, g: u8, b: u8| {
        (
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        )
    };
    match level {
        MemoryLevel::Normal => rgb(0x2e, 0xc2, 0x7e),
        MemoryLevel::NearSoft => rgb(0xe5, 0xa5, 0x0a),
        MemoryLevel::OverSoft => rgb(0xe6, 0x61, 0x00),
        MemoryLevel::OverHard => rgb(0xe0, 0x1b, 0x24),
    }
}

#[derive(Debug)]
pub struct CapsuleWidget {
    root: gtk::Box,
    name: gtk::Label,
    meter: gtk::DrawingArea,
    label: gtk::Label,
    /// What the meter draws.
    memory: Rc<RefCell<Option<MemoryView>>>,
}

impl CapsuleWidget {
    pub fn new() -> Self {
        let name = gtk::Label::builder()
            .css_classes(["capsule"])
            .valign(gtk::Align::Center)
            .build();
        let memory: Rc<RefCell<Option<MemoryView>>> = Rc::default();
        let meter = gtk::DrawingArea::builder()
            .content_width(METER_WIDTH)
            .valign(gtk::Align::Fill)
            .build();
        meter.set_draw_func({
            let memory = memory.clone();
            move |area, cr, w, h| {
                if let Some(m) = memory.borrow().as_ref() {
                    draw(area, cr, f64::from(w), f64::from(h), m);
                }
            }
        });
        let label = gtk::Label::builder().css_classes(["numeric"]).build();
        let root = gtk::Box::builder()
            .spacing(8)
            .css_classes(["capsule-group"])
            .visible(false)
            .build();
        root.append(&name);
        root.append(&meter);
        root.append(&label);
        Self {
            root,
            name,
            meter,
            label,
            memory,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn render(&self, capsule: Option<&Capsule>) {
        let Some(c) = capsule else {
            self.root.set_visible(false);
            return;
        };
        self.root.set_visible(true);
        self.name.set_label(&c.name);
        for class in self.name.css_classes() {
            if class.starts_with("koje-ctx-") || class == "koje-host" {
                self.name.remove_css_class(&class);
            }
        }
        self.name.add_css_class(&c.accent.css_class());
        if let Some(m) = &c.memory {
            self.meter.set_visible(m.fill.is_some());
            self.label.set_label(&m.label);
            self.label.set_visible(true);
            self.root.set_tooltip_text(Some(&m.tooltip));
        } else {
            self.meter.set_visible(false);
            self.label.set_visible(false);
            self.root.set_tooltip_text(None);
        }
        self.memory.replace(c.memory.clone());
        self.meter.queue_draw();
    }
}

/// A line with round ends from `x0` to `x1`, `thickness` high, centred at `y`.
fn line(cr: &cairo::Context, x0: f64, x1: f64, y: f64, thickness: f64) {
    let r = thickness / 2.0;
    if x1 - x0 < thickness {
        cr.rectangle(x0, y - r, (x1 - x0).max(0.0), thickness);
        return;
    }
    cr.new_sub_path();
    cr.arc(x1 - r, y, r, -PI / 2.0, PI / 2.0);
    cr.arc(x0 + r, y, r, PI / 2.0, 3.0 * PI / 2.0);
    cr.close_path();
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, width: f64, height: f64, m: &MemoryView) {
    let Some(fill) = m.fill else {
        return;
    };
    let text = area.color();
    let text = (
        f64::from(text.red()),
        f64::from(text.green()),
        f64::from(text.blue()),
    );
    let middle = height / 2.0;
    // The free part: the text color, faint.
    cr.set_source_rgba(text.0, text.1, text.2, 0.2);
    line(cr, 0.0, width, middle, TRACK);
    let _ = cr.fill();
    // The used part in its level's color.
    let used = level_color(m.level);
    cr.set_source_rgb(used.0, used.1, used.2);
    line(cr, 0.0, width * f64::from(fill) / 1000.0, middle, TRACK);
    let _ = cr.fill();
    // The soft limit: a tick across the line.
    if let Some(soft) = m.soft {
        let at = (width * f64::from(soft) / 1000.0).clamp(1.0, width - 1.0);
        cr.set_source_rgba(text.0, text.1, text.2, 0.7);
        cr.rectangle(at - 1.0, middle - SOFT_MARK / 2.0, 2.0, SOFT_MARK);
        let _ = cr.fill();
    }
}
