//! One bar, a clone of GNOME's top bar (ADR 0047): a layer-shell surface
//! along the top of a monitor that reserves its height (exclusive zone),
//! workspaces and the current context's stash on the left, the clock in the
//! middle, tray, koje menu and Quick Settings on the right. It renders a [`BarView`] and turns clicks into
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

use crate::background_ui::Background;
use crate::bar_layout::bar_box;
use crate::clock::Clock;
use crate::menu_ui::MenuWidget;
use crate::model::{Accent, Action, BarView, OverlayButton, Shell, WorkspaceButton};
use crate::overview::Stash;
use crate::overview_ui::OverviewSurface;
use crate::quick_ui::QuickWidget;

#[derive(Debug)]
pub struct Bar {
    monitor: gdk::Monitor,
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    workspaces: gtk::Box,
    stash: StashButton,
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
    /// The monitor's background (M8.82).
    background: Rc<Background>,
    clock: Rc<Clock>,
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

        let stash = StashButton::new(&actions);
        let start = gtk::Box::builder()
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Start)
            .build();
        start.append(&workspaces);
        start.append(stash.widget());

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

        let clock = Clock::new(&actions);
        let layout = bar_box(&start, clock.widget(), &end);
        window.set_child(Some(&layout));
        window.present();
        let overview = OverviewSurface::new(app, monitor, actions.clone(), shell);
        let background = Background::new(app, monitor);

        Self {
            monitor: monitor.clone(),
            window,
            actions,
            workspaces,
            stash,
            menu,
            quick,
            warning,
            warning_text,
            shown: RefCell::new(None),
            buttons: RefCell::default(),
            overview,
            background,
            clock,
        }
    }

    pub fn overview(&self) -> &Rc<OverviewSurface> {
        &self.overview
    }

    pub fn background(&self) -> &Rc<Background> {
        &self.background
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
            let active = view
                .workspaces
                .iter()
                .map(|w| w.active)
                .chain(view.overlays.iter().map(|o| o.open));
            for ((_, button), on) in self.buttons.borrow().iter().zip(active) {
                if on {
                    button.add_css_class("active");
                } else {
                    button.remove_css_class("active");
                }
            }
        } else {
            self.render_workspaces(view);
        }
        self.stash.render(view.stash.as_ref());
        self.menu.render(view.menu.as_ref());
        self.clock.render(&view.date);
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
            WorkspaceStyle::Labels => {
                // Icon and label per workspace, the overlays the same after
                // a gap (ADR 0055).
                let group = gtk::Box::builder()
                    .css_classes(["workspace-labels"])
                    .spacing(2)
                    .build();
                for w in &view.workspaces {
                    let b = self.label_button(w);
                    group.append(&b);
                    buttons.push((w.name.clone(), b));
                }
                if !view.overlays.is_empty() {
                    group.append(&gtk::Box::builder().css_classes(["overlay-gap"]).build());
                }
                for o in &view.overlays {
                    let b = self.overlay_button(o, Shape::Label);
                    group.append(&b);
                    buttons.push((o.name.clone(), b));
                }
                self.workspaces.append(&group);
            }
            WorkspaceStyle::Names => {
                for w in &view.workspaces {
                    let b = self.name_button(w);
                    self.workspaces.append(&b);
                    buttons.push((w.name.clone(), b));
                }
                for o in &view.overlays {
                    let b = self.overlay_button(o, Shape::Name);
                    self.workspaces.append(&b);
                    buttons.push((o.name.clone(), b));
                }
            }
            WorkspaceStyle::Dots | WorkspaceStyle::ColoredDots => {
                let colored = view.style == WorkspaceStyle::ColoredDots;
                // GNOME's workspace indicator: one pill holding the dots.
                let indicator = gtk::Box::builder()
                    .css_classes(["panel-button", "workspace-indicator"])
                    .spacing(5)
                    .build();
                // GNOME's pill is wider with fewer workspaces (panel.js).
                indicator.add_css_class(dot_width_class(view.workspaces.len()));
                for w in &view.workspaces {
                    let b = self.dot(w, colored);
                    indicator.append(&b);
                    buttons.push((w.name.clone(), b));
                }
                // The overlays after a gap, drawn like workspaces (M8.34).
                if !view.overlays.is_empty() {
                    indicator.append(&gtk::Box::builder().css_classes(["overlay-gap"]).build());
                }
                for o in &view.overlays {
                    let b = self.overlay_button(o, Shape::Dot);
                    indicator.append(&b);
                    buttons.push((o.name.clone(), b));
                }
                self.workspaces.append(&indicator);
            }
        }
        self.buttons.replace(buttons);
    }

    /// What a click on a workspace does: an empty context's workspace through
    /// kojed, like `koje focus`.
    fn on_click(&self, button: &gtk::Button, w: &WorkspaceButton) {
        let action = w.focus();
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

    /// The workspace's icon and label; a context's icon in its color, and
    /// its color while it is active.
    fn label_button(&self, w: &WorkspaceButton) -> gtk::Button {
        let button = gtk::Button::builder()
            .child(&icon_label(&w.icon, &w.label))
            .css_classes(["panel-button", "workspace-label", &w.accent.css_class()])
            .tooltip_text(&w.label)
            .build();
        mark(&button, w);
        self.on_click(&button, w);
        button
    }

    fn dot(&self, w: &WorkspaceButton, colored: bool) -> gtk::Button {
        let button = gtk::Button::builder()
            .css_classes(["workspace-dot"])
            .valign(gtk::Align::Center)
            .tooltip_text(&w.label)
            .build();
        if colored {
            button.add_css_class("colored");
            button.add_css_class(&w.accent.css_class());
        }
        mark(&button, w);
        self.on_click(&button, w);
        button
    }

    /// An overlay: a dot, its name or its icon, active while it is open; a
    /// click shows or hides it.
    fn overlay_button(&self, o: &OverlayButton, shape: Shape) -> gtk::Button {
        let button = match shape {
            Shape::Dot => gtk::Button::builder()
                .css_classes(["workspace-dot", "overlay"])
                .valign(gtk::Align::Center)
                .tooltip_text(&o.label)
                .build(),
            Shape::Name => gtk::Button::builder()
                .label(&o.label)
                .css_classes(["panel-button", "workspace-name", "overlay"])
                .build(),
            Shape::Label => gtk::Button::builder()
                .child(&icon_label(&o.icon, &o.label))
                .css_classes(["panel-button", "workspace-label", "overlay"])
                .tooltip_text(&o.label)
                .build(),
        };
        if o.open {
            button.add_css_class("active");
        }
        let (actions, name) = (self.actions.clone(), o.name.clone());
        button.connect_clicked(move |_| {
            let _ = actions.send(Action::ToggleOverlay(name.clone()));
        });
        button
    }

    pub fn close(&self) {
        self.overview.close();
        self.background.close();
        self.window.destroy();
    }
}

