//! A GNOME-style question with a countdown (M8.47, M8.53): a card above
//! everything on a layer-shell surface with the keyboard – Enter confirms,
//! Escape cancels – and what happens when the time is up.

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

/// One question.
pub struct Question {
    pub title: String,
    /// The text for the seconds left.
    pub text: Box<dyn Fn(u32) -> String>,
    pub cancel_label: &'static str,
    /// Sent on Cancel or Escape.
    pub on_cancel: Option<Action>,
    pub confirm_label: &'static str,
    /// Red instead of blue: the confirm button ends something.
    pub destructive: bool,
    pub on_confirm: Action,
    /// Sent when the time is up; `None`: just close.
    pub on_timeout: Option<Action>,
}

impl std::fmt::Debug for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Question")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct CountdownDialog {
    window: gtk::Window,
    title: gtk::Label,
    body: gtk::Label,
    cancel: gtk::Button,
    confirm: gtk::Button,
    actions: UnboundedSender<Action>,
    question: RefCell<Option<Question>>,
    left: Cell<u32>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl CountdownDialog {
    pub fn new(app: &adw::Application, actions: &UnboundedSender<Action>) -> Rc<Self> {
        let window = gtk::Window::builder().application(app).build();
        window.add_css_class("koje-dialog");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-dialog"));
        window.set_keyboard_mode(KeyboardMode::Exclusive);

        let title = gtk::Label::builder().css_classes(["title-2"]).build();
        let body = gtk::Label::builder().wrap(true).max_width_chars(40).build();
        let cancel = gtk::Button::builder()
            .css_classes(["pill"])
            .hexpand(true)
            .build();
        let confirm = gtk::Button::builder()
            .css_classes(["pill"])
            .hexpand(true)
            .build();
        let buttons = gtk::Box::builder().spacing(12).homogeneous(true).build();
        buttons.append(&cancel);
        buttons.append(&confirm);
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
        window.set_default_widget(Some(&confirm));

        let me = Rc::new(Self {
            window,
            title,
            body,
            cancel,
            confirm,
            actions: actions.clone(),
            question: RefCell::new(None),
            left: Cell::new(0),
            timer: RefCell::new(None),
        });
        {
            let me2 = Rc::downgrade(&me);
            me.confirm.connect_clicked(move |_| {
                if let Some(me) = me2.upgrade() {
                    me.answer(true);
                }
            });
        }
        {
            let me2 = Rc::downgrade(&me);
            me.cancel.connect_clicked(move |_| {
                if let Some(me) = me2.upgrade() {
                    me.answer(false);
                }
            });
        }
        let keys = gtk::EventControllerKey::new();
        {
            let me2 = Rc::downgrade(&me);
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape
                    && let Some(me) = me2.upgrade()
                {
                    me.answer(false);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }
        me.window.add_controller(keys);
        me
    }

    /// Ask, counting down from `seconds`; a new question replaces the old.
    pub fn ask(self: &Rc<Self>, q: Question, seconds: u32) {
        self.stop();
        self.title.set_label(&q.title);
        self.body.set_label(&(q.text)(seconds));
        self.cancel.set_label(q.cancel_label);
        self.confirm.set_label(q.confirm_label);
        for c in ["suggested-action", "destructive-action"] {
            self.confirm.remove_css_class(c);
        }
        self.confirm.add_css_class(if q.destructive {
            "destructive-action"
        } else {
            "suggested-action"
        });
        self.question.replace(Some(q));
        self.left.set(seconds);
        self.window.present();
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(me) = me.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let left = me.left.get().saturating_sub(1);
            me.left.set(left);
            if left == 0 {
                me.timer.borrow_mut().take();
                let q = me.question.borrow_mut().take();
                if let Some(action) = q.and_then(|q| q.on_timeout) {
                    let _ = me.actions.send(action);
                }
                me.window.set_visible(false);
                return glib::ControlFlow::Break;
            }
            if let Some(q) = me.question.borrow().as_ref() {
                me.body.set_label(&(q.text)(left));
            }
            glib::ControlFlow::Continue
        });
        self.timer.replace(Some(id));
    }

    fn answer(&self, confirmed: bool) {
        self.stop();
        let q = self.question.borrow_mut().take();
        if let Some(q) = q {
            let action = if confirmed {
                Some(q.on_confirm)
            } else {
                q.on_cancel
            };
            if let Some(a) = action {
                let _ = self.actions.send(a);
            }
        }
        self.window.set_visible(false);
    }

    fn stop(&self) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
    }
}
