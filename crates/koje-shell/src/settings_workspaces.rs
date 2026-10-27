//! The settings app's Workspaces page (M8.90, ADR 0055): how the bar shows
//! workspaces, the host workspaces' labels and icons, the overlays' labels,
//! icons and keys. Save writes `config.toml` through `koje config --stdin`
//! (checked as the CLI checks it). The form and its checks are
//! [`crate::settings`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use koje_core::config::GlobalConfig;
use koje_core::look;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{self, BarForm, STYLES, WorkspaceRow};
use crate::settings_icons::icon_button;
use crate::settings_ui::{Ui, clear_page, warn_long_label};

/// Shown as the sidebar's second row.
pub(crate) struct WorkspacesPage {
    pub(crate) page: adw::PreferencesPage,
    /// The config the page was built from; it is rebuilt when that changes
    /// and the user changed nothing.
    built: Rc<RefCell<Option<GlobalConfig>>>,
    dirty: Rc<Cell<bool>>,
}

impl WorkspacesPage {
    pub(crate) fn new() -> Self {
        Self {
            page: adw::PreferencesPage::new(),
            built: Rc::default(),
            dirty: Rc::default(),
        }
    }

    pub(crate) fn render(&self, ui: &Rc<Ui>, global: &GlobalConfig) {
        if self.dirty.get() || self.built.borrow().as_ref() == Some(global) {
            return;
        }
        self.built.replace(Some(global.clone()));
        clear_page(&self.page);
        let form = Rc::new(RefCell::new(settings::bar_form(global)));
        let changed: Rc<dyn Fn()> = {
            let dirty = self.dirty.clone();
            Rc::new(move || dirty.set(true))
        };
        self.page.add(&style(&form, &changed));
        let hosts = adw::PreferencesGroup::builder()
            .title("Host Workspaces")
            .description("Numbered workspaces outside the contexts. One that is not listed shows its number. A context's name and icon are set on its page. The bar shows names in full; long ones make it wide.")
            .build();
        let list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::None)
            .build();
        fill_workspaces(&list, &form, &changed);
        hosts.add(&list);
        self.page.add(&hosts);
        self.page.add(&overlays(&form, &changed));
        self.page.add(&self.save(ui, &form));
    }

    fn save(&self, ui: &Rc<Ui>, form: &Rc<RefCell<BarForm>>) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        let button = gtk::Button::builder()
            .label("Save")
            .halign(gtk::Align::End)
            .css_classes(["suggested-action", "pill"])
            .build();
        let (ui, form, dirty) = (Rc::downgrade(ui), form.clone(), self.dirty.clone());
        let built = self.built.clone();
        button.connect_clicked(move |_| {
            let Some(ui) = ui.upgrade() else { return };
            match settings::bar_settings(&form.borrow()) {
                Err(errors) => ui.toast(
                    &errors
                        .iter()
                        .map(|e| e.message.as_str())
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
                Ok(s) => match new_text(&s) {
                    Ok(text) => {
                        ui.command("config.toml", &["config", "--stdin"], Some(text));
                        // Built again from the file as saved.
                        dirty.set(false);
                        built.replace(None);
                    }
                    Err(e) => ui.toast(&e),
                },
            }
        });
        group.add(&button);
        group
    }
}

/// How the bar shows workspaces.
fn style(form: &Rc<RefCell<BarForm>>, changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Bar").build();
    let model = gtk::StringList::new(&STYLES.map(|(_, title)| title));
    let row = adw::ComboRow::builder()
        .title("Workspaces")
        .model(&model)
        .build();
    let current = STYLES
        .iter()
        .position(|(s, _)| *s == form.borrow().style)
        .unwrap_or(0);
    row.set_selected(u32::try_from(current).unwrap_or(0));
    let (form, changed) = (form.clone(), changed.clone());
    row.connect_selected_notify(move |r| {
        if let Some((style, _)) = STYLES.get(r.selected() as usize) {
            form.borrow_mut().style = *style;
            changed();
        }
    });
    group.add(&row);
    group
}

