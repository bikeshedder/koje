//! The background (M8.82) and GNOME's background menu (M8.83): a layer-shell
//! surface per monitor on the `background` layer showing what GNOME's
//! settings say ([`background::spec`]), composed off the GTK main loop; a
//! right click opens GNOME's menu – Change Background…, Display Settings,
//! Settings – which opens GNOME Settings' panels, so a picture chosen there
//! comes back through the settings.

use std::cell::Cell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;

use crate::background::{self, Settings, Spec};

const BACKGROUND: &str = "org.gnome.desktop.background";
const INTERFACE: &str = "org.gnome.desktop.interface";

/// GNOME Shell's background menu (`backgroundMenu.js`): its entries and the
/// desktop files they open.
const MENU: [(&str, &str, &str); 3] = [
    (
        "change",
        "Change Background…",
        "gnome-background-panel.desktop",
    ),
    ("display", "Display Settings", "gnome-display-panel.desktop"),
    ("settings", "Settings", "org.gnome.Settings.desktop"),
];

fn settings(schema: &str) -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(schema, true)?;
    Some(gio::Settings::new(schema))
}

/// What GNOME's settings say now; without the schema GNOME's default colors.
pub fn current() -> Spec {
    let dark = settings(INTERFACE).is_some_and(|s| s.string("color-scheme") == "prefer-dark");
    let Some(s) = settings(BACKGROUND) else {
        return background::spec(&Settings {
            dark,
            options: "none".into(),
            ..Settings::default()
        });
    };
    background::spec(&Settings {
        uri: s.string("picture-uri").into(),
        uri_dark: s.string("picture-uri-dark").into(),
        dark,
        options: s.string("picture-options").into(),
        primary: s.string("primary-color").into(),
        secondary: s.string("secondary-color").into(),
        shading: s.string("color-shading-type").into(),
        opacity: s.int("picture-opacity"),
    })
}

/// Call `changed` whenever GNOME's background or the style changes.
pub fn follow(changed: impl Fn() + 'static) {
    let changed = Rc::new(changed);
    for (schema, key) in [(BACKGROUND, None), (INTERFACE, Some("color-scheme"))] {
        if let Some(s) = settings(schema) {
            let changed = changed.clone();
            s.connect_changed(key, move |_, _| changed());
            // Follows as long as the shell runs.
            std::mem::forget(s);
        }
    }
}

#[derive(Debug)]
pub struct Background {
    window: gtk::Window,
    picture: gtk::Picture,
    monitor: gdk::Monitor,
    /// The newest composition wins when several are under way.
    generation: Cell<u64>,
}

impl Background {
    pub fn new(app: &adw::Application, monitor: &gdk::Monitor) -> Rc<Self> {
        let window = gtk::Window::builder()
            .application(app)
            .title("koje background")
            .css_classes(["koje-background"])
            .build();
        window.init_layer_shell();
        window.set_layer(Layer::Background);
        window.set_namespace(Some("koje-background"));
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::None);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        // Under the bar too, as GNOME's.
        window.set_exclusive_zone(-1);
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Fill)
            .can_shrink(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        window.set_child(Some(&picture));
        menu(&window, &picture);
        window.present();
        let me = Rc::new(Self {
            window,
            picture,
            monitor: monitor.clone(),
            generation: Cell::new(0),
        });
        me.show(current());
        me
    }

    /// Compose `spec` for this monitor's size in pixels, then show it.
    pub fn show(self: &Rc<Self>, spec: Spec) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let geometry = self.monitor.geometry();
        let scale = self.monitor.scale();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "pixels"
        )]
        let (w, h) = (
            (f64::from(geometry.width()) * scale).round().max(1.0) as usize,
            (f64::from(geometry.height()) * scale).round().max(1.0) as usize,
        );
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let composed = gio::spawn_blocking(move || compose(&spec, w, h)).await;
            let Some(this) = this.upgrade() else { return };
            if this.generation.get() != generation {
                return;
            }
            let Ok(Some((bytes, w, h))) = composed else {
                tracing::warn!("composing the background failed");
                return;
            };
            let texture = gdk::MemoryTexture::new(
                i32::try_from(w).unwrap_or(i32::MAX),
                i32::try_from(h).unwrap_or(i32::MAX),
                gdk::MemoryFormat::R8g8b8,
                &bytes,
                w * 3,
            );
            this.picture.set_paintable(Some(&texture));
        });
    }

    pub fn close(&self) {
        self.window.destroy();
    }
}

