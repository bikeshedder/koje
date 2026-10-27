//! One bar, a clone of GNOME's top bar (ADR 0047): a layer-shell surface
//! along the top of a monitor that reserves its height (exclusive zone),
//! workspaces on the left, the clock in the middle, tray, koje menu and Quick
//! Settings on the right. It renders a [`BarView`] and turns clicks into
//! [`Action`]s.

use std::cell::RefCell;
use std::rc::Rc;

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
use crate::model::{Action, BarView, Shell, WorkspaceButton};
use crate::overview_ui::OverviewSurface;
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
    /// The workspace buttons, kept while only the active one changes – so
    /// the switch animates (M8.80).
    buttons: RefCell<Vec<(String, gtk::Button)>>,
    /// The monitor's overview (ADR 0049), hidden until asked for.
    overview: Rc<OverviewSurface>,
}

impl Bar {
    pub fn new(
        app: &adw::Application,
        monitor: &gdk::Monitor,
        actions: UnboundedSender<Action>,
        shell: Rc<RefCell<Shell>>,
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
        let overview = OverviewSurface::new(app, monitor, actions.clone(), shell);

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
            buttons: RefCell::default(),
            overview,
        }
    }

    pub fn overview(&self) -> &Rc<OverviewSurface> {
        &self.overview
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
        let same = self
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|old| same_buttons(old, view));
        if same {
            // Only the active one moved: the CSS transitions animate it.
            for ((_, button), w) in self.buttons.borrow().iter().zip(&view.workspaces) {
                if w.active {
                    button.add_css_class("active");
                } else {
                    button.remove_css_class("active");
                }
            }
        } else {
            self.render_workspaces(view);
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

    fn render_workspaces(&self, view: &BarView) {
        while let Some(child) = self.workspaces.first_child() {
            self.workspaces.remove(&child);
        }
        let mut buttons = Vec::new();
        match view.style {
            WorkspaceStyle::Names => {
                for w in &view.workspaces {
                    let b = self.name_button(w);
                    self.workspaces.append(&b);
                    buttons.push((w.name.clone(), b));
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
                    let b = self.dot(w, colored);
                    indicator.append(&b);
                    buttons.push((w.name.clone(), b));
                }
                self.workspaces.append(&indicator);
            }
        }
        self.buttons.replace(buttons);
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
        self.overview.close();
        self.window.destroy();
    }
}

/// The same workspace buttons but for which one is active: they are kept.
fn same_buttons(old: &BarView, new: &BarView) -> bool {
    let key = |w: &WorkspaceButton| (w.name.clone(), w.accent.clone(), w.empty);
    old.style == new.style
        && old
            .workspaces
            .iter()
            .map(key)
            .eq(new.workspaces.iter().map(key))
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
