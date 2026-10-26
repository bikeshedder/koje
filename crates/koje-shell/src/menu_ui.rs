//! The koje menu's widgets (M8.55): a menu button with the koje icon and the
//! active context's memory meter, and a dropdown with the context in detail –
//! memory, a graph of the last minutes, editable limits – or, on a host
//! workspace, the running contexts. It only renders a [`MenuView`].

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::cairo;
use gtk4::prelude::*;
use koje_core::memory::MemoryLevel;
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

use crate::menu::{ContextPanel, Graph, MemoryView, MenuView, Panel, limit_arg};
use crate::model::{Accent, Action};
use crate::overview_ui::OverviewPage;

/// The koje icon's size in pixels.
const ICON_SIZE: i32 = 16;

/// The meter's width in pixels.
const METER_WIDTH: i32 = 64;
const TRACK: f64 = 4.0;
const SOFT_MARK: f64 = 10.0;
const GRAPH_HEIGHT: i32 = 96;
const DROPDOWN_WIDTH: i32 = 320;

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

/// Replace `widget`'s accent class.
fn set_accent(widget: &impl IsA<gtk::Widget>, accent: &Accent) {
    for class in widget.css_classes() {
        if class.starts_with("koje-ctx-") || class == "koje-host" {
            widget.remove_css_class(&class);
        }
    }
    widget.add_css_class(&accent.css_class());
}

/// A memory meter that draws whatever its cell holds.
#[derive(Debug, Clone)]
pub struct Meter {
    pub area: gtk::DrawingArea,
    pub label: gtk::Label,
    view: Rc<RefCell<Option<MemoryView>>>,
}

impl Meter {
    pub fn new() -> Self {
        let view: Rc<RefCell<Option<MemoryView>>> = Rc::default();
        let area = gtk::DrawingArea::builder()
            .content_width(METER_WIDTH)
            .content_height(12)
            .valign(gtk::Align::Center)
            .build();
        area.set_draw_func({
            let view = view.clone();
            move |area, cr, w, h| {
                if let Some(m) = view.borrow().as_ref() {
                    draw_meter(area, cr, f64::from(w), f64::from(h), m);
                }
            }
        });
        let label = gtk::Label::builder().css_classes(["numeric"]).build();
        Self { area, label, view }
    }

    pub fn set(&self, view: Option<&MemoryView>) {
        self.area
            .set_visible(view.is_some_and(|m| m.fill.is_some()));
        self.label.set_visible(view.is_some());
        if let Some(m) = view {
            self.label.set_label(&m.label);
        }
        self.view.replace(view.cloned());
        self.area.queue_draw();
    }
}

/// The context page of the dropdown.
#[derive(Debug)]
struct ContextPage {
    root: gtk::Box,
    name: gtk::Label,
    summary: gtk::Label,
    graph: gtk::DrawingArea,
    span: gtk::Label,
    graph_view: Rc<RefCell<Option<Graph>>>,
    soft: adw::EntryRow,
    hard: adw::EntryRow,
    error: gtk::Label,
    /// The context and the limits last shown: the entries change only when
    /// those change, not under the user's typing.
    shown: Rc<RefCell<(String, String, String)>>,
}

