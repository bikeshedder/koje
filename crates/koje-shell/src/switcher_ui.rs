//! The Alt+Tab switcher (M8.49): a layer-shell popup on the focused monitor
//! that takes the keyboard while it is open. Each Alt+Tab or Super+Tab (a
//! Hyprland binding, `koje-shell switch`) moves the selection; letting go of
//! the modifier shows the selected app's most recent window, or the selected
//! window.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::{Action, Shell};
use crate::switcher::{SwitchApp, SwitchWindow, first, step};

pub const CSS: &str = "
window.koje-switcher { background: transparent; }
.switcher { background: alpha(#1d1d20, 0.95); color: #ffffff; border-radius: 24px; padding: 18px; }
.switcher-item { padding: 12px; border-radius: 16px; }
.switcher-item.selected { background: alpha(#ffffff, 0.15); }
.switcher-item .context-mark { min-width: 24px; min-height: 4px; border-radius: 9999px; background: var(--accent-bg-color); }
.switcher-name { font-weight: bold; margin-top: 10px; }
";

/// How often the switcher checks whether Alt or Super is still held: its
/// release may come before the popup has the keyboard.
const POLL: Duration = Duration::from_millis(50);

/// The modifiers that keep the switcher open.
fn holding(state: gdk::ModifierType) -> bool {
    state.intersects(
        gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK,
    )
}

/// Apps, or one app's windows (Alt+grave).
#[derive(Debug, Clone)]
enum Items {
    Apps(Vec<SwitchApp>),
    Windows(SwitchApp),
}

impl Items {
    fn len(&self) -> usize {
        match self {
            Self::Apps(a) => a.len(),
            Self::Windows(a) => a.windows.len(),
        }
    }
}

#[derive(Debug)]
struct Open {
    window: gtk::Window,
    items: Items,
    selected: usize,
    tiles: Vec<gtk::Box>,
    name: gtk::Label,
    /// Alt or Super was seen held: from then on its release ends the switch.
    held_seen: bool,
}

#[derive(Debug)]
pub struct Switcher {
    app: adw::Application,
    actions: UnboundedSender<Action>,
    shell: Rc<RefCell<Shell>>,
    open: RefCell<Option<Open>>,
}

impl Switcher {
    pub fn new(
        app: &adw::Application,
        actions: UnboundedSender<Action>,
        shell: Rc<RefCell<Shell>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            app: app.clone(),
            actions,
            shell,
            open: RefCell::new(None),
        })
    }

    /// Alt+Tab (or Alt+grave): open on `monitor`, or move on.
    pub fn next(self: &Rc<Self>, monitor: Option<&gdk::Monitor>, backwards: bool, windows: bool) {
        if self.open.borrow().is_some() {
            if windows {
                self.to_windows();
            }
            self.step(backwards);
            return;
        }
        let shell = self.shell.borrow();
        let workspace = shell.current_workspace();
        let apps = crate::switcher::apps(shell.windows(), &shell.running(), workspace.as_deref());
        drop(shell);
        let items = if windows {
            let Some(app) = apps.into_iter().next() else {
                return;
            };
            Items::Windows(app)
        } else {
            Items::Apps(apps)
        };
        if items.len() == 0 {
            return;
        }
        let selected = first(items.len(), backwards);
        self.show(monitor, items, selected);
    }

    fn show(self: &Rc<Self>, monitor: Option<&gdk::Monitor>, items: Items, selected: usize) {
        let window = gtk::Window::builder()
            .application(&self.app)
            .title("koje switcher")
            .build();
        window.add_css_class("koje-switcher");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-switcher"));
        window.set_monitor(monitor);
        window.set_keyboard_mode(KeyboardMode::Exclusive);

        let row = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();
        let tiles: Vec<gtk::Box> = match &items {
            Items::Apps(apps) => apps.iter().map(app_tile).collect(),
            Items::Windows(app) => app.windows.iter().map(|w| window_tile(app, w)).collect(),
        };
        for (i, tile) in tiles.iter().enumerate() {
            let click = gtk::GestureClick::new();
            let me = Rc::downgrade(self);
            click.connect_released(move |_, _, _, _| {
                if let Some(me) = me.upgrade() {
                    me.select(i);
                    me.activate();
                }
            });
            tile.add_controller(click);
            row.append(tile);
        }
        let name = gtk::Label::builder()
            .css_classes(["switcher-name"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(60)
            .build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["switcher"])
            .build();
        column.append(&row);
        column.append(&name);
        window.set_child(Some(&column));
        self.keys(&window);
        window.present();
        self.open.replace(Some(Open {
            window,
            items,
            selected,
            tiles,
            name,
            held_seen: false,
        }));
        self.select(selected);
        self.watch_alt();
    }

    /// Keys while it is open: Tab, arrows, grave, Enter, Escape, the release of
    /// Alt or Super.
    fn keys(self: &Rc<Self>, window: &gtk::Window) {
        let keys = gtk::EventControllerKey::new();
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(me) = me.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            match key {
                gdk::Key::Escape => me.close(),
                gdk::Key::Return | gdk::Key::KP_Enter => me.activate(),
                gdk::Key::Tab | gdk::Key::Right => me.step(shift),
                gdk::Key::ISO_Left_Tab | gdk::Key::Left => me.step(true),
                gdk::Key::grave | gdk::Key::asciitilde => {
                    me.to_windows();
                    me.step(shift);
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        let me = Rc::downgrade(self);
        keys.connect_key_released(move |_, key, _, _| {
            if matches!(
                key,
                gdk::Key::Alt_L
                    | gdk::Key::Alt_R
                    | gdk::Key::Meta_L
                    | gdk::Key::Meta_R
                    | gdk::Key::Super_L
                    | gdk::Key::Super_R
            ) && let Some(me) = me.upgrade()
            {
                me.activate();
            }
        });
        window.add_controller(keys);
    }

    /// Alt or Super let go before the popup had the keyboard: switch at once.
    fn watch_alt(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        glib::timeout_add_local(POLL, move || {
            let Some(me) = me.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let held = gdk::Display::default()
                .and_then(|d| d.default_seat())
                .and_then(|s| s.keyboard())
                .is_some_and(|k| holding(k.modifier_state()));
            let release = {
                let mut open = me.open.borrow_mut();
                let Some(open) = open.as_mut() else {
                    return glib::ControlFlow::Break;
                };
                if held {
                    open.held_seen = true;
                }
                // Only once the popup has the keyboard is the state its own.
                !held && (open.held_seen || open.window.is_active())
            };
            if release {
                me.activate();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    fn step(&self, backwards: bool) {
        let next = {
            let open = self.open.borrow();
            let Some(open) = open.as_ref() else {
                return;
            };
            step(open.selected, open.items.len(), backwards)
        };
        self.select(next);
    }

    /// From the apps to the selected app's windows (Alt+grave while open): shown
    /// again on its first window; the caller's step moves on.
    fn to_windows(self: &Rc<Self>) {
        let (app, monitor) = {
            let open = self.open.borrow();
            let Some(o) = open.as_ref() else {
                return;
            };
            let Items::Apps(apps) = &o.items else {
                return;
            };
            let Some(app) = apps.get(o.selected).cloned() else {
                return;
            };
            (app, o.window.monitor())
        };
        if let Some(old) = self.open.take() {
            old.window.destroy();
        }
        self.show(monitor.as_ref(), Items::Windows(app), 0);
    }

    fn select(&self, index: usize) {
        let mut open = self.open.borrow_mut();
        let Some(open) = open.as_mut() else {
            return;
        };
        open.selected = index;
        for (i, tile) in open.tiles.iter().enumerate() {
            if i == index {
                tile.add_css_class("selected");
            } else {
                tile.remove_css_class("selected");
            }
        }
        let label = match &open.items {
            Items::Apps(apps) => apps.get(index).map(|a| match &a.context {
                Some(c) => format!("{} – {c}", crate::app_info::name(&a.app)),
                None => crate::app_info::name(&a.app),
            }),
            Items::Windows(app) => app.windows.get(index).map(|w| w.title.clone()),
        };
        open.name.set_label(&label.unwrap_or_default());
    }

    /// Show the selection and close.
    fn activate(&self) {
        let Some(open) = self.open.take() else {
            return;
        };
        open.window.destroy();
        let (window, context) = match &open.items {
            Items::Apps(apps) => {
                let Some(app) = apps.get(open.selected) else {
                    return;
                };
                (app.windows.first().cloned(), app.context.clone())
            }
            Items::Windows(app) => (app.windows.get(open.selected).cloned(), app.context.clone()),
        };
        let Some(window) = window else {
            return;
        };
        if let (true, Some(context)) = (window.stashed, context) {
            let _ = self.actions.send(Action::Unstash {
                context,
                address: window.address.clone(),
            });
        }
        let _ = self.actions.send(Action::FocusWindow(window.address));
    }

    fn close(&self) {
        if let Some(open) = self.open.take() {
            open.window.destroy();
        }
    }
}

fn tile(
    icon: &gtk4::gio::Icon,
    size: i32,
    accent: &crate::model::Accent,
    context: bool,
) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["switcher-item", &accent.css_class()])
        .build();
    let image = gtk::Image::from_gicon(icon);
    image.set_pixel_size(size);
    column.append(&image);
    let mark = gtk::Box::builder()
        .css_classes(["context-mark"])
        .halign(gtk::Align::Center)
        .build();
    mark.set_opacity(if context { 1.0 } else { 0.0 });
    column.append(&mark);
    column
}

fn app_tile(app: &SwitchApp) -> gtk::Box {
    tile(
        &crate::app_info::icon(&app.app),
        96,
        &app.accent,
        app.context.is_some(),
    )
}

fn window_tile(app: &SwitchApp, _w: &SwitchWindow) -> gtk::Box {
    tile(
        &crate::app_info::icon(&app.app),
        64,
        &app.accent,
        app.context.is_some(),
    )
}