/// The colors, then the picture (a slideshow's first) placed as GNOME's
/// `picture-options` say, as RGB bytes `w` × `h`. A picture that cannot be
/// read leaves the colors.
fn compose(spec: &Spec, w: usize, h: usize) -> Option<(glib::Bytes, usize, usize)> {
    let (wi, hi) = (i32::try_from(w).ok()?, i32::try_from(h).ok()?);
    let fill = glib::Bytes::from_owned(background::fill(spec, w, h));
    let dest = gdk_pixbuf::Pixbuf::from_bytes(
        &fill,
        gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        wi,
        hi,
        wi * 3,
    );
    // `from_bytes` is read-only: draw into a copy.
    let dest = dest.copy()?;
    if let Some(path) = &spec.picture {
        let slideshow = std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"));
        let path = if slideshow {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|x| background::slideshow_first(&x))
        } else {
            Some(path.clone())
        };
        match path.map(|p| gdk_pixbuf::Pixbuf::from_file(&p)) {
            Some(Ok(image)) => {
                let alpha = i32::from(spec.opacity) * 255 / 100;
                let size = (f64::from(image.width()), f64::from(image.height()));
                #[allow(clippy::cast_precision_loss, reason = "screen sizes")]
                let screen = (w as f64, h as f64);
                for r in background::place(spec.fit, size, screen) {
                    #[allow(clippy::cast_possible_truncation, reason = "pixels on screen")]
                    let (x0, y0, x1, y1) = (
                        r.x.max(0.0) as i32,
                        r.y.max(0.0) as i32,
                        (r.x + r.w).min(screen.0) as i32,
                        (r.y + r.h).min(screen.1) as i32,
                    );
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    image.composite(
                        &dest,
                        x0,
                        y0,
                        x1 - x0,
                        y1 - y0,
                        r.x,
                        r.y,
                        r.w / size.0,
                        r.h / size.1,
                        gdk_pixbuf::InterpType::Bilinear,
                        alpha,
                    );
                }
            }
            Some(Err(e)) => tracing::warn!("background picture: {e}"),
            None => tracing::warn!("background: no picture in {path:?}", path = spec.picture),
        }
    }
    // Rows without padding, as the texture expects.
    let stride = usize::try_from(dest.rowstride()).ok()?;
    let bytes = dest.read_pixel_bytes();
    if stride == w * 3 {
        return Some((bytes, w, h));
    }
    let mut rows = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        rows.extend_from_slice(bytes.get(y * stride..y * stride + w * 3)?);
    }
    Some((glib::Bytes::from_owned(rows), w, h))
}

/// GNOME's background menu on a right click, where it was clicked.
fn menu(window: &gtk::Window, picture: &gtk::Picture) {
    let model = gio::Menu::new();
    let group = gio::SimpleActionGroup::new();
    for (i, (name, label, desktop)) in MENU.iter().enumerate() {
        // GNOME separates Settings from the two above.
        if i == 2 {
            let section = gio::Menu::new();
            section.append(Some(label), Some(&format!("background.{name}")));
            model.append_section(None, &section);
        } else {
            model.append(Some(label), Some(&format!("background.{name}")));
        }
        let action = gio::SimpleAction::new(name, None);
        match gio_unix::DesktopAppInfo::new(desktop) {
            Some(app) => {
                let window = window.clone();
                action.connect_activate(move |_, _| {
                    crate::quick_ui::launch_gnome_settings(&app, &WidgetExt::display(&window));
                });
            }
            None => action.set_enabled(false),
        }
        group.add_action(&action);
    }
    window.insert_action_group("background", Some(&group));
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    popover.set_parent(picture);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    click.connect_pressed(move |_, _, x, y| {
        #[allow(clippy::cast_possible_truncation, reason = "pixels")]
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    });
    picture.add_controller(click);
}
