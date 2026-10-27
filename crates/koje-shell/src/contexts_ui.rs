//! The koje menu on a host workspace (M8.56): every context with its status
//! and memory and what can be done with it – pause, wake, stop, restart,
//! start – and a form for a new context. Renders a [`ContextList`].

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

use crate::menu::{ContextList, ContextListRow, PALETTE, context_name};
use crate::menu_ui::Meter;
use crate::model::{Action, Status};

/// How long a stop or restart button waits for the confirming click.
const CONFIRM: Duration = Duration::from_secs(4);

/// What a button does that needs a second click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Stop,
    Restart,
}

#[derive(Debug)]
pub struct ContextListPage {
    pub root: gtk::Box,
    list: gtk::ListBox,
    popover: gtk::Popover,
    actions: UnboundedSender<Action>,
    /// A stop or restart clicked once: `(context, which)`.
    armed: Rc<RefCell<Option<(String, Confirm)>>>,
    /// The context names, for checking a new one.
    names: Rc<RefCell<Vec<String>>>,
    form: NewForm,
    /// What the list shows now: re-rendered only when it changes.
    shown: RefCell<Option<ContextList>>,
}

/// "New context…": a name and a color.
#[derive(Debug)]
struct NewForm {
    reveal: gtk::Button,
    revealer: gtk::Revealer,
    name: adw::EntryRow,
    create: gtk::Button,
    spinner: gtk::Spinner,
    error: gtk::Label,
}