/// The current context's stash (M8.101): how many windows it holds; a click
/// lists them, a click on one brings it back. Hidden while it is empty.
#[derive(Debug)]
struct StashButton {
    button: gtk::MenuButton,
    count: gtk::Label,
    list: gtk::Box,
    actions: UnboundedSender<Action>,
    shown: RefCell<Option<Stash>>,
}

impl StashButton {
    fn new(actions: &UnboundedSender<Action>) -> Self {
        let count = gtk::Label::new(None);
        let face = gtk::Box::builder().spacing(6).build();
        face.append(&gtk::Image::from_icon_name("view-conceal-symbolic"));
        face.append(&count);
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["stash-list"])
            .build();
        let popover = gtk::Popover::builder().child(&list).build();
        crate::popup::keep_inside(&popover);
        let button = gtk::MenuButton::builder()
            .child(&face)
            .popover(&popover)
            .css_classes(["panel-button", "stash-button"])
            .visible(false)
            .build();
        Self {
            button,
            count,
            list,
            actions: actions.clone(),
            shown: RefCell::new(None),
        }
    }

    fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }

    fn render(&self, stash: Option<&Stash>) {
        if self.shown.borrow().as_ref() == stash {
            return;
        }
        // The context's color on its icon.
        if let Some(old) = self.shown.replace(stash.cloned()) {
            self.button
                .remove_css_class(&Accent::Context(old.context).css_class());
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let Some(stash) = stash else {
            if let Some(p) = self.button.popover() {
                p.popdown();
            }
            self.button.set_visible(false);
            return;
        };
        self.button
            .add_css_class(&Accent::Context(stash.context.clone()).css_class());
        let n = stash.windows.len();
        self.count.set_label(&n.to_string());
        self.button.set_tooltip_text(Some(&format!(
            "Stash of {}: {n} hidden window{}",
            stash.context,
            if n == 1 { "" } else { "s" }
        )));
        for w in &stash.windows {
            let row = gtk::Box::builder().spacing(8).build();
            let icon = gtk::Image::from_gicon(&crate::app_info::icon(&w.app));
            icon.set_pixel_size(16);
            row.append(&icon);
            row.append(
                &gtk::Label::builder()
                    .label(&w.title)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(40)
                    .build(),
            );
            let item = gtk::Button::builder()
                .child(&row)
                .css_classes(["flat", "stash-item"])
                .tooltip_text(format!("Bring back: {}", w.title))
                .build();
            let (actions, button) = (self.actions.clone(), self.button.clone());
            let unstash = Action::Unstash {
                context: stash.context.clone(),
                address: w.address.clone(),
            };
            let focus = Action::FocusWindow(w.address.clone());
            item.connect_clicked(move |_| {
                let _ = actions.send(unstash.clone());
                let _ = actions.send(focus.clone());
                button.popdown();
            });
            self.list.append(&item);
        }
        self.button.set_visible(true);
    }
}

