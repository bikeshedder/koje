//! The OSD (M8.26) as GNOME's: a pill at the bottom of every monitor with an
//! icon, the device's name and a level bar, shown for 1.5 s and faded in and
//! out in 100 ms; clicks go through it. `koje media` shows it through the
//! shell's `osd` action, scrolling over the volume indicator directly. What
//! it shows is a [`media::Osd`]; the bar's parts are [`media::Osd::bar`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell as _};
use koje_core::media::{self, Bar};
use libadwaita as adw;
use libadwaita::prelude::AnimationExt as _;

/// GNOME's `HIDE_TIMEOUT` and `FADE_TIME` (osdWindow.js).
const SHOWN: Duration = Duration::from_millis(1500);
const FADE_MS: u32 = 100;
/// GNOME's level bar: 6 px high; the part beyond 100 % in the destructive
/// color (dark: `$red_4`, `#c01c28`), a 3 px gap at 100 %; the track the
/// text color at 10 %.
const BAR_HEIGHT: i32 = 6;
const OVERDRIVE: (f64, f64, f64) = (0.753, 0.110, 0.157);
const GAP: f64 = 3.0;
const TRACK_ALPHA: f64 = 0.1;

/// GNOME Shell 50's `.osd-window` (`_osd.scss`, `%osd_panel`): `$osd_bg_color`
/// (`#222226` lightened 5 %), a hairline border, the heading font in bold,
/// 32 px icon, 12 px between icon and text, 8 px between label and bar, 4em
/// above the bottom edge. Dark in both styles like its bar (ADR 0047).
pub const CSS: &str = "
window.koje-osd { background: transparent; }
.osd {
  background: #2e2e33;
  color: #ffffff;
  border: 1px solid alpha(#ffffff, 0.02);
  border-radius: 999px;
  padding: 12px 18px;
  margin: 0 0 4em 0;
  font-size: 11pt;
  font-weight: bold;
}
.osd image { -gtk-icon-size: 32px; }
.osd-label { margin-right: 6px; }
.osd-level { margin-right: 6px; }
";

/// One monitor's OSD.
#[derive(Debug)]
struct Card {
    monitor: gdk::Monitor,
    window: gtk::Window,
    icon: gtk::Image,
    label: gtk::Label,
    level: gtk::DrawingArea,
    bar: Rc<Cell<Option<Bar>>>,
}

impl Card {
    fn new(app: &adw::Application, monitor: &gdk::Monitor) -> Self {
        let window = gtk::Window::builder()
            .application(app)
            .css_classes(["koje-osd"])
            .build();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-osd"));
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_anchor(Edge::Bottom, true);
        window.set_monitor(Some(monitor));
        // Not in the way: clicks go to what is below.
        window.connect_realize(|w| {
            if let Some(surface) = w.surface() {
                surface.set_input_region(Some(&cairo::Region::create()));
            }
        });

        let icon = gtk::Image::new();
        let label = gtk::Label::builder()
            .css_classes(["osd-label"])
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .max_width_chars(32)
            .build();
        let bar: Rc<Cell<Option<Bar>>> = Rc::default();
        let level = gtk::DrawingArea::builder()
            .content_height(BAR_HEIGHT)
            .width_request(160)
            .hexpand(true)
            .build();
        {
            let bar = bar.clone();
            level.set_draw_func(move |area, cr, width, height| {
                if let Some(bar) = bar.get() {
                    draw_bar(cr, &area.color(), bar, f64::from(width), f64::from(height));
                }
            });
        }
        level.add_css_class("osd-level");
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .spacing(8)
            .hexpand(true)
            .build();
        text.append(&label);
        text.append(&level);
        let card = gtk::Box::builder().css_classes(["osd"]).spacing(12).build();
        card.append(&icon);
        card.append(&text);
        window.set_child(Some(&card));
        Self {
            monitor: monitor.clone(),
            window,
            icon,
            label,
            level,
            bar,
        }
    }

    fn set(&self, osd: &media::Osd) {
        self.icon.set_icon_name(Some(&osd.icon));
        match &osd.label {
            Some(l) => {
                self.label.set_label(l);
                self.label.set_visible(true);
            }
            None => self.label.set_visible(false),
        }
        let bar = osd.bar();
        self.level.set_visible(bar.is_some());
        self.bar.set(bar);
        self.level.queue_draw();
    }
}

/// The track, the level up to 100 %, the overdrive beyond it and a gap at
/// 100 % when the bar goes further – as GNOME's `BarLevel`.
fn draw_bar(cr: &cairo::Context, fg: &gdk::RGBA, bar: Bar, width: f64, height: f64) {
    let r = height / 2.0;
    let pill = |x0: f64, x1: f64| {
        cr.new_sub_path();
        cr.arc(
            x0 + r,
            r,
            r,
            std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2,
        );
        cr.arc(
            x1 - r,
            r,
            r,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        );
        cr.close_path();
    };
    let (red, green, blue) = (
        f64::from(fg.red()),
        f64::from(fg.green()),
        f64::from(fg.blue()),
    );
    cr.set_source_rgba(red, green, blue, TRACK_ALPHA);
    pill(0.0, width);
    let _ = cr.fill();
    // The filled part as one pill, painted in two colors.
    let end = (bar.overdrive * width).max(if bar.overdrive > 0.0 { height } else { 0.0 });
    if end <= 0.0 {
        return;
    }
    let _ = cr.save();
    pill(0.0, end);
    cr.clip();
    let split = bar.normal * width;
    cr.set_source_rgba(red, green, blue, f64::from(fg.alpha()));
    cr.rectangle(0.0, 0.0, split.max(height.min(end)), height);
    let _ = cr.fill();
    if end > split {
        cr.set_source_rgb(OVERDRIVE.0, OVERDRIVE.1, OVERDRIVE.2);
        cr.rectangle(split, 0.0, end - split, height);
        let _ = cr.fill();
    }
    let _ = cr.restore();
    if let Some(mark) = bar.mark {
        // A see-through gap at 100 %.
        let _ = cr.save();
        cr.set_operator(cairo::Operator::Clear);
        cr.rectangle(mark * width - GAP / 2.0, 0.0, GAP, height);
        let _ = cr.fill();
        let _ = cr.restore();
    }
}

/// The OSD on every monitor; a new one replaces what is shown.
#[derive(Debug)]
pub struct Osd {
    app: adw::Application,
    cards: RefCell<Vec<Card>>,
    hide: RefCell<Option<glib::SourceId>>,
    fade: RefCell<Option<adw::TimedAnimation>>,
}

impl Osd {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        Rc::new(Self {
            app: app.clone(),
            cards: RefCell::default(),
            hide: RefCell::default(),
            fade: RefCell::default(),
        })
    }

    /// One card per monitor, also after monitors came or went.
    fn sync(&self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitors: Vec<gdk::Monitor> = display
            .monitors()
            .iter::<gdk::Monitor>()
            .filter_map(Result::ok)
            .collect();
        let mut cards = self.cards.borrow_mut();
        cards.retain(|c| {
            let keep = monitors.contains(&c.monitor);
            if !keep {
                c.window.destroy();
            }
            keep
        });
        for m in &monitors {
            if !cards.iter().any(|c| &c.monitor == m) {
                cards.push(Card::new(&self.app, m));
            }
        }
    }

    pub fn show(self: &Rc<Self>, osd: &media::Osd) {
        self.sync();
        let shown = self.cards.borrow().iter().any(|c| c.window.is_visible());
        for card in self.cards.borrow().iter() {
            card.set(osd);
            card.window.set_visible(true);
        }
        // Fading out: back to full; hidden: fade in. Paused, a fade-out
        // does not hide.
        if let Some(fade) = self.fade.take() {
            fade.pause();
        }
        if shown {
            self.set_opacity(1.0);
        } else {
            self.animate(0.0, 1.0, false);
        }
        if let Some(id) = self.hide.take() {
            id.remove();
        }
        let this = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(SHOWN, move || {
            if let Some(this) = this.upgrade() {
                this.hide.take();
                this.animate(1.0, 0.0, true);
            }
        });
        self.hide.replace(Some(id));
    }

    fn set_opacity(&self, value: f64) {
        for card in self.cards.borrow().iter() {
            card.window.set_opacity(value);
        }
    }

    fn animate(self: &Rc<Self>, from: f64, to: f64, then_hide: bool) {
        let Some(widget) = self.cards.borrow().first().map(|c| c.window.clone()) else {
            return;
        };
        let target = {
            let this = Rc::downgrade(self);
            adw::CallbackAnimationTarget::new(move |v| {
                if let Some(this) = this.upgrade() {
                    this.set_opacity(v);
                }
            })
        };
        let fade = adw::TimedAnimation::new(&widget, from, to, FADE_MS, target);
        if then_hide {
            let this = Rc::downgrade(self);
            fade.connect_done(move |_| {
                if let Some(this) = this.upgrade() {
                    this.fade.take();
                    for card in this.cards.borrow().iter() {
                        card.window.set_visible(false);
                    }
                }
            });
        }
        self.set_opacity(from);
        fade.play();
        self.fade.replace(Some(fade));
    }
}