impl ContextListPage {
    pub fn new(popover: &gtk::Popover, actions: &UnboundedSender<Action>) -> Rc<Self> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let names: Rc<RefCell<Vec<String>>> = Rc::default();
        let form = NewForm::new(actions, &names);
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        root.append(
            &gtk::Label::builder()
                .label("Contexts")
                .css_classes(["heading"])
                .halign(gtk::Align::Start)
                .build(),
        );
        root.append(&list);
        let settings = gtk::Button::builder()
            .label("koje Settings…")
            .css_classes(["flat"])
            .halign(gtk::Align::Start)
            .build();
        settings.connect_clicked(|b| {
            if let Some(p) = b
                .ancestor(gtk::Popover::static_type())
                .and_downcast::<gtk::Popover>()
            {
                p.popdown();
            }
            crate::settings_ui::open(None);
        });
        let buttons = gtk::Box::builder().spacing(6).build();
        buttons.append(&form.reveal);
        buttons.append(&settings);
        root.append(&buttons);
        root.append(&form.revealer);
        Rc::new(Self {
            root,
            list,
            popover: popover.clone(),
            actions: actions.clone(),
            armed: Rc::default(),
            names,
            form,
            shown: RefCell::new(None),
        })
    }

    pub fn render(self: &Rc<Self>, o: &ContextList) {
        self.form.render(o);
        if self.shown.borrow().as_ref() == Some(o) {
            return;
        }
        self.names
            .replace(o.rows.iter().map(|r| r.name.clone()).collect());
        self.rebuild(o);
        self.shown.replace(Some(o.clone()));
    }

    /// Build the list again, e.g. after a click armed a button.
    fn rebuild(self: &Rc<Self>, o: &ContextList) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if o.rows.is_empty() {
            self.list.append(
                &adw::ActionRow::builder()
                    .title("No contexts yet")
                    .css_classes(["dim-label"])
                    .build(),
            );
            return;
        }
        for r in &o.rows {
            self.list.append(&self.row(r));
        }
    }

    fn row(self: &Rc<Self>, r: &ContextListRow) -> adw::ActionRow {
        let name = gtk::Label::builder()
            .label(&r.name)
            .css_classes(["capsule", &r.accent.css_class()])
            .valign(gtk::Align::Center)
            .build();
        let subtitle = match (&r.busy, &r.error, r.status) {
            (Some(b), _, _) => b.label().to_owned(),
            (None, Some(e), _) => e.clone(),
            (None, None, Status::Paused) => "paused".to_owned(),
            (None, None, Status::Stopped) => "stopped".to_owned(),
            (None, None, Status::Running) => String::new(),
        };
        let row = adw::ActionRow::builder()
            .subtitle(glib::markup_escape_text(&subtitle))
            .subtitle_lines(3)
            .build();
        if r.error.is_some() && r.busy.is_none() {
            row.add_css_class("error");
        }
        row.add_prefix(&name);
        if let Some(m) = &r.memory {
            let meter = Meter::new();
            meter.set(Some(m));
            row.add_suffix(&meter.area);
            row.add_suffix(&meter.label);
            row.set_tooltip_text(Some(&m.tooltip));
        }
        if r.busy.is_some() {
            row.add_suffix(&gtk::Spinner::builder().spinning(true).build());
            return row;
        }
        let button = |icon: &str, tip: &str, action: Action| {
            let b = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tip)
                .valign(gtk::Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            let actions = self.actions.clone();
            b.connect_clicked(move |_| {
                let _ = actions.send(action.clone());
            });
            b
        };
        let ctx = r.name.clone();
        match r.status {
            Status::Running => {
                row.add_suffix(&button(
                    "media-playback-pause-symbolic",
                    "Pause",
                    Action::Pause(ctx.clone()),
                ));
                row.add_suffix(&self.confirmed(&ctx, Confirm::Restart));
                row.add_suffix(&self.confirmed(&ctx, Confirm::Stop));
                // A click on the row shows the context.
                row.set_activatable(true);
                let (actions, popover) = (self.actions.clone(), self.popover.clone());
                row.connect_activated(move |_| {
                    let _ = actions.send(Action::FocusContext(ctx.clone()));
                    popover.popdown();
                });
            }
            Status::Paused => {
                row.add_suffix(&button(
                    "media-playback-start-symbolic",
                    "Wake",
                    Action::Wake(ctx.clone()),
                ));
                row.add_suffix(&self.confirmed(&ctx, Confirm::Stop));
            }
            Status::Stopped => {
                row.add_suffix(&button(
                    "media-playback-start-symbolic",
                    "Start",
                    Action::Start(ctx),
                ));
            }
        }
        row
    }

    /// Stop or restart: the first click arms it (red, "click again"), a
    /// second within [`CONFIRM`] does it.
    fn confirmed(self: &Rc<Self>, ctx: &str, which: Confirm) -> gtk::Button {
        let (icon, tip, action) = match which {
            Confirm::Stop => (
                "system-shutdown-symbolic",
                "Stop",
                Action::Stop(ctx.to_owned()),
            ),
            Confirm::Restart => (
                "view-refresh-symbolic",
                "Restart",
                Action::Restart(ctx.to_owned()),
            ),
        };
        let armed = self.armed.borrow().as_ref() == Some(&(ctx.to_owned(), which));
        let b = gtk::Button::builder()
            .icon_name(icon)
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .tooltip_text(if armed {
                format!("Click again to {} {ctx}", tip.to_lowercase())
            } else {
                tip.to_owned()
            })
            .build();
        if armed {
            b.add_css_class("destructive-action");
        }
        let (page, ctx) = (Rc::downgrade(self), ctx.to_owned());
        b.connect_clicked(move |_| {
            let Some(page) = page.upgrade() else {
                return;
            };
            let key = (ctx.clone(), which);
            if page.armed.borrow().as_ref() == Some(&key) {
                page.armed.replace(None);
                let _ = page.actions.send(action.clone());
            } else {
                page.armed.replace(Some(key.clone()));
                let weak = Rc::downgrade(&page);
                glib::timeout_add_local_once(CONFIRM, move || {
                    if let Some(page) = weak.upgrade()
                        && page.armed.borrow().as_ref() == Some(&key)
                    {
                        page.armed.replace(None);
                        page.redraw();
                    }
                });
            }
            page.redraw();
        });
        b
    }

    fn redraw(self: &Rc<Self>) {
        let shown = self.shown.borrow().clone();
        if let Some(o) = shown {
            self.rebuild(&o);
        }
    }
}

