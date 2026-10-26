//! One bar, a clone of GNOME's top bar (ADR 0047): a layer-shell surface
//! along the top of a monitor that reserves its height (exclusive zone),
//! workspaces on the left, the clock in the middle, tray, koje menu and Quick
//! Settings on the right. It renders a [`BarView`] and turns clicks into
//! [`Action`]s.

use std::cell::RefCell;

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell as _};
use koje_core::config::WorkspaceStyle;
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::bar_layout::bar_box;
use crate::clock::Clock;
use crate::menu_ui::MenuWidget;
use crate::model::{Action, BarView, WorkspaceButton};
use crate::quick_ui::QuickWidget;

#[derive(Debug)]
pub struct Bar {
    monitor: gdk::Monitor,
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    workspaces: gtk::Box,
    menu: MenuWidget,
    quick: QuickWidget,
    warning: gtk::Box,
    warning_text: gtk::Label,
    /// What the bar shows now: an update that changes nothing redraws nothing.
    shown: RefCell<Option<BarView>>,
}

impl Bar {
    pub fn new(
        app: &adw::Application,
        monitor: &gdk::Monitor,
        actions: UnboundedSender<Action>,
    ) -> Self {
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

        let workspaces = gtk::Box::builder()
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Start)
            .build();

        let menu = MenuWidget::new(&actions);
        let warning_text = gtk::Label::new(None);
        let warning = gtk::Box::builder()
            .spacing(6)
            .css_classes(["warning"])
            .visible(false)
            .build();
        warning.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        warning.append(&warning_text);
        let end = gtk::Box::builder().halign(gtk::Align::End).build();
        end.append(&warning);
        end.append(menu.widget());
        let quick = QuickWidget::new(&actions);
        end.append(quick.widget());

        let clock = Clock::new();
        let layout = bar_box(&workspaces, clock.widget(), &end);
        window.set_child(Some(&layout));
        window.present();

        Self {
            monitor: monitor.clone(),
            window,
            actions,
            workspaces,
            menu,
            quick,
            warning,
            warning_text,
            shown: RefCell::new(None),
        }
    }

    pub fn monitor(&self) -> &gdk::Monitor {
        &self.monitor
    }

    /// The monitor's connector (`eDP-1`), as Hyprland names it.
    pub fn connector(&self) -> Option<String> {
        self.monitor.connector().map(String::from)
    }

    pub fn render(&self, view: &BarView) {
        if self.shown.borrow().as_ref() == Some(view) {
            return;
        }
        while let Some(child) = self.workspaces.first_child() {
            self.workspaces.remove(&child);
        }
        match view.style {
            WorkspaceStyle::Names => {
                for w in &view.workspaces {
                    self.workspaces.append(&self.name_button(w));
                }
            }
            WorkspaceStyle::Dots | WorkspaceStyle::ColoredDots => {
                let colored = view.style == WorkspaceStyle::ColoredDots;
                // GNOME's workspace indicator: one pill holding the dots.
                let indicator = gtk::Box::builder()
                    .css_classes(["panel-button", "workspace-indicator"])
                    .spacing(6)
                    .build();
                for w in &view.workspaces {
                    indicator.append(&self.dot(w, colored));
                }
                self.workspaces.append(&indicator);
            }
        }
        self.menu.render(view.menu.as_ref());
        self.quick.render(&view.quick);
        match view.warning {
            Some(text) => {
                self.warning_text.set_label(text);
                self.warning.set_visible(true);
            }
            None => self.warning.set_visible(false),
        }
        self.shown.replace(Some(view.clone()));
    }

    /// What a click on a workspace does: an empty context's workspace through
    /// kojed, like `koje focus`.
    fn on_click(&self, button: &gtk::Button, w: &WorkspaceButton) {
        let action = if w.empty {
            Action::FocusContext(w.name.clone())
        } else {
            Action::FocusWorkspace(w.name.clone())
        };
        let actions = self.actions.clone();
        button.connect_clicked(move |_| {
            let _ = actions.send(action.clone());
        });
    }

    fn name_button(&self, w: &WorkspaceButton) -> gtk::Button {
        let button = gtk::Button::builder()
            .label(&w.name)
            .css_classes(["panel-button", "workspace-name", &w.accent.css_class()])
            .build();
        mark(&button, w);
        self.on_click(&button, w);
        button
    }

    fn dot(&self, w: &WorkspaceButton, colored: bool) -> gtk::Button {
        let button = gtk::Button::builder()
            .css_classes(["workspace-dot"])
            .valign(gtk::Align::Center)
            .tooltip_text(&w.name)
            .build();
        if colored {
            button.add_css_class("colored");
            button.add_css_class(&w.accent.css_class());
        }
        mark(&button, w);
        self.on_click(&button, w);
        button
    }

    pub fn close(&self) {
        self.window.destroy();
    }
}

/// `active` and `empty` for the CSS; a hint on an empty context.
fn mark(button: &gtk::Button, w: &WorkspaceButton) {
    if w.active {
        button.add_css_class("active");
    }
    if w.empty {
        button.add_css_class("empty");
        button.set_tooltip_text(Some(&format!("{} – no windows", w.name)));
    }
}
