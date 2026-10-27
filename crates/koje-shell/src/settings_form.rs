//! The settings app's Context page (M8.10): labels, icon and color, the access
//! profile and its switches, folders, memory limits; Save writes
//! `context.toml` through `koje edit --stdin` (checked as the CLI checks
//! it), Recreate and Remove ask first. The form and its checks are
//! [`crate::settings`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use koje_core::access::Profile;
use koje_core::config::ContextConfig;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{self, Form, effective};
use crate::settings_ui::{Ui, clear_page};

/// The profile choices: `config.toml`'s, then the four profiles.
fn profile_names() -> Vec<String> {
    let mut names = vec!["From config.toml".to_owned()];
    names.extend(Profile::ALL.iter().map(|p| {
        let s = p.as_str();
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    }));
    names
}

/// GNOME-like titles for the switches.
fn switch_title(name: &str) -> &'static str {
    match name {
        "gpu" => "Graphics card",
        "sound" => "Sound",
        "wayland" => "Windows on the desktop",
        "nested" => "Containers inside",
        "network" => "Network",
        "notifications" => "Notifications",
        "file_chooser" => "File chooser",
        "settings" => "Desktop settings",
        "inhibit" => "Keep the screen on",
        "screencast" => "Screen sharing",
        "print" => "Printing",
        _ => "Other",
    }
}

pub(crate) fn build(ui: &Rc<Ui>, name: &str, cfg: &ContextConfig, initial: Form) {
    let page = ui.context_page();
    clear_page(page);
    let form = Rc::new(RefCell::new(initial));
    let changed: Rc<dyn Fn()> = {
        let ui = Rc::downgrade(ui);
        Rc::new(move || {
            if let Some(ui) = ui.upgrade() {
                ui.set_dirty();
            }
        })
    };
    let global = ui.global();
    let (look, prefix, color) = appearance(&form, &changed);
    page.add(&look);
    page.add(&access(&form, &changed, &global));
    page.add(&folders(&form, &changed));
    let (memory, soft, hard) = memory(&form, &changed, &global);
    page.add(&memory);
    let fields = vec![
        ("prefix", prefix),
        ("color", color),
        ("memory_soft", soft),
        ("memory_hard", hard),
    ];
    page.add(&save(ui, &form, name, fields));
    page.add(&container(ui, name, cfg));
}

/// The short label and the color, with the palette.
fn appearance(
    form: &Rc<RefCell<Form>>,
    changed: &Rc<dyn Fn()>,
) -> (adw::PreferencesGroup, adw::EntryRow, adw::EntryRow) {
    // Label and color.
    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    let prefix = adw::EntryRow::builder()
        .title("Short label")
        .text(&form.borrow().prefix)
        .build();
    {
        let (form, changed) = (form.clone(), changed.clone());
        prefix.connect_changed(move |e| {
            form.borrow_mut().prefix = e.text().to_string();
            changed();
        });
    }
    let label = adw::EntryRow::builder()
        .title("Name in the bar (empty: the context's name)")
        .text(&form.borrow().label)
        .build();
    {
        let (form, changed) = (form.clone(), changed.clone());
        label.connect_changed(move |e| {
            form.borrow_mut().label = e.text().to_string();
            changed();
        });
    }
    look.add(&label);
    let icon = adw::ActionRow::builder()
        .title("Icon")
        .subtitle("In the bar beside the name")
        .build();
    {
        let current = form.borrow().icon.clone();
        let (form, changed) = (form.clone(), changed.clone());
        icon.add_suffix(&crate::settings_icons::icon_button(
            &current,
            koje_core::look::CONTEXT_ICON,
            move |name| {
                form.borrow_mut().icon = name;
                changed();
            },
        ));
    }
    look.add(&icon);
    look.add(&prefix);
    let color = adw::EntryRow::builder()
        .title("Color")
        .text(&form.borrow().color)
        .build();
    {
        let (form, changed) = (form.clone(), changed.clone());
        color.connect_changed(move |e| {
            form.borrow_mut().color = e.text().to_string();
            changed();
        });
    }
    let swatches = gtk::Box::builder()
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    for (label, hex) in crate::menu::PALETTE {
        let b = gtk::Button::builder()
            .tooltip_text(label)
            .css_classes(["swatch-button", &format!("swatch-{}", label.to_lowercase())])
            .build();
        let color = color.clone();
        b.connect_clicked(move |_| color.set_text(hex));
        swatches.append(&b);
    }
    color.add_suffix(&swatches);
    look.add(&color);

    (look, prefix, color)
}