/// A row per host workspace – icon, label, remove – then "Add Workspace".
fn fill_workspaces(list: &gtk::ListBox, form: &Rc<RefCell<BarForm>>, changed: &Rc<dyn Fn()>) {
    list.remove_all();
    let rows = form.borrow().workspaces.clone();
    for (i, w) in rows.iter().enumerate() {
        let row = adw::EntryRow::builder()
            .title(format!("Workspace {}", w.number))
            .text(&w.label)
            .build();
        warn_long_label(&row);
        {
            let (form, changed) = (form.clone(), changed.clone());
            row.add_prefix(&icon_button(&w.icon, look::HOST_ICON, move |name| {
                if let Some(w) = form.borrow_mut().workspaces.get_mut(i) {
                    w.icon = name;
                }
                changed();
            }));
        }
        {
            let (form, changed) = (form.clone(), changed.clone());
            row.connect_changed(move |e| {
                if let Some(w) = form.borrow_mut().workspaces.get_mut(i) {
                    w.label = e.text().to_string();
                }
                changed();
            });
        }
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .tooltip_text("Remove")
            .build();
        {
            let (list, form, changed) = (list.clone(), form.clone(), changed.clone());
            remove.connect_clicked(move |_| {
                form.borrow_mut().workspaces.remove(i);
                changed();
                fill_workspaces(&list, &form, &changed);
            });
        }
        row.add_suffix(&remove);
        list.append(&row);
    }
    let add = adw::ButtonRow::builder()
        .title("Add Workspace")
        .start_icon_name("list-add-symbolic")
        .build();
    let (list2, form2, changed2) = (list.clone(), form.clone(), changed.clone());
    add.connect_activated(move |_| {
        {
            let mut f = form2.borrow_mut();
            let number = settings::next_workspace(&f);
            f.workspaces.push(WorkspaceRow {
                number,
                label: String::new(),
                icon: String::new(),
            });
            f.workspaces.sort_by_key(|w| w.number);
        }
        changed2();
        fill_workspaces(&list2, &form2, &changed2);
    });
    list.append(&add);
}

/// Each overlay: its icon, label and key.
fn overlays(form: &Rc<RefCell<BarForm>>, changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Overlays")
        .description("Windows above the workspaces, shown and hidden by their key. The bar shows them by their icon.")
        .build();
    let edited: Rc<dyn Fn()> = {
        let (form, changed) = (form.clone(), changed.clone());
        Rc::new(move || {
            form.borrow_mut().overlays_changed = true;
            changed();
        })
    };
    let rows = form.borrow().overlays.clone();
    for (i, o) in rows.iter().enumerate() {
        let expander = adw::ExpanderRow::builder()
            .title(if o.label.is_empty() {
                &o.name
            } else {
                &o.label
            })
            .subtitle(&o.key)
            .build();
        {
            let (form, edited) = (form.clone(), edited.clone());
            expander.add_prefix(&icon_button(
                &o.icon,
                look::overlay_icon(&o.name),
                move |n| {
                    if let Some(o) = form.borrow_mut().overlays.get_mut(i) {
                        o.icon = n;
                    }
                    edited();
                },
            ));
        }
        let label = adw::EntryRow::builder()
            .title(format!("Name (empty: {})", o.name))
            .text(&o.label)
            .build();
        {
            let (form, edited, expander) = (form.clone(), edited.clone(), expander.clone());
            let name = o.name.clone();
            label.connect_changed(move |e| {
                let text = e.text().to_string();
                expander.set_title(if text.trim().is_empty() { &name } else { &text });
                if let Some(o) = form.borrow_mut().overlays.get_mut(i) {
                    o.label = text;
                }
                edited();
            });
        }
        let key = adw::EntryRow::builder()
            .title("Key (e.g. SUPER+M)")
            .text(&o.key)
            .build();
        {
            let (form, edited, expander) = (form.clone(), edited.clone(), expander.clone());
            key.connect_changed(move |e| {
                let text = e.text().to_string();
                expander.set_subtitle(&text);
                if let Some(o) = form.borrow_mut().overlays.get_mut(i) {
                    o.key = text;
                }
                edited();
            });
        }
        expander.add_row(&label);
        expander.add_row(&key);
        group.add(&expander);
    }
    group
}

/// `config.toml` as it is now (empty when missing), with the page applied.
fn new_text(s: &look::BarSettings) -> Result<String, String> {
    let paths = koje_core::paths::Paths::from_env().map_err(|e| e.to_string())?;
    let path = paths.global_config();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    look::with_bar(&text, s).map_err(|e| e.to_string())
}