impl ContextPage {
    fn new(actions: &UnboundedSender<Action>) -> Self {
        let name = gtk::Label::builder()
            .css_classes(["capsule"])
            .halign(gtk::Align::Start)
            .build();
        let summary = gtk::Label::builder()
            .css_classes(["dim-label"])
            .halign(gtk::Align::Start)
            .build();
        let graph_view: Rc<RefCell<Option<Graph>>> = Rc::default();
        let graph = gtk::DrawingArea::builder()
            .content_height(GRAPH_HEIGHT)
            .css_classes(["koje-graph"])
            .build();
        graph.set_draw_func({
            let view = graph_view.clone();
            move |area, cr, w, h| {
                if let Some(g) = view.borrow().as_ref() {
                    draw_graph(area, cr, f64::from(w), f64::from(h), g);
                }
            }
        });
        let span = gtk::Label::builder()
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();
        let soft = adw::EntryRow::builder()
            .title("Soft limit")
            .show_apply_button(true)
            .build();
        let hard = adw::EntryRow::builder()
            .title("Hard limit")
            .show_apply_button(true)
            .build();
        let limits = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        limits.append(&soft);
        limits.append(&hard);
        let hint = gtk::Label::builder()
            .label("e.g. 6G or 512M; empty for none")
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();
        let error = gtk::Label::builder()
            .css_classes(["caption", "error"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .visible(false)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        for w in [
            name.upcast_ref::<gtk::Widget>(),
            summary.upcast_ref(),
            graph.upcast_ref(),
            span.upcast_ref(),
            limits.upcast_ref(),
            hint.upcast_ref(),
            error.upcast_ref(),
        ] {
            root.append(w);
        }
        let shown: Rc<RefCell<(String, String, String)>> = Rc::default();

        apply_limits(&soft, &hard, &error, &shown, actions);

        Self {
            root,
            name,
            summary,
            graph,
            span,
            graph_view,
            soft,
            hard,
            error,
            shown,
        }
    }

    fn render(&self, p: &ContextPanel) {
        self.name.set_label(&p.name);
        set_accent(&self.root, &p.accent);
        self.summary.set_label(&p.summary);
        self.summary.set_visible(!p.summary.is_empty());
        self.graph.set_visible(p.graph.is_some());
        self.span.set_visible(p.graph.is_some());
        if let Some(g) = &p.graph {
            self.span.set_label(&g.span);
        }
        self.graph_view.replace(p.graph.clone());
        self.graph.queue_draw();

        let now = (p.name.clone(), p.soft.clone(), p.hard.clone());
        if *self.shown.borrow() != now {
            self.soft.set_text(&p.soft);
            self.hard.set_text(&p.hard);
            self.shown.replace(now);
        }
        match &p.error {
            Some(e) => {
                self.error.set_label(e);
                self.error.set_visible(true);
            }
            None => self.error.set_visible(false),
        }
    }
}

#[derive(Debug)]
pub struct MenuWidget {
    button: gtk::MenuButton,
    icon: gtk::DrawingArea,
    meter: Meter,
    popover: gtk::Popover,
    stack: gtk::Stack,
    context: ContextPage,
    overview: Rc<OverviewPage>,
}

impl MenuWidget {
    pub fn new(actions: &UnboundedSender<Action>) -> Self {
        let icon = koje_icon();
        let meter = Meter::new();
        let face = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        face.append(&icon);
        face.append(&meter.area);
        face.append(&meter.label);

        let context = ContextPage::new(actions);
        let stack = gtk::Stack::builder()
            .width_request(DROPDOWN_WIDTH)
            .vhomogeneous(false)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        let popover = gtk::Popover::builder().child(&stack).build();
        let overview = OverviewPage::new(&popover, actions);
        stack.add_named(&context.root, Some("context"));
        stack.add_named(&overview.root, Some("overview"));
        let button = gtk::MenuButton::builder()
            .child(&face)
            .popover(&popover)
            .css_classes(["panel-button", "koje-menu"])
            .tooltip_text("koje")
            .visible(false)
            .build();
        Self {
            button,
            icon,
            meter,
            popover,
            stack,
            context,
            overview,
        }
    }

    pub fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }

    pub fn render(&self, menu: Option<&MenuView>) {
        let Some(m) = menu else {
            self.popover.popdown();
            self.button.set_visible(false);
            return;
        };
        self.button.set_visible(true);
        set_accent(&self.icon, &m.accent);
        self.meter.set(m.meter.as_ref());
        self.button.set_tooltip_text(Some(
            m.meter.as_ref().map_or("koje", |v| v.tooltip.as_str()),
        ));
        match &m.panel {
            Panel::Context(p) => {
                self.context.render(p);
                self.stack.set_visible_child_name("context");
            }
            Panel::Overview(o) => {
                self.overview.render(o);
                self.stack.set_visible_child_name("overview");
            }
        }
    }
}

/// Either entry applies both: what is typed in the other one counts too.
/// Checked here first, so `koje limit` only gets sizes it takes.
fn apply_limits(
    soft: &adw::EntryRow,
    hard: &adw::EntryRow,
    error: &gtk::Label,
    shown: &Rc<RefCell<(String, String, String)>>,
    actions: &UnboundedSender<Action>,
) {
    for row in [soft, hard] {
        let (soft, hard, error, shown, actions) = (
            soft.clone(),
            hard.clone(),
            error.clone(),
            shown.clone(),
            actions.clone(),
        );
        row.connect_apply(move |_| {
            let context = shown.borrow().0.clone();
            match (limit_arg(&soft.text()), limit_arg(&hard.text())) {
                (Ok(soft), Ok(hard)) => {
                    error.set_visible(false);
                    let _ = actions.send(Action::SetLimits {
                        context,
                        soft,
                        hard,
                    });
                }
                (Err(e), _) | (_, Err(e)) => {
                    error.set_label(&e);
                    error.set_visible(true);
                }
            }
        });
    }
}

/// koje's icon – a bunk, as `assets/icons/io.koje.Koje-symbolic.svg` – drawn
/// in the widget's CSS color. GTK 4.18 with librsvg 2.60 does not recolor
/// symbolic SVGs loaded from files (only its built-in icons), so the bar
/// draws it itself.
fn koje_icon() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(ICON_SIZE)
        .content_height(ICON_SIZE)
        .valign(gtk::Align::Center)
        .css_classes(["koje-icon"])
        .build();
    area.set_draw_func(|area, cr, w, h| {
        let color = rgb_of(area);
        let scale = f64::from(w.min(h)) / 16.0;
        cr.scale(scale, scale);
        cr.set_source_rgb(color.0, color.1, color.2);
        // Two posts, two berths, two pillows (the SVG's geometry).
        line_v(cr, 2.0, 2.0, 14.0, 2.0);
        line_v(cr, 14.0, 2.0, 14.0, 2.0);
        cr.rectangle(3.0, 5.0, 10.0, 2.0);
        cr.rectangle(3.0, 11.0, 10.0, 2.0);
        line(cr, 4.0, 7.5, 3.5, 2.0);
        line(cr, 4.0, 7.5, 9.5, 2.0);
        let _ = cr.fill();
    });
    area
}

