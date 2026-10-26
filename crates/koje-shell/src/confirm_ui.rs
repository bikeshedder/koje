//! "Keep these display settings?" (M8.53): after GNOME Settings changed the
//! screens, the shell asks as GNOME Shell does; without an answer kojed
//! reverts on its own (ADR 0046). A layer-shell surface above everything,
//! with the keyboard: Enter keeps, Escape reverts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell as _};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::Action;

/// The countdown, as GNOME words it.
pub fn countdown_text(seconds: u32) -> String {
    if seconds == 1 {
        "Settings changes will revert in 1 second".to_owned()
    } else {
        format!("Settings changes will revert in {seconds} seconds")
    }
}

#[derive(Debug)]
pub struct DisplayConfirm {
    window: gtk::Window,
    body: gtk::Label,
    left: Rc<Cell<u32>>,
    /// The running countdown; a new change starts it again.
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl DisplayConfirm {
    pub fn new(app: &adw::Application, actions: &UnboundedSender<Action>) -> Rc<Self> {
        let window = gtk::Window::builder()
            .application(app)
            .title("Keep these display settings?")
            .build();
        window.add_css_class("display-confirm");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-dialog"));
        window.set_keyboard_mode(KeyboardMode::Exclusive);

        let title = gtk::Label::builder()
            .label("Keep these display settings?")
            .css_classes(["title-2"])
            .build();
        let body = gtk::Label::builder().wrap(true).build();
        let revert = gtk::Button::builder()
            .label("Revert Settings")
            .css_classes(["pill"])
            .hexpand(true)
            .build();
        let keep = gtk::Button::builder()
            .label("Keep Changes")
            .css_classes(["pill", "suggested-action"])
            .hexpand(true)
            .build();
        let buttons = gtk::Box::builder().spacing(12).homogeneous(true).build();
        buttons.append(&revert);
        buttons.append(&keep);
        // A rounded card with a shadow on a clear window, as GNOME's dialogs.
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .width_request(380)
            .css_classes(["dialog-card"])
            .build();
        content.append(&title);
        content.append(&body);
        content.append(&buttons);
        window.set_child(Some(&content));
        window.set_default_widget(Some(&keep));

        let me = Rc::new(Self {
            window,
            body,
            left: Rc::default(),
            timer: Rc::default(),
        });
        let answer = |action: Action| {
            let (me, actions) = (Rc::downgrade(&me), actions.clone());
            move || {
                let _ = actions.send(action.clone());
                if let Some(me) = me.upgrade() {
                    me.close();
                }
            }
        };
        let keep_now = answer(Action::KeepDisplayChange);
        let revert_now = answer(Action::RevertDisplayChange);
        keep.connect_clicked({
            let f = keep_now.clone();
            move |_| f()
        });
        revert.connect_clicked({
            let f = revert_now.clone();
            move |_| f()
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                revert_now();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        me.window.add_controller(keys);
        me
    }

    /// Ask, counting down from `seconds`.
    pub fn show(self: &Rc<Self>, seconds: u32) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        self.left.set(seconds);
        self.body.set_label(&countdown_text(seconds));
        self.window.present();
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(me) = me.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let left = me.left.get().saturating_sub(1);
            me.left.set(left);
            if left == 0 {
                // kojed reverts by itself now.
                me.timer.borrow_mut().take();
                me.window.set_visible(false);
                return glib::ControlFlow::Break;
            }
            me.body.set_label(&countdown_text(left));
            glib::ControlFlow::Continue
        });
        self.timer.replace(Some(id));
    }

    fn close(&self) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        self.window.set_visible(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_like_gnome() {
        assert_eq!(
            countdown_text(20),
            "Settings changes will revert in 20 seconds"
        );
        assert_eq!(
            countdown_text(1),
            "Settings changes will revert in 1 second"
        );
    }
}
