//! One bar: a layer-shell surface along the top of a monitor that reserves
//! its height (exclusive zone). It renders a [`BarView`] and turns clicks
//! into [`Action`]s.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio};
use gtk4_layer_shell::{Edge, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::menu_ui::MenuWidget;
use crate::model::{Accent, Action, BarView, Task, WorkspaceButton};

/// Widest a task's title gets, in characters.
const TITLE_CHARS: i32 = 24;

#[derive(Debug)]
pub struct Bar {
    monitor: gdk::Monitor,
    window: gtk::Window,
    actions: UnboundedSender<Action>,
    workspaces: gtk::Box,
    tasks: gtk::Box,
    menu: MenuWidget,
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
            .spacing(4)
            .css_classes(["workspaces"])
            .build();
        let tasks = gtk::Box::builder()
            .spacing(2)
            .css_classes(["tasks"])
            .build();
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        start.append(&workspaces);
        start.append(&tasks);

        let menu = MenuWidget::new(&actions);
        let warning_text = gtk::Label::new(None);
        let warning = gtk::Box::builder()
            .spacing(6)
            .css_classes(["warning"])
            .visible(false)
            .build();
        warning.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        warning.append(&warning_text);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        end.append(menu.widget());
        end.append(&warning);

        let layout = gtk::CenterBox::builder()
            .start_widget(&start)
            .end_widget(&end)
            .margin_start(6)
            .margin_end(12)
            .build();
        window.set_child(Some(&layout));
        window.present();

        Self {
            monitor: monitor.clone(),
            window,
            actions,
            workspaces,
            tasks,
            menu,
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
        clear(&self.workspaces);
        for w in &view.workspaces {
            self.workspaces.append(&self.workspace_button(w));
        }
        clear(&self.tasks);
        set_accent(&self.tasks, &view.tasks_accent);
        for t in &view.tasks {
            self.tasks.append(&self.task_button(t));
        }
        self.menu.render(view.menu.as_ref());
        match view.warning {
            Some(text) => {
                self.warning_text.set_label(text);
                self.warning.set_visible(true);
            }
            None => self.warning.set_visible(false),
        }
        self.shown.replace(Some(view.clone()));
    }

    fn workspace_button(&self, w: &WorkspaceButton) -> gtk::Button {
        let button = gtk::Button::builder()
            .label(&w.name)
            .css_classes(["flat", &w.accent.css_class()])
            .build();
        if w.active {
            button.add_css_class("active");
        }
        if w.empty {
            button.add_css_class("empty");
            button.set_tooltip_text(Some(&format!("{} – no windows", w.name)));
        }
        // An empty context's workspace through kojed, like `koje focus`.
        let action = if w.empty {
            Action::FocusContext(w.name.clone())
        } else {
            Action::FocusWorkspace(w.name.clone())
        };
        let actions = self.actions.clone();
        button.connect_clicked(move |_| {
            let _ = actions.send(action.clone());
        });
        button
    }

    fn task_button(&self, t: &Task) -> gtk::Button {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&gtk::Image::from_gicon(&icon(&t.class)));
        content.append(
            &gtk::Label::builder()
                .label(&t.title)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .max_width_chars(TITLE_CHARS)
                .build(),
        );
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text(&t.title)
            .css_classes(["flat"])
            .build();
        if t.focused {
            button.add_css_class("focused");
        }
        let (actions, address) = (self.actions.clone(), t.address.clone());
        button.connect_clicked(move |_| {
            let _ = actions.send(Action::FocusWindow(address.clone()));
        });
        button
    }

    pub fn close(&self) {
        self.window.destroy();
    }
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Give `widget` the accent class of `accent`, replacing any earlier one.
fn set_accent(widget: &impl IsA<gtk::Widget>, accent: &Accent) {
    for class in widget.css_classes() {
        if class.starts_with("koje-ctx-") || class == "koje-host" {
            widget.remove_css_class(&class);
        }
    }
    widget.add_css_class(&accent.css_class());
}

thread_local! {
    static ICONS: RefCell<HashMap<String, gio::Icon>> = RefCell::default();
}

/// A window's icon from its app id: the `.desktop` entry's, an icon of that
/// name, else a generic one. Programs in contexts have their entries in the
/// container (M8.18), so they often get the generic one.
fn icon(class: &str) -> gio::Icon {
    ICONS.with(|cache| {
        if let Some(icon) = cache.borrow().get(class) {
            return icon.clone();
        }
        let icon = lookup(class);
        cache.borrow_mut().insert(class.to_owned(), icon.clone());
        icon
    })
}

fn lookup(class: &str) -> gio::Icon {
    let from_entry = [class.to_owned(), class.to_lowercase()]
        .iter()
        .find_map(|id| gio_unix::DesktopAppInfo::new(&format!("{id}.desktop")))
        .and_then(|info| info.icon());
    if let Some(icon) = from_entry {
        return icon;
    }
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    let name = class.to_lowercase();
    if !name.is_empty() && theme.is_some_and(|t| t.has_icon(&name)) {
        return gio::ThemedIcon::new(&name).upcast();
    }
    gio::ThemedIcon::new("application-x-executable-symbolic").upcast()
}