impl NewForm {
    fn new(actions: &UnboundedSender<Action>, names: &Rc<RefCell<Vec<String>>>) -> Self {
        let reveal = gtk::Button::builder()
            .label("New context…")
            .css_classes(["flat"])
            .halign(gtk::Align::Start)
            .build();
        let name = adw::EntryRow::builder().title("Name").build();
        let rows = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        rows.append(&name);

        let color: Rc<RefCell<Option<&'static str>>> = Rc::default();
        let swatches = swatches(&color);

        let create = gtk::Button::builder()
            .label("Create and start")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk::Align::End)
            .build();
        let spinner = gtk::Spinner::builder().visible(false).build();
        let error = gtk::Label::builder()
            .css_classes(["caption", "error"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .visible(false)
            .build();
        let bottom = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::End)
            .build();
        bottom.append(&spinner);
        bottom.append(&create);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        content.append(&rows);
        content.append(&swatches);
        content.append(&error);
        content.append(&bottom);
        let revealer = gtk::Revealer::builder().child(&content).build();

        {
            let (revealer, name) = (revealer.clone(), name.clone());
            reveal.connect_clicked(move |_| {
                let open = !revealer.reveals_child();
                revealer.set_reveal_child(open);
                if open {
                    name.grab_focus();
                }
            });
        }
        let submit = {
            let (actions, names, name, color, error) = (
                actions.clone(),
                names.clone(),
                name.clone(),
                color.clone(),
                error.clone(),
            );
            move || match context_name(&name.text(), &names.borrow()) {
                Ok(n) => {
                    error.set_visible(false);
                    let _ = actions.send(Action::Create {
                        name: n,
                        color: color.borrow().map(str::to_owned),
                    });
                }
                Err(e) => {
                    error.set_label(&e);
                    error.set_visible(true);
                }
            }
        };
        let submit = Rc::new(submit);
        {
            let submit = submit.clone();
            create.connect_clicked(move |_| submit());
        }
        name.connect_entry_activated(move |_| submit());

        Self {
            reveal,
            revealer,
            name,
            create,
            spinner,
            error,
        }
    }

    /// Busy while creating; on success the form closes and empties.
    fn render(&self, o: &ContextList) {
        let was_creating = self.spinner.is_visible();
        self.spinner.set_visible(o.creating);
        self.spinner.set_spinning(o.creating);
        self.create.set_sensitive(!o.creating);
        if let Some(e) = &o.create_error {
            self.error.set_label(e);
            self.error.set_visible(true);
        } else if was_creating && !o.creating {
            self.error.set_visible(false);
            self.name.set_text("");
            self.revealer.set_reveal_child(false);
        }
    }
}

/// Automatic, then the palette; exactly one is chosen, into `color`.
fn swatches(color: &Rc<RefCell<Option<&'static str>>>) -> gtk::Box {
    let swatches = gtk::Box::builder().spacing(6).build();
    let auto = gtk::ToggleButton::builder()
        .label("Auto")
        .tooltip_text("Let koje choose a color")
        .css_classes(["pill", "swatch-auto"])
        .active(true)
        .build();
    swatches.append(&auto);
    {
        let color = color.clone();
        auto.connect_toggled(move |b| {
            if b.is_active() {
                color.replace(None);
            }
        });
    }
    for (label, hex) in PALETTE {
        let swatch = gtk::ToggleButton::builder()
            .tooltip_text(label)
            .css_classes([
                "circular",
                "swatch",
                &format!("swatch-{}", label.to_lowercase()),
            ])
            .group(&auto)
            .build();
        let color = color.clone();
        swatch.connect_toggled(move |b| {
            if b.is_active() {
                color.replace(Some(hex));
            }
        });
        swatches.append(&swatch);
    }
    swatches
}