/// A vertical line with round ends at `x` from `y0` to `y1`.
fn line_v(cr: &cairo::Context, x: f64, y0: f64, y1: f64, thickness: f64) {
    let r = thickness / 2.0;
    cr.new_sub_path();
    cr.arc(x, y0, r, PI, 0.0);
    cr.arc(x, y1, r, 0.0, PI);
    cr.close_path();
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

fn rgb_of(widget: &impl IsA<gtk::Widget>) -> (f64, f64, f64) {
    let c = widget.color();
    (
        f64::from(c.red()),
        f64::from(c.green()),
        f64::from(c.blue()),
    )
}

fn draw_meter(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: f64,
    height: f64,
    m: &MemoryView,
) {
    let Some(fill) = m.fill else {
        return;
    };
    let text = rgb_of(area);
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

/// The working set over time as an area in the accent color (the graph's
/// CSS `color`), the soft limit as a dashed and the hard one as a solid line.
fn draw_graph(area: &gtk::DrawingArea, cr: &cairo::Context, width: f64, height: f64, g: &Graph) {
    #[allow(clippy::cast_precision_loss)] // bytes to pixels
    let y = |bytes: u64| height - height * (bytes as f64 / g.top as f64);
    let n = g.used.len();
    #[allow(clippy::cast_precision_loss)]
    let x = |i: usize| {
        if n < 2 {
            width
        } else {
            width * i as f64 / (n - 1) as f64
        }
    };
    let accent = rgb_of(area);

    cr.move_to(0.0, height);
    for (i, &v) in g.used.iter().enumerate() {
        if n == 1 {
            cr.line_to(0.0, y(v));
        }
        cr.line_to(x(i), y(v));
    }
    cr.line_to(width, height);
    cr.close_path();
    cr.set_source_rgba(accent.0, accent.1, accent.2, 0.25);
    let _ = cr.fill_preserve();
    cr.new_path();
    for (i, &v) in g.used.iter().enumerate() {
        if i == 0 {
            cr.move_to(if n == 1 { 0.0 } else { x(i) }, y(v));
        }
        cr.line_to(x(i), y(v));
    }
    cr.set_source_rgb(accent.0, accent.1, accent.2);
    cr.set_line_width(2.0);
    let _ = cr.stroke();

    let limit = |bytes: u64, color: (f64, f64, f64), dashed: bool| {
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.set_line_width(1.0);
        cr.set_dash(if dashed { &[4.0, 3.0] } else { &[] }, 0.0);
        let at = y(bytes).round() + 0.5;
        cr.move_to(0.0, at);
        cr.line_to(width, at);
        let _ = cr.stroke();
    };
    if let Some(soft) = g.soft {
        limit(soft, level_color(MemoryLevel::NearSoft), true);
    }
    if let Some(hard) = g.hard {
        limit(hard, level_color(MemoryLevel::OverHard), false);
    }
    cr.set_dash(&[], 0.0);
}