/// The profile and every switch, showing what applies.
fn access(
    form: &Rc<RefCell<Form>>,
    changed: &Rc<dyn Fn()>,
    global: &koje_core::config::GlobalConfig,
) -> adw::PreferencesGroup {
    // Access: the profile and every switch.
    let access = adw::PreferencesGroup::builder()
        .title("Access")
        .description("What the context gets from the host. A profile sets every switch; a changed switch is kept as the context's own.")
        .build();
    let names = profile_names();
    let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let profile = adw::ComboRow::builder()
        .title("Profile")
        .model(&model)
        .build();
    let selected = form
        .borrow()
        .profile
        .and_then(|p| Profile::ALL.iter().position(|x| *x == p))
        .map_or(0, |i| i + 1);
    profile.set_selected(u32::try_from(selected).unwrap_or(0));
    access.add(&profile);
    let updating = Rc::new(Cell::new(false));
    let switch_rows: Vec<(&'static str, adw::SwitchRow)> = form
        .borrow()
        .switches
        .iter()
        .map(|(n, own)| {
            let row = adw::SwitchRow::builder().title(switch_title(n)).build();
            if own.is_some() {
                row.set_subtitle("Set for this context");
            }
            (*n, row)
        })
        .collect();
    let sync = {
        let (form, rows, updating, global) = (
            form.clone(),
            switch_rows.clone(),
            updating.clone(),
            global.clone(),
        );
        move || {
            updating.set(true);
            for ((_, on), (_, row)) in effective(&form.borrow(), &global).iter().zip(&rows) {
                row.set_active(*on);
            }
            updating.set(false);
        }
    };
    sync();
    {
        let (form, sync, changed) = (form.clone(), sync.clone(), changed.clone());
        profile.connect_selected_notify(move |c| {
            let i = c.selected() as usize;
            form.borrow_mut().profile = i.checked_sub(1).and_then(|i| Profile::ALL.get(i).copied());
            sync();
            changed();
        });
    }
    for (name, row) in &switch_rows {
        let (form, updating, changed, name) =
            (form.clone(), updating.clone(), changed.clone(), *name);
        row.connect_active_notify(move |r| {
            if updating.get() {
                return;
            }
            if let Some(s) = form
                .borrow_mut()
                .switches
                .iter_mut()
                .find(|(n, _)| *n == name)
            {
                s.1 = Some(r.is_active());
            }
            r.set_subtitle("Set for this context");
            changed();
        });
        access.add(row);
    }

    access
}

fn folders(form: &Rc<RefCell<Form>>, changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    // Folders.
    let folders = adw::PreferencesGroup::builder()
        .title("Folders")
        .description("Host folders the context sees at the same path.")
        .build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    fill_mounts(&list, form, changed);
    folders.add(&list);

    folders
}

/// The limits; empty takes `config.toml`'s.
fn memory(
    form: &Rc<RefCell<Form>>,
    changed: &Rc<dyn Fn()>,
    global: &koje_core::config::GlobalConfig,
) -> (adw::PreferencesGroup, adw::EntryRow, adw::EntryRow) {
    // Memory.
    let defaults = &global.default_limits;
    let memory = adw::PreferencesGroup::builder()
        .title("Memory")
        .description(limits_hint(defaults))
        .build();
    let soft = adw::EntryRow::builder()
        .title("Warn at (e.g. 6G)")
        .text(&form.borrow().memory_soft)
        .build();
    let hard = adw::EntryRow::builder()
        .title("Shut down at (e.g. 8G)")
        .text(&form.borrow().memory_hard)
        .build();
    {
        let (form, changed) = (form.clone(), changed.clone());
        soft.connect_changed(move |e| {
            form.borrow_mut().memory_soft = e.text().to_string();
            changed();
        });
    }
    {
        let (form, changed) = (form.clone(), changed.clone());
        hard.connect_changed(move |e| {
            form.borrow_mut().memory_hard = e.text().to_string();
            changed();
        });
    }
    memory.add(&soft);
    memory.add(&hard);

    (memory, soft, hard)
}

/// Check the form, mark bad fields, else write it with `koje edit --stdin`.
fn save(
    ui: &Rc<Ui>,
    form: &Rc<RefCell<Form>>,
    name: &str,
    fields: Vec<(&'static str, adw::EntryRow)>,
) -> adw::PreferencesGroup {
    // Save.
    let save_group = adw::PreferencesGroup::new();
    let save = gtk::Button::builder()
        .label("Save")
        .halign(gtk::Align::End)
        .css_classes(["suggested-action", "pill"])
        .build();
    {
        let (ui, form, name) = (Rc::downgrade(ui), form.clone(), name.to_owned());
        save.connect_clicked(move |_| {
            let Some(ui) = ui.upgrade() else { return };
            for (_, row) in &fields {
                row.remove_css_class("error");
            }
            match settings::settings(&form.borrow()) {
                Err(errors) => {
                    for e in &errors {
                        if let Some((_, row)) = fields.iter().find(|(f, _)| *f == e.field) {
                            row.add_css_class("error");
                        }
                    }
                    ui.toast(
                        &errors
                            .iter()
                            .map(|e| e.message.as_str())
                            .collect::<Vec<_>>()
                            .join(" · "),
                    );
                }
                Ok(s) => match new_text(&name, &s) {
                    Ok(text) => {
                        ui.command(&name, &["edit", &name, "--stdin"], Some(text));
                        ui.saved();
                    }
                    Err(e) => ui.toast(&e),
                },
            }
        });
    }
    save_group.add(&save);

    save_group
}

/// The image with Recreate, and Remove; both ask first.
fn container(ui: &Rc<Ui>, name: &str, cfg: &ContextConfig) -> adw::PreferencesGroup {
    // The container.
    let container = adw::PreferencesGroup::builder().title("Container").build();
    let image = adw::ActionRow::builder()
        .title("Image")
        .subtitle(&cfg.image)
        .build();
    let recreate = gtk::Button::builder()
        .label("Recreate…")
        .valign(gtk::Align::Center)
        .build();
    {
        let (ui, name) = (Rc::downgrade(ui), name.to_owned());
        recreate.connect_clicked(move |_| {
            if let Some(ui) = ui.upgrade() {
                ui.ask(
                    &format!("Recreate {name}?"),
                    "A new container from the current settings – a new image and access apply only then. Windows are saved and opened again; what was installed in the old container outside the home is lost.",
                    "Recreate",
                    vec!["recreate".into(), name.clone(), "--yes".into()],
                    &name,
                );
            }
        });
    }
    image.add_suffix(&recreate);
    container.add(&image);
    let remove = adw::ButtonRow::builder()
        .title("Remove Context…")
        .css_classes(["destructive-action"])
        .build();
    {
        let (ui, name) = (Rc::downgrade(ui), name.to_owned());
        remove.connect_activated(move |_| {
            if let Some(ui) = ui.upgrade() {
                ui.ask(
                    &format!("Remove {name}?"),
                    "The context, its container, its home and all its states are deleted.",
                    "Remove",
                    vec!["rm".into(), name.clone(), "--yes".into()],
                    &name,
                );
            }
        });
    }
    container.add(&remove);
    container
}

/// What an empty limit means: `config.toml`'s, or none.
fn limits_hint(defaults: &koje_core::config::Limits) -> String {
    match (defaults.memory_soft, defaults.memory_hard) {
        (None, None) => "Empty: no limit.".to_owned(),
        (soft, hard) => format!(
            "Empty: config.toml's – warn at {}, shut down at {}.",
            soft.map_or("–".to_owned(), |s| s.to_string()),
            hard.map_or("–".to_owned(), |s| s.to_string())
        ),
    }
}

/// The folders as rows with a remove button, then an entry to add one.
fn fill_mounts(list: &gtk::ListBox, form: &Rc<RefCell<Form>>, changed: &Rc<dyn Fn()>) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let mounts = form.borrow().mounts.clone();
    for (i, m) in mounts.iter().enumerate() {
        let row = adw::ActionRow::builder().title(m).build();
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .tooltip_text("Remove")
            .build();
        let (list2, form2, changed2) = (list.clone(), form.clone(), changed.clone());
        remove.connect_clicked(move |_| {
            form2.borrow_mut().mounts.remove(i);
            changed2();
            fill_mounts(&list2, &form2, &changed2);
        });
        row.add_suffix(&remove);
        list.append(&row);
    }
    let add = adw::EntryRow::builder()
        .title("Add a folder (e.g. ~/src)")
        .show_apply_button(true)
        .build();
    let (list2, form2, changed2) = (list.clone(), form.clone(), changed.clone());
    add.connect_apply(move |e| {
        let path = e.text().trim().to_owned();
        if !path.is_empty() {
            form2.borrow_mut().mounts.push(path);
            changed2();
            fill_mounts(&list2, &form2, &changed2);
        }
    });
    list.append(&add);
}

/// `context.toml` as it is now, with the settings applied.
fn new_text(name: &str, s: &koje_core::context::ContextSettings) -> Result<String, String> {
    let paths = koje_core::paths::Paths::from_env().map_err(|e| e.to_string())?;
    let name = koje_core::name::ContextName::new(name).map_err(|e| e.to_string())?;
    let path = paths.context_config(&name);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    koje_core::context::with_settings(&text, s).map_err(|e| e.to_string())
}
