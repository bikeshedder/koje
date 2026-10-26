//! One bar: a layer-shell surface along the top of a monitor that reserves
//! its height (exclusive zone). It only renders a [`BarView`].

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell as _};
use libadwaita as adw;

use crate::model::BarView;

#[derive(Debug)]
pub struct Bar {
    monitor: gdk::Monitor,
    window: gtk::Window,
    workspace: gtk::Label,
    context: gtk::Label,
    warning: gtk::Box,
    warning_text: gtk::Label,
}

impl Bar {
    pub fn new(app: &adw::Application, monitor: &gdk::Monitor) -> Self {
        let window = gtk::Window::builder()
            .application(app)
            .title("koje bar")
            .build();
        // Added, not set: the window keeps libadwaita's `background` class.
        window.add_css_class("koje-bar");
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_namespace(Some("koje-bar"));
        window.set_monitor(Some(monitor));
        for edge in [Edge::Top, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.auto_exclusive_zone_enable();

        let workspace = gtk::Label::builder().css_classes(["heading"]).build();
        let context = gtk::Label::new(None);
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        start.append(&workspace);
        start.append(&context);

        let warning_text = gtk::Label::new(None);
        let warning = gtk::Box::builder()
            .spacing(6)
            .css_classes(["warning"])
            .visible(false)
            .build();
        warning.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        warning.append(&warning_text);

        let layout = gtk::CenterBox::builder()
            .start_widget(&start)
            .end_widget(&warning)
            .margin_start(12)
            .margin_end(12)
            .build();
        window.set_child(Some(&layout));
        window.present();

        Self {
            monitor: monitor.clone(),
            window,
            workspace,
            context,
            warning,
            warning_text,
        }
    }

    pub fn monitor(&self) -> &gdk::Monitor {
        &self.monitor
    }

    pub fn render(&self, view: &BarView) {
        self.workspace.set_label(&view.workspace);
        match &view.context {
            Some((name, _color)) => {
                self.context.set_label(name);
                self.context.set_visible(true);
            }
            None => self.context.set_visible(false),
        }
        match view.warning {
            Some(text) => {
                self.warning_text.set_label(text);
                self.warning.set_visible(true);
            }
            None => self.warning.set_visible(false),
        }
    }

    pub fn close(&self) {
        self.window.destroy();
    }
}