/// How an overlay is drawn in the bar.
#[derive(Clone, Copy)]
enum Shape {
    Dot,
    Name,
    Label,
}

/// GNOME's active dot is 8 px × 3.625 with up to two workspaces, × 3.25 up
/// to five, × 2.75 above (`panel.js`, `WorkspaceIndicators`); inactive dots
/// are 75 % of 8 px at half opacity.
fn dot_width_class(workspaces: usize) -> &'static str {
    match workspaces {
        0..=2 => "few",
        3..=5 => "some",
        _ => "many",
    }
}

/// The same workspace buttons but for which one is active: they are kept.
fn same_buttons(old: &BarView, new: &BarView) -> bool {
    let key = |w: &WorkspaceButton| {
        (
            w.name.clone(),
            w.label.clone(),
            w.icon.clone(),
            w.accent.clone(),
            w.empty,
        )
    };
    let overlay = |o: &OverlayButton| (o.name.clone(), o.label.clone(), o.icon.clone());
    old.style == new.style
        && old
            .workspaces
            .iter()
            .map(key)
            .eq(new.workspaces.iter().map(key))
        && old
            .overlays
            .iter()
            .map(overlay)
            .eq(new.overlays.iter().map(overlay))
}

/// `active` and `empty` for the CSS; a hint on an empty context.
/// A workspace's or an overlay's icon and label, as the bar and the
/// overview's strip show them (ADR 0055).
pub fn icon_label(icon: &str, label: &str) -> gtk::Box {
    let content = gtk::Box::builder().spacing(6).build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    content
}

fn mark(button: &gtk::Button, w: &WorkspaceButton) {
    if w.active {
        button.add_css_class("active");
    }
    if w.empty {
        button.add_css_class("empty");
        button.set_tooltip_text(Some(&format!("{} – no windows", w.label)));
    }
}
